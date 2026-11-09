use colored::*;
use serde_json::Value;
use std::io::{self, BufRead, BufReader, IsTerminal, Read};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Instant;

mod cli;
mod collection;
mod curl_parser;
mod display;
mod help;
mod mcp;
mod mcp_client;
mod version;
mod ws_client;

use clap::Parser;
use cli::{Cli, Commands, OutputMode};
use collection::{Collection, Request};
use curl_parser::CurlCommand;
use display::display_response;
use help::print_doctor;
use mcp_client::McpClient;
use version::{check_for_update_notification, update_pcurl};

fn main() {
    let cli = Cli::parse();

    // Special flags (doctor / update) take priority
    if cli.doctor {
        print_doctor();
        return;
    }
    if cli.update {
        update_pcurl();
        return;
    }

    // Check NO_COLOR env var (clap doesn't auto-handle this in derive API)
    let no_color = cli.no_color || std::env::var("NO_COLOR").is_ok();

    let output_mode = OutputMode {
        body_only: cli.body_only,
        headers_only: cli.headers_only,
        no_color,
    };

    if no_color {
        colored::control::set_override(false);
    }

    // Dispatch by subcommand
    match cli.command {
        // pcurl run api.pcurl [--only "get user"] [--env staging.env] [--var k=v]
        Some(Commands::Run {
            file,
            only,
            env,
            vars,
        }) => {
            run_collection(&file, only.as_deref(), env.as_deref(), &vars, &output_mode);
        }

        // pcurl list api.pcurl
        Some(Commands::List { file }) => {
            list_collection(&file);
        }

        // pcurl save "get user" 'curl https://api.example.com/users/1'
        Some(Commands::Save { name, curl, file }) => {
            save_request(&name, &curl, &file);
        }

        // pcurl ws wss://echo.websocket.org
        Some(Commands::Ws { url, verbose }) => {
            run_websocket(&url, verbose, &output_mode);
        }

        // pcurl wscat -c wss://echo.websocket.org
        Some(Commands::Wscat { connect, verbose }) => {
            run_websocket(&connect, verbose, &output_mode);
        }

        // pcurl mcp http://localhost:8080/mcp/message tools/call '{"name":"test"}'
        Some(Commands::Mcp {
            url,
            method,
            params,
            session_id,
            verbose,
        }) => {
            run_mcp_mode(&url, &method, &params, session_id, verbose, &output_mode);
        }

        // No subcommand → HTTP mode
        None => {
            let stdin_has_data = !io::stdin().is_terminal();

            match cli.curl_command {
                // Argument mode: pcurl 'curl https://...'
                Some(cmd) => {
                    // Detect if it's a direct URL (doesn't start with "curl")
                    let curl_cmd = if cmd.trim_start().starts_with("curl") {
                        cmd
                    } else {
                        format!("curl -si {}", cmd)
                    };
                    run_http_argument_mode(&curl_cmd, &output_mode);
                }

                // Pipe mode: curl -si ... | pcurl
                None => {
                    if stdin_has_data {
                        run_pipe_mode(&output_mode);
                    } else {
                        // stdin is a terminal → no pipe → show help
                        eprintln!(
                            "pcurl: no input received. Use `pcurl --help` to see usage modes."
                        );
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    // ── Check for updates (silent notification) ───────────────────────
    // This now happens only if no other command/flag took precedence
    check_for_update_notification();
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution
// ─────────────────────────────────────────────────────────────────────────────

// Validate that the URL only uses safe or explicitly allowed protocols
fn is_safe_url(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ws://")
        || url.starts_with("wss://")
        || url.starts_with("ftp://")
        || url.starts_with("file://")
}

fn run_http_argument_mode(command_str: &str, mode: &OutputMode) -> u16 {
    let parsed = CurlCommand::parse(command_str);

    if !parsed.url.is_empty() && !is_safe_url(&parsed.url) {
        eprintln!(
            "{} {}: URL protocol not allowed: {}",
            "❌".red().bold(),
            "Error".red().bold(),
            &parsed.url.split(':').next().unwrap_or("unknown")
        );
        eprintln!(
            "{} Allowed protocols: http, https, ws, wss, ftp, file",
            "➡️".dimmed()
        );
        std::process::exit(1);
    }

    let method_label = parsed
        .method
        .as_deref()
        .unwrap_or(if parsed.data.is_some() { "POST" } else { "GET" });
    let curl_args = parsed.to_args_with_headers();

    execute_curl_and_stream(&curl_args, &parsed.url, method_label, mode)
}

// ─────────────────────────────────────────────────────────────────────────────
// Collections
// ─────────────────────────────────────────────────────────────────────────────

fn fail(message: &str) -> ! {
    eprintln!("{} {}", "✗".red().bold(), message);
    std::process::exit(1);
}

fn parse_cli_vars(vars: &[String]) -> Result<Vec<(String, String)>, String> {
    vars.iter()
        .map(|v| {
            v.split_once('=')
                .map(|(k, val)| (k.trim().to_string(), val.trim().to_string()))
                .ok_or_else(|| format!("--var expects KEY=VALUE, got '{}'", v))
        })
        .collect()
}

fn merged_variables(
    col: &Collection,
    env: Option<&str>,
    vars: &[String],
) -> Result<Vec<(String, String)>, String> {
    let mut merged = col.variables.clone();
    if let Some(env_path) = env {
        merged = collection::merge_vars(&merged, &Collection::load_vars(env_path)?);
    }
    Ok(collection::merge_vars(&merged, &parse_cli_vars(vars)?))
}

fn request_targets<'a>(
    col: &'a Collection,
    only: Option<&str>,
) -> Result<Vec<&'a Request>, String> {
    match only {
        None => Ok(col.requests.iter().collect()),
        Some(name) => match col.get(name) {
            Some(r) => Ok(vec![r]),
            None => {
                let names: Vec<&str> = col.requests.iter().map(|r| r.name.as_str()).collect();
                let available = if names.is_empty() {
                    "(none)".to_string()
                } else {
                    names.join(", ")
                };
                Err(format!(
                    "no request named '{}'. Available: {}",
                    name, available
                ))
            }
        },
    }
}

fn run_collection(
    file: &str,
    only: Option<&str>,
    env: Option<&str>,
    vars: &[String],
    mode: &OutputMode,
) {
    let col = match Collection::load(file) {
        Ok(c) => c,
        Err(e) => fail(&e),
    };

    let merged = match merged_variables(&col, env, vars) {
        Ok(v) => v,
        Err(e) => fail(&e),
    };

    let targets = match request_targets(&col, only) {
        Ok(t) => t,
        Err(e) => fail(&e),
    };

    if targets.is_empty() {
        fail(&format!("no requests in '{}'", file));
    }

    let total = targets.len();
    let mut ok = 0usize;
    let mut errors = 0usize;

    for (index, req) in targets.iter().enumerate() {
        let label = match &req.group {
            Some(g) => format!("{} / {}", g, req.name),
            None => req.name.clone(),
        };
        display::display_request_header(index + 1, total, &label);

        let curl = collection::substitute(&req.curl, &merged);
        let cmd = if curl.trim_start().starts_with("curl") {
            curl
        } else {
            format!("curl -si {}", curl)
        };

        if (200..300).contains(&run_http_argument_mode(&cmd, mode)) {
            ok += 1;
        } else {
            errors += 1;
        }
    }

    if total > 1 {
        println!();
        let err_label = if errors == 1 { "error" } else { "errors" };
        println!(
            "{}",
            format!("{} requests · {} OK · {} {}", total, ok, errors, err_label).dimmed()
        );
    }
}

fn list_collection(file: &str) {
    let col = match Collection::load(file) {
        Ok(c) => c,
        Err(e) => fail(&e),
    };

    println!();
    println!("{} {}", "collection".dimmed(), file.white().bold());

    if col.requests.is_empty() {
        println!(
            "  {}",
            "(no requests yet — add one with `pcurl save`)"
                .dimmed()
                .italic()
        );
        return;
    }

    let mut current_group: Option<&str> = None;
    for req in &col.requests {
        if req.group.as_deref() != current_group {
            current_group = req.group.as_deref();
            if let Some(g) = &req.group {
                println!("\n{}", g.cyan().bold());
            }
        }
        println!("  {}", req.name);
    }

    println!();
    println!(
        "{} {}",
        "run one:".dimmed(),
        format!("pcurl run {} --only \"<name>\"", file).cyan()
    );
}

fn save_request(name: &str, curl: &str, file: &str) {
    match collection::append_request(file, name, curl) {
        Ok(()) => println!("{} saved '{}' to {}", "✓".green().bold(), name, file),
        Err(e) => fail(&e),
    }
}

fn run_mcp_mode(
    url: &str,
    method: &str,
    params_str: &str,
    session_id: Option<String>,
    verbose: bool,
    mode: &OutputMode,
) {
    let params_value = match serde_json::from_str::<Value>(params_str) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} Invalid JSON in params: {}", "✗".red().bold(), e);
            std::process::exit(1);
        }
    };

    if let Some(sid) = session_id {
        run_mcp_direct(url, &sid, method, &params_value, verbose, mode);
    } else {
        run_mcp_sse(url, method, &params_value, verbose, mode);
    }
}

fn run_mcp_sse(url: &str, method: &str, params: &Value, verbose: bool, mode: &OutputMode) {
    let mut client = match McpClient::connect(url, verbose) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} MCP connection failed: {}", "✗".red().bold(), e);
            std::process::exit(1);
        }
    };

    println!();
    println!(
        "{} {} {}",
        "MCP".cyan().bold(),
        "→".dimmed(),
        url.white().bold()
    );
    println!("{}", "─".repeat(64).dimmed());

    let init_result = client.send_request(
        "initialize",
        &serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "pcurl", "version": env!("CARGO_PKG_VERSION")}
        }),
    );

    match init_result {
        Ok(json) => {
            if verbose {
                let server_name = json
                    .pointer("/result/serverInfo/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let server_version = json
                    .pointer("/result/serverInfo/version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                eprintln!(
                    "  {} Initialized: {} {}",
                    "✓".green(),
                    server_name.cyan(),
                    server_version.dimmed()
                );
            }
        }
        Err(e) => {
            eprintln!("  {} Initialize failed: {}", "✗".red().bold(), e);
            client.close();
            std::process::exit(1);
        }
    }

    let start = Instant::now();
    let result = client.send_request(method, params);
    let elapsed = start.elapsed().as_millis();

    match result {
        Ok(json) => display_mcp_result(&json, elapsed, mode),
        Err(e) => {
            eprintln!("  {} {}", "✗".red().bold(), e);
        }
    }

    client.close();
}

