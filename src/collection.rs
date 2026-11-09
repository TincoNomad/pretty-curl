//! Plain-text request collections — a file of named curl commands.
//!
//! Format:
//! ```text
//! @base = http://localhost:3000
//! @token = dev-token
//!
//! ## Usuarios
//! ### get user
//! curl {{base}}/users/1 -H "Authorization: Bearer {{token}}"
//!
//! ### create user
//! curl -X POST {{base}}/users -d '{"name":"Juan"}'
//! ```
//!
//! `###` starts a named request, `##` a group (label only), `@name = value`
//! declares a variable, `#` is a comment. A request's curl may span several
//! lines using a trailing `\`.

use std::fs;

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub name: String,
    pub group: Option<String>,
    pub curl: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collection {
    pub variables: Vec<(String, String)>,
    pub requests: Vec<Request>,
}

impl Collection {
    pub fn load(path: &str) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("cannot read collection '{}': {}", path, e))?;
        Ok(Self::parse(&text))
    }

    pub fn parse(text: &str) -> Self {
        let mut col = Collection::default();
        let mut group: Option<String> = None;
        let mut cur: Option<(String, Vec<String>)> = None;

        for raw in text.lines() {
            let trimmed = raw.trim();

            if let Some(name) = trimmed.strip_prefix("###") {
                Self::flush(&mut col, &group, &mut cur);
                cur = Some((name.trim().to_string(), Vec::new()));
                continue;
            }
            if let Some(name) = trimmed.strip_prefix("##") {
                Self::flush(&mut col, &group, &mut cur);
                group = Some(name.trim().to_string());
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.strip_prefix('@').and_then(|r| r.split_once('=')) {
                col.variables
                    .push((key.trim().to_string(), value.trim().to_string()));
                continue;
            }
            if let Some((_, lines)) = cur.as_mut() {
                // A trailing backslash continues the curl command on the next line.
                let line = trimmed.strip_suffix('\\').unwrap_or(trimmed);
                lines.push(line.trim().to_string());
            }
        }
        Self::flush(&mut col, &group, &mut cur);
        col
    }

    fn flush(
        col: &mut Collection,
        group: &Option<String>,
        cur: &mut Option<(String, Vec<String>)>,
    ) {
        if let Some((name, lines)) = cur.take() {
            let curl = lines.join(" ");
            if !curl.is_empty() {
                col.requests.push(Request {
                    name,
                    group: group.clone(),
                    curl,
                });
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Request> {
        self.requests.iter().find(|r| r.name == name)
    }

    /// Load variables from an environment file (same `@name = value` syntax).
    pub fn load_vars(path: &str) -> Result<Vec<(String, String)>, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read env '{}': {}", path, e))?;
        Ok(Self::parse(&text).variables)
    }
}

/// Replaces `{{name}}` placeholders using the given variables.
///
/// shortcut: single pass, no recursive expansion — variables whose values
/// contain other `{{...}}` are not re-expanded. Upgrade if nesting is needed.
pub fn substitute(text: &str, vars: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{}}}}}", key), value);
    }
    out
}

/// Merges `extra` into `base`; values in `extra` win, keys stay unique.
pub fn merge_vars(base: &[(String, String)], extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = base.to_vec();
    for (key, value) in extra {
        match out.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.clone(),
            None => out.push((key.clone(), value.clone())),
        }
    }
    out
}

/// Appends a named request to a collection file, creating it if needed.
pub fn append_request(path: &str, name: &str, curl: &str) -> Result<(), String> {
    let mut text = fs::read_to_string(path).unwrap_or_default();
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(&format!("### {}\n{}\n", name, curl));
    fs::write(path, text).map_err(|e| format!("cannot write '{}': {}", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# a comment
@base = http://localhost:3000
@token = dev

## Usuarios
### get user
curl {{base}}/users/1 -H \"Authorization: Bearer {{token}}\"

### create user
curl -X POST {{base}}/users -d '{\"name\":\"Juan\"}'
";

    #[test]
    fn parses_variables_groups_and_requests() {
        let col = Collection::parse(SAMPLE);
        assert_eq!(col.variables.len(), 2);
        assert_eq!(
            col.variables[0],
            ("base".to_string(), "http://localhost:3000".to_string())
        );
        assert_eq!(col.requests.len(), 2);
        assert_eq!(col.requests[0].name, "get user");
        assert_eq!(col.requests[0].group.as_deref(), Some("Usuarios"));
        assert_eq!(col.requests[1].name, "create user");
    }

    #[test]
    fn joins_continued_lines() {
        let col = Collection::parse("### up\ncurl -X POST https://x \\\n  -d '{}'\n");
        assert_eq!(col.requests[0].curl, "curl -X POST https://x -d '{}'");
    }

    #[test]
    fn ignores_requests_without_command() {
        let col = Collection::parse("### empty\n\n### real\ncurl https://x\n");
        assert_eq!(col.requests.len(), 1);
        assert_eq!(col.requests[0].name, "real");
    }

    #[test]
    fn substitutes_variables() {
        let vars = vec![("base".to_string(), "http://x".to_string())];
        assert_eq!(substitute("curl {{base}}/a", &vars), "curl http://x/a");
    }

    #[test]
    fn merge_lets_extra_win() {
        let base = vec![("k".to_string(), "old".to_string())];
        let extra = vec![
            ("k".to_string(), "new".to_string()),
            ("n".to_string(), "1".to_string()),
        ];
        let merged = merge_vars(&base, &extra);
        assert_eq!(
            merged,
            vec![
                ("k".to_string(), "new".to_string()),
                ("n".to_string(), "1".to_string())
            ]
        );
        assert_eq!(substitute("{{k}}", &merged), "new");
    }
}
