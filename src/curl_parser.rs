/// Parses a full curl command and reconstructs arguments
/// injecting -i (include headers) and -s (silent) to allow parsing the output.
pub struct CurlCommand {
    pub url: String,
    pub method: Option<String>,
    pub headers: Vec<(String, String)>,
    pub data: Option<String>,
    pub extra_args: Vec<String>,
}

impl CurlCommand {
    pub fn parse(input: &str) -> Self {
        Self::from_tokens(tokenize(input))
    }

    /// Same as `parse`, but `{{name}}` is replaced **after** tokenizing. Doing it
    /// before would let a value containing quotes inject extra curl flags.
    pub fn parse_with_vars(input: &str, vars: &[(String, String)]) -> Self {
        let tokens = tokenize(input)
            .into_iter()
            .map(|token| crate::collection::substitute(&token, vars))
            .collect();
        Self::from_tokens(tokens)
    }

    fn from_tokens(tokens: Vec<String>) -> Self {
        let mut iter = tokens.iter().peekable();

        // Skip 'curl' if present
        if iter.peek().map(|s| s.as_str()) == Some("curl") {
            iter.next();
        }

        let mut url = String::new();
        let mut method: Option<String> = None;
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut data: Option<String> = None;
        let mut extra_args: Vec<String> = Vec::new();

        while let Some(token) = iter.next() {
            match token.as_str() {
                // Method
                "-X" | "--request" => {
                    if let Some(m) = iter.next() {
                        method = Some(m.clone());
                    }
                }
                // Headers
                "-H" | "--header" => {
                    if let Some(h) = iter.next() {
                        if let Some(pos) = h.find(':') {
                            headers.push((
                                h[..pos].trim().to_string(),
                                h[pos + 1..].trim().to_string(),
                            ));
                        }
                    }
                }
                // Body
                "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" => {
                    if let Some(d) = iter.next() {
                        data = Some(d.clone());
                    }
                }
                // Flags we already inject — ignore user-provided ones
                "-i" | "--include" | "-s" | "--silent" | "-v" | "--verbose" | "-N"
                | "--no-buffer" => {}
                // Flags with values we pass through
                "-o" | "--output" | "-u" | "--user" | "--connect-timeout" | "--max-time" | "-m"
                | "--proxy" | "-x" | "--cacert" | "--cert" | "--key" | "--resolve"
                | "--dns-servers" | "-A" | "--user-agent" | "--referer" | "-e" => {
                    if let Some(val) = iter.next() {
                        extra_args.push(token.clone());
                        extra_args.push(val.clone());
                    }
                }
                // Boolean flags we pass through
                "-k" | "--insecure" | "-L" | "--location" | "-g" | "--compressed" | "--http1.0"
                | "--http1.1" | "--http2" | "--http3" | "-I" | "--head" | "--no-keepalive" => {
                    extra_args.push(token.clone());
                }
                t => {
                    if t.starts_with("http://")
                        || t.starts_with("https://")
                        || t.starts_with("ftp://")
                    {
                        url = t.to_string();
                    } else if !t.starts_with('-') && url.is_empty() {
                        // Token without dash and no URL yet → assume it's the URL
                        url = t.to_string();
                    } else if t.starts_with('-') {
                        // Unknown flag: pass it through
                        extra_args.push(t.to_string());
                    }
                }
            }
        }

        CurlCommand {
            url,
            method,
            headers,
            data,
            extra_args,
        }
    }

    /// Builds the argument vector to pass to `curl`,
    /// injecting -i, -s and -N to allow streaming response parsing.
    pub fn to_args_with_headers(&self) -> Vec<String> {
        let mut args = Vec::new();

        args.push("-i".to_string()); // include headers in stdout
        args.push("-s".to_string()); // silence progress bar
        args.push("-N".to_string()); // no output buffering (needed for SSE streaming)

        if let Some(m) = &self.method {
            args.push("-X".to_string());
            args.push(m.clone());
        }

        for (k, v) in &self.headers {
            args.push("-H".to_string());
            args.push(format!("{}: {}", k, v));
        }

        if let Some(d) = &self.data {
            args.push("-d".to_string());
            args.push(d.clone());

            // Auto-inject Content-Type if body looks like JSON and not already defined
            let has_ct = self
                .headers
                .iter()
                .any(|(k, _)| k.to_lowercase() == "content-type");
            if !has_ct && d.trim_start().starts_with('{') {
                args.push("-H".to_string());
                args.push("Content-Type: application/json".to_string());
            }
        }

        for extra in &self.extra_args {
            args.push(extra.clone());
        }

        args.push(self.url.clone());
        args
    }
}