fn run_mcp_direct(
    url: &str,
    sid: &str,
    method: &str,
    params: &Value,
    verbose: bool,
    mode: &OutputMode,
) {
    let full_url = format!("{}?session_id={}", url.trim_end_matches('/'), sid);
    let body = mcp::build_json_rpc_body_value(method, params);

    if verbose {
        eprintln!("{} Session: {}", "ℹ".cyan(), sid.dimmed());
        eprintln!("{} POST {}", "ℹ".cyan(), full_url.dimmed());
        eprintln!("{} {}", "ℹ".cyan(), body.yellow());
    }

    let args: Vec<String> = vec![
        "-s".to_string(),
        "-N".to_string(),
        "-i".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        "-H".to_string(),
        "Content-Type: application/json".to_string(),
        "-d".to_string(),
        body,
        full_url,
    ];

    execute_curl_and_stream(&args, url, "MCP", mode);
}

/// MCP wraps tool results as `content: [{ type: "text", text: "<json string>" }]`.
/// If that `text` parses as JSON, replace it with the parsed value so the
/// pretty-printer shows a real object instead of one escaped green line.
fn unwrap_mcp_content(result: &Value) -> Value {
    let mut out = result.clone();
    if let Some(items) = out.get_mut("content").and_then(Value::as_array_mut) {
        for item in items.iter_mut() {
            if item.get("type").and_then(Value::as_str) != Some("text") {
                continue;
            }
            let parsed = item
                .get("text")
                .and_then(Value::as_str)
                .and_then(|t| serde_json::from_str::<Value>(t).ok());
            if let Some(parsed) = parsed {
                item["text"] = parsed;
            }
        }
    }
    out
}

