use colored::Colorize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

const SSE_TIMEOUT_S: u64 = 30;
const INIT_TIMEOUT_S: u64 = 10;

#[derive(Debug)]
struct SseEvent {
    event_type: String,
    data: String,
}

pub struct McpClient {
    #[allow(dead_code)]
    pub session_id: String,
    pub message_url: String,
    event_rx: mpsc::Receiver<SseEvent>,
    sse_child: Option<Child>,
    next_id: u64,
    verbose: bool,
    headers: Vec<String>,
}

impl McpClient {
    pub fn connect(message_url: &str, headers: &[String], verbose: bool) -> Result<Self, String> {
        let sse_url = message_url.replace("/mcp/message", "/mcp/sse");

        let mut args = vec!["-s".to_string(), "-N".to_string()];
        for header in headers {
            args.push("-H".to_string());
            args.push(header.clone());
        }
        args.push(sse_url.clone());

        let mut child = Command::new("curl")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn curl for SSE: {}", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "No stdout from SSE curl".to_string())?;

        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut event_type = String::new();
            let mut event_data = String::new();
            let mut line_buf = String::new();

            loop {
                line_buf.clear();
                match reader.read_line(&mut line_buf) {
                    Ok(0) => break,
                    Err(_) => break,
                    Ok(_) => {}
                }

                let trimmed = line_buf.trim_end_matches(&['\r', '\n'][..]);

                if trimmed.is_empty() {
                    if !event_type.is_empty() || !event_data.is_empty() {
                        let _ = tx.send(SseEvent {
                            event_type: std::mem::take(&mut event_type),
                            data: std::mem::take(&mut event_data),
                        });
                    }
                } else if let Some(val) = trimmed.strip_prefix("event: ") {
                    event_type = val.to_string();
                } else if let Some(val) = trimmed.strip_prefix("data: ") {
                    event_data = val.to_string();
                }
            }
        });

        if verbose {
            eprintln!("  {} SSE {}", "↻".cyan().bold(), sse_url.dimmed());
        }

        let session_id = loop {
            match rx.recv_timeout(Duration::from_secs(INIT_TIMEOUT_S)) {
                Ok(event) => {
                    if verbose {
                        eprintln!(
                            "  {} event: {}  data: {}",
                            "←".green(),
                            event.event_type.dimmed(),
                            event.data.dimmed()
                        );
                    }
                    if event.event_type == "endpoint" {
                        if let Some(pos) = event.data.find("session_id=") {
                            let sid = event.data[pos + 11..]
                                .split(|c: char| c.is_whitespace() || c == '&' || c == '\n')
                                .next()
                                .unwrap_or("")
                                .to_string();
                            if !sid.is_empty() {
                                break sid;
                            }
                        }
                        let trimmed = event.data.trim();
                        if !trimmed.is_empty() && trimmed.len() > 10 {
                            break trimmed.to_string();
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    return Err("Timed out waiting for session_id from SSE".to_string());
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = child.kill();
                    return Err("SSE connection closed unexpectedly".to_string());
                }
            }
        };

        let full_url = format!(
            "{}?session_id={}",
            message_url.trim_end_matches('/'),
            session_id
        );

        if verbose {
            eprintln!("  {} Session: {}", "✓".green(), session_id.dimmed());
        }

        Ok(McpClient {
            session_id,
            message_url: full_url,
            event_rx: rx,
            sse_child: Some(child),
            next_id: 1,
            verbose,
            headers: headers.to_vec(),
        })
    }

    pub fn send_request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let body_str = body.to_string();

        if self.verbose {
            eprintln!("  {} POST {}", "→".cyan().bold(), self.message_url.dimmed());
            eprintln!("  {} {}", "→".cyan(), body_str.yellow());
        }

        let mut request = ureq::post(&self.message_url).set("Content-Type", "application/json");
        for header in &self.headers {
            if let Some((key, value)) = header.split_once(':') {
                request = request.set(key.trim(), value.trim());
            }
        }
        let response = request
            .send_string(&body_str)
            .map_err(|e| format!("POST failed: {}", e))?;

        let status = response.status();
        if status != 200 && status != 202 {
            return Err(format!("Server returned HTTP {}", status));
        }

        let timeout = Duration::from_secs(SSE_TIMEOUT_S);
        loop {
            match self.event_rx.recv_timeout(timeout) {
                Ok(event) => {
                    if self.verbose {
                        eprintln!(
                            "  {} event: {}  data: {}",
                            "←".green(),
                            event.event_type.dimmed(),
                            event.data.dimmed()
                        );
                    }

                    if event.event_type == "message" || event.event_type.is_empty() {
                        if let Ok(json) = serde_json::from_str::<Value>(&event.data) {
                            if json.get("id").and_then(|v| v.as_u64()) == Some(id) {
                                return Ok(json);
                            }
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "Timed out waiting for response (method: {})",
                        method
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("SSE connection closed by server".to_string());
                }
            }
        }
    }

    pub fn close(&mut self) {
        if let Some(mut child) = self.sse_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.close();
    }
}