/// Simple shell-like tokenizer: respects single and double quotes
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '\\' if in_double => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ' ' | '\t' | '\n' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let input = "curl https://example.com";
        let tokens = tokenize(input);
        assert_eq!(tokens, vec!["curl", "https://example.com"]);
    }

    #[test]
    fn test_tokenize_with_quotes() {
        let input = "curl -H \"Content-Type: application/json\" https://example.com";
        let tokens = tokenize(input);
        assert_eq!(
            tokens,
            vec![
                "curl",
                "-H",
                "Content-Type: application/json",
                "https://example.com"
            ]
        );
    }

    #[test]
    fn test_tokenize_single_quotes() {
        let input = "curl -H 'Authorization: Bearer token' https://example.com";
        let tokens = tokenize(input);
        assert_eq!(
            tokens,
            vec![
                "curl",
                "-H",
                "Authorization: Bearer token",
                "https://example.com"
            ]
        );
    }

    #[test]
    fn test_tokenize_mixed_quotes() {
        let input = "curl -d '{\"name\":\"test\"}' -H \"X-Custom: value\" https://example.com";
        let tokens = tokenize(input);
        assert_eq!(
            tokens,
            vec![
                "curl",
                "-d",
                "{\"name\":\"test\"}",
                "-H",
                "X-Custom: value",
                "https://example.com"
            ]
        );
    }

    #[test]
    fn test_curl_parse_simple_get() {
        let cmd = CurlCommand::parse("curl https://api.example.com/users");
        assert_eq!(cmd.url, "https://api.example.com/users");
        assert_eq!(cmd.method, None);
        assert!(cmd.headers.is_empty());
        assert!(cmd.data.is_none());
    }

    #[test]
    fn test_curl_parse_with_method() {
        let cmd = CurlCommand::parse("curl -X POST https://api.example.com/users");
        assert_eq!(cmd.url, "https://api.example.com/users");
        assert_eq!(cmd.method, Some("POST".to_string()));
        assert!(cmd.headers.is_empty());
        assert!(cmd.data.is_none());
    }

    #[test]
    fn test_curl_parse_with_headers() {
        let cmd = CurlCommand::parse("curl -H \"Content-Type: application/json\" -H \"Authorization: Bearer token\" https://api.example.com/users");
        assert_eq!(cmd.url, "https://api.example.com/users");
        assert_eq!(cmd.headers.len(), 2);
        assert_eq!(
            cmd.headers[0],
            ("Content-Type".to_string(), "application/json".to_string())
        );
        assert_eq!(
            cmd.headers[1],
            ("Authorization".to_string(), "Bearer token".to_string())
        );
    }

    #[test]
    fn test_curl_parse_with_data() {
        let cmd = CurlCommand::parse("curl -d '{\"name\":\"test\"}' https://api.example.com/users");
        assert_eq!(cmd.url, "https://api.example.com/users");
        assert_eq!(cmd.data, Some("{\"name\":\"test\"}".to_string()));
    }

    #[test]
    fn test_curl_parse_complex() {
        let cmd = CurlCommand::parse("curl -X POST -H \"Content-Type: application/json\" -d '{\"name\":\"test\"}' -k -L https://api.example.com/users");
        assert_eq!(cmd.url, "https://api.example.com/users");
        assert_eq!(cmd.method, Some("POST".to_string()));
        assert_eq!(cmd.headers.len(), 1);
        assert_eq!(cmd.data, Some("{\"name\":\"test\"}".to_string()));
        assert_eq!(cmd.extra_args.len(), 2); // -k and -L
    }

    #[test]
    fn test_to_args_with_headers() {
        let cmd = CurlCommand::parse(
            "curl -X POST -H \"Content-Type: application/json\" https://api.example.com/users",
        );
        let args = cmd.to_args_with_headers();

        // Should include -i and -s automatically
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"-s".to_string()));
        assert!(args.contains(&"-X".to_string()));
        assert!(args.contains(&"POST".to_string()));
        assert!(args.contains(&"-H".to_string()));
        assert!(args.contains(&"Content-Type: application/json".to_string()));
        assert!(args.contains(&"https://api.example.com/users".to_string()));
    }

    #[test]
    fn test_auto_content_type_injection() {
        let cmd = CurlCommand::parse("curl -d '{\"name\":\"test\"}' https://api.example.com/users");
        let args = cmd.to_args_with_headers();

        // Should auto-inject Content-Type for JSON data
        assert!(args.contains(&"Content-Type: application/json".to_string()));
    }

    #[test]
    fn test_no_auto_content_type_when_exists() {
        let cmd = CurlCommand::parse("curl -H \"Content-Type: text/plain\" -d '{\"name\":\"test\"}' https://api.example.com/users");
        let args = cmd.to_args_with_headers();

        // Should not duplicate Content-Type
        let content_type_count = args
            .iter()
            .filter(|arg| arg.contains("Content-Type"))
            .count();
        assert_eq!(content_type_count, 1);
    }

    #[test]
    fn parse_with_vars_substitutes_inside_a_token() {
        let vars = vec![("base".to_string(), "http://x".to_string())];
        let parsed = CurlCommand::parse_with_vars("curl {{base}}/a -d '{{base}}'", &vars);

        assert_eq!(parsed.url, "http://x/a");
        assert_eq!(parsed.data.as_deref(), Some("http://x"));
    }

    #[test]
    fn parse_with_vars_cannot_inject_flags() {
        // Substituting after tokenizing means a quote in the value stays inside
        // the argument instead of closing it and adding a new flag.
        let vars = vec![("pw".to_string(), "x' -o /tmp/pwn '".to_string())];
        let parsed = CurlCommand::parse_with_vars("curl http://x -d '{{pw}}'", &vars);

        assert_eq!(parsed.data.as_deref(), Some("x' -o /tmp/pwn '"));
        assert!(
            parsed.extra_args.is_empty(),
            "value injected flags: {:?}",
            parsed.extra_args
        );
    }
}