fn display_mcp_result(json: &Value, elapsed_ms: u128, mode: &OutputMode) {
    if let Some(result) = json.get("result") {
        println!(
            "  {}  {}  {}",
            "✓".green().bold(),
            "[OK]".white().bold(),
            format!("{} ms", elapsed_ms).dimmed()
        );
        if !mode.headers_only {
            println!(
                "{}",
                "  ──────────────────────────────────────────────────────".dimmed()
            );
            let value = unwrap_mcp_content(result);
            display::display_json_value(&value, mode);
        }
    } else if let Some(error) = json.get("error") {
        println!("  {}  {}", "✗".red().bold(), "[ERROR]".red().bold());
        println!(
            "{}",
            "  ──────────────────────────────────────────────────────".dimmed()
        );
        let formatted = serde_json::to_string_pretty(error).unwrap_or_default();
        for line in formatted.lines() {
            println!("  {}", line.red());
        }
        println!();
    } else if !mode.headers_only {
        println!(
            "  {}",
            serde_json::to_string_pretty(json)
                .unwrap_or_default()
                .white()
        );
    }
}

fn execute_curl_and_stream(
    args: &[String],
    url_display: &str,
    method_display: &str,
    mode: &OutputMode,
) -> u16 {
    println!();
    println!(
        "{} {} {}",
        method_display.white().bold(),
        "→".dimmed(),
        url_display
    );
    println!("{}", "─".repeat(64).dimmed());

    let start = Instant::now();

    let mut child = match Command::new("curl")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            eprintln!("{} curl not found or error: {}", "✗".red().bold(), e);
            std::process::exit(1);
        }
        Ok(c) => c,
    };

    let stdout = child.stdout.take().expect("failed to capture stdout");
    let stderr = child.stderr.take().expect("failed to capture stderr");

    let stderr_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        reader
            .lines()
            .map_while(Result::ok)
            .filter(|line| {
                let t = line.trim();
                !t.is_empty()
                    && !t.contains("% Total")
                    && !t.contains("Dload")
                    && !t.starts_with(' ')
            })
            .collect::<Vec<_>>()
    });

    let mut reader = BufReader::new(stdout);
    let mut buf = String::new();

    let mut body = String::new();
    let mut headers_displayed = false;

    let mut saved_redirect_headers = String::new();
    let mut saved_redirect_body = String::new();
    let mut has_saved_redirect = false;

    let mut block = String::new();
    let mut status: u16 = 0;
    let mut is_sse = false;
    let mut final_status: u16 = 0;

    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) => break,
            Err(_) => break,
            Ok(_) => {}
        }

        let line = buf.trim_end_matches(&['\r', '\n'][..]);

        if headers_displayed {
            if is_sse {
                if !mode.headers_only {
                    display::display_sse_line(&buf, mode);
                }
            } else {
                body.push_str(&buf);
            }
            continue;
        }

        if line.is_empty() {
            if status > 0 {
                if !(300..400).contains(&status) {
                    display::display_status_and_headers(&block, start.elapsed().as_millis(), mode);
                    is_sse = display::is_sse_content_type(&block);
                    headers_displayed = true;
                    final_status = status;
                    has_saved_redirect = false;
                    saved_redirect_headers.clear();
                    saved_redirect_body.clear();
                } else {
                    has_saved_redirect = true;
                    saved_redirect_headers = block.clone();
                    saved_redirect_body.clear();
                }
            }
            block.clear();
            status = 0;
        } else if line.starts_with("HTTP/") {
            if has_saved_redirect {
                has_saved_redirect = false;
                saved_redirect_headers.clear();
                saved_redirect_body.clear();
            }
            status = display::parse_status_code(line);
            block = line.to_string();
        } else if status > 0 {
            block.push('\n');
            block.push_str(line);
        } else if has_saved_redirect {
            saved_redirect_body.push_str(&buf);
        }
    }

    let elapsed = start.elapsed().as_millis();
    let _ = child.wait();

    if let Ok(errors) = stderr_handle.join() {
        if !errors.is_empty() {
            eprintln!("{}", errors.join("\n").yellow());
        }
    }

    if headers_displayed && is_sse {
        if !mode.body_only {
            println!();
        }
    } else if headers_displayed {
        let body_trimmed = body.trim();
        if !mode.headers_only {
            if body_trimmed.is_empty() {
                println!("  {}", "(empty response body)".dimmed().italic());
            } else {
                display::display_body_section(body_trimmed, mode);
            }
        } else if !mode.body_only {
            println!();
        }
    } else if has_saved_redirect {
        display::display_status_and_headers(&saved_redirect_headers, elapsed, mode);
        let redirect_body = saved_redirect_body.trim();
        if !mode.headers_only {
            if redirect_body.is_empty() {
                println!("  {}", "(empty response body)".dimmed().italic());
            } else {
                display::display_body_section(redirect_body, mode);
            }
        } else if !mode.body_only {
            println!();
        }
    } else {
        let output = body.trim();
        if output.is_empty() {
            eprintln!("{} No response received. Check the URL.", "✗".red().bold());
        } else {
            display::display_body_section(output, mode);
        }
    }

    final_status
}

fn run_pipe_mode(mode: &OutputMode) {
    const MAX_INPUT_SIZE: usize = 50 * 1024 * 1024; // 50 MB

    let mut input = String::new();
    let mut reader = io::stdin().take((MAX_INPUT_SIZE + 1) as u64);
    let bytes_read = reader
        .read_to_string(&mut input)
        .expect("Error reading stdin");

    if bytes_read > MAX_INPUT_SIZE {
        eprintln!("  {} Input exceeds 50MB limit, truncating", "⚠️".yellow());
        input.truncate(MAX_INPUT_SIZE);
    }

    println!();
    display_response(&input, 0, mode);
}

async fn run_websocket_async(url: &str, verbose: bool, mode: &OutputMode) {
    ws_client::connect_ws(url, verbose, mode).await;
}

fn run_websocket(url: &str, verbose: bool, mode: &OutputMode) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_websocket_async(url, verbose, mode));
}

#[cfg(test)]
mod tests {
    use crate::display::parse_status_code;

    #[test]
    fn test_parse_status_code() {
        assert_eq!(parse_status_code("HTTP/1.1 200 OK"), 200);
        assert_eq!(parse_status_code("HTTP/2 404 Not Found"), 404);
        assert_eq!(parse_status_code("HTTP/1.1 500 Internal Server Error"), 500);
        assert_eq!(parse_status_code(""), 0);
        assert_eq!(parse_status_code("invalid"), 0);
    }

    #[test]
    fn test_display_response_json() {
        let json_response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"id\": 1, \"name\": \"test\"}";
        crate::display::display_response(
            json_response,
            123,
            &crate::cli::OutputMode {
                body_only: false,
                headers_only: false,
                no_color: false,
            },
        );
    }

    #[test]
    fn test_display_response_xml() {
        let xml_response = "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\n\r\n<?xml version=\"1.0\"?><root><item>test</item></root>";
        crate::display::display_response(
            xml_response,
            456,
            &crate::cli::OutputMode {
                body_only: false,
                headers_only: false,
                no_color: false,
            },
        );
    }

    #[test]
    fn test_display_response_plain_text() {
        let text_response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nHello, World!";
        crate::display::display_response(
            text_response,
            789,
            &crate::cli::OutputMode {
                body_only: false,
                headers_only: false,
                no_color: false,
            },
        );
    }

    #[test]
    fn test_display_response_no_headers() {
        let body_only = "{\"message\": \"no headers\"}";
        crate::display::display_response(
            body_only,
            0,
            &crate::cli::OutputMode {
                body_only: false,
                headers_only: false,
                no_color: false,
            },
        );
    }

    #[test]
    fn test_display_response_empty_body() {
        let empty_response = "HTTP/1.1 204 No Content\r\n\r\n";
        crate::display::display_response(
            empty_response,
            100,
            &crate::cli::OutputMode {
                body_only: false,
                headers_only: false,
                no_color: false,
            },
        );
    }

    #[test]
    fn test_display_response_with_redirects() {
        let redirect_response = "HTTP/1.1 301 Moved Permanently\r\nLocation: /new-url\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"success\": true}";
        crate::display::display_response(
            redirect_response,
            250,
            &crate::cli::OutputMode {
                body_only: false,
                headers_only: false,
                no_color: false,
            },
        );
    }

    #[test]
    fn test_unwrap_mcp_content_extracts_json_string() {
        let result = serde_json::json!({
            "content": [{ "type": "text", "text": "{\"total_ventas\":4}" }]
        });
        let out = crate::unwrap_mcp_content(&result);
        assert_eq!(out["content"][0]["text"]["total_ventas"], 4);
    }

    #[test]
    fn test_unwrap_mcp_content_keeps_plain_text() {
        let result = serde_json::json!({
            "content": [{ "type": "text", "text": "no soy json" }]
        });
        let out = crate::unwrap_mcp_content(&result);
        assert_eq!(out["content"][0]["text"], "no soy json");
    }
}
