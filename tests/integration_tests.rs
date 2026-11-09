use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn create_temp_input(response: &str) -> std::process::Child {
    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn cat process");

    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        stdin
            .write_all(response.as_bytes())
            .expect("Failed to write to stdin");
        // Close stdin so cat sends EOF and terminates
        drop(stdin);
    }

    child
}

#[test]
fn test_integration_with_real_json_response() {
    let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"id\": 1, \"name\": \"test\", \"active\": true}";
    let mut cat_child = create_temp_input(response);

    // Test pcurl with the input using compiled binary
    let output = Command::new("./target/debug/pcurl")
        .env("PCURL_TEST_MODE", "1")
        .stdin(cat_child.stdout.take().unwrap())
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain the JSON structure
    assert!(stdout.contains("\"id\":"));
    assert!(stdout.contains("\"name\":"));
    assert!(stdout.contains("\"active\":"));
    assert!(stdout.contains("200 OK"));
}

#[test]
fn test_integration_with_xml_response() {
    let response = "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\n\r\n<?xml version=\"1.0\"?><root><item>test</item></root>";
    let mut cat_child = create_temp_input(response);

    let output = Command::new("cargo")
        .args(&["run", "--bin", "pcurl", "--"])
        .env("PCURL_TEST_MODE", "1")
        .stdin(cat_child.stdout.take().unwrap())
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain XML structure
    assert!(stdout.contains("<?xml"));
    assert!(stdout.contains("<root>"));
    assert!(stdout.contains("<item>"));
}

#[test]
fn test_integration_with_plain_text_response() {
    let response =
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nHello, World!\nThis is plain text.";
    let mut cat_child = create_temp_input(response);

    let output = Command::new("cargo")
        .args(&["run", "--bin", "pcurl", "--"])
        .env("PCURL_TEST_MODE", "1")
        .stdin(cat_child.stdout.take().unwrap())
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain the plain text
    assert!(stdout.contains("Hello, World!"));
    assert!(stdout.contains("This is plain text"));
}

#[test]
fn test_integration_error_response() {
    let response = "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\n\r\n{\"error\": \"Not found\", \"code\": 404}";
    let mut cat_child = create_temp_input(response);

    let output = Command::new("cargo")
        .args(&["run", "--bin", "pcurl", "--"])
        .env("PCURL_TEST_MODE", "1")
        .stdin(cat_child.stdout.take().unwrap())
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain error information
    assert!(stdout.contains("404 Not Found"));
    assert!(stdout.contains("\"error\":"));
    assert!(stdout.contains("\"code\":"));
}

#[test]
fn test_integration_empty_response() {
    let response = "HTTP/1.1 204 No Content\r\n\r\n";
    let mut cat_child = create_temp_input(response);

    let output = Command::new("cargo")
        .args(&["run", "--bin", "pcurl", "--"])
        .env("PCURL_TEST_MODE", "1")
        .stdin(cat_child.stdout.take().unwrap())
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should handle empty body gracefully
    assert!(stdout.contains("204 No Content"));
    assert!(stdout.contains("(empty response body)"));
}

#[test]
fn test_integration_redirect_response() {
    let response = "HTTP/1.1 301 Moved Permanently\r\nLocation: /new-url\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"success\": true, \"redirected\": true}";
    let mut cat_child = create_temp_input(response);

    let output = Command::new("cargo")
        .args(&["run", "--bin", "pcurl", "--"])
        .env("PCURL_TEST_MODE", "1")
        .stdin(cat_child.stdout.take().unwrap())
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should handle redirects and show final response
    assert!(stdout.contains("200 OK"));
    assert!(stdout.contains("\"success\":"));
    assert!(stdout.contains("\"redirected\":"));
}

#[test]
fn test_streaming_headers_appear_before_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let mut buf = [0; 4096];
            let _ = stream.read(&mut buf);
            thread::sleep(Duration::from_millis(10));
            let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 27\r\n\r\n{\"status\": \"ok\", \"key\": 42}";
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });

    thread::sleep(Duration::from_millis(50));

    let output = Command::new("./target/debug/pcurl")
        .arg(format!("curl http://127.0.0.1:{}/test", port))
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let status_pos = stdout.find("200 OK").expect("Should contain 200 OK");
    let body_pos = stdout.find("BODY").expect("Should contain BODY label");
    assert!(
        status_pos < body_pos,
        "Headers (200 OK at {}) must appear before BODY section (at {})",
        status_pos,
        body_pos
    );

    assert!(stdout.contains("\"status\":"));
    assert!(stdout.contains("\"key\":"));
    assert!(stdout.contains("Content-Type"));

    server.join().unwrap();
}

#[test]
fn test_streaming_sse_events_appear_in_realtime() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let mut buf = [0; 4096];
            let _ = stream.read(&mut buf);

            let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n";
            stream.write_all(headers.as_bytes()).unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(50));

            let event1 = "data: {\"event\": \"connected\"}\n\n";
            stream.write_all(event1.as_bytes()).unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(80));

            let event2 = "data: {\"event\": \"message\", \"text\": \"hello\"}\n\n";
            stream.write_all(event2.as_bytes()).unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(80));

            let event3 = "data: {\"event\": \"close\"}\n\n";
            stream.write_all(event3.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });

    thread::sleep(Duration::from_millis(50));

    let output = Command::new("./target/debug/pcurl")
        .arg(format!("curl http://127.0.0.1:{}/sse-test", port))
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should NOT contain "BODY" label (SSE mode shows events inline)
    assert!(
        !stdout.contains("BODY"),
        "SSE mode should not show BODY label"
    );

    // All 3 SSE events should appear with ← prefix
    assert!(
        stdout.contains("← {\"event\": \"connected\"}"),
        "Event 1 missing"
    );
    assert!(
        stdout.contains("← {\"event\": \"message\", \"text\": \"hello\"}"),
        "Event 2 missing"
    );
    assert!(
        stdout.contains("← {\"event\": \"close\"}"),
        "Event 3 missing"
    );

    // Events should appear in order
    let pos1 = stdout.find("connected").unwrap();
    let pos2 = stdout.find("message").unwrap();
    let pos3 = stdout.find("close").unwrap();
    assert!(pos1 < pos2, "Events out of order: event1 before event2");
    assert!(pos2 < pos3, "Events out of order: event2 before event3");

    // Should still have headers (status + content-type)
    assert!(stdout.contains("200 OK"));
    assert!(stdout.contains("text/event-stream"));

    server.join().unwrap();
}

#[test]
fn test_mcp_subcommand() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);

            // Validate JSON-RPC body is present
            assert!(request.contains("jsonrpc"));
            assert!(request.contains("\"tools/call\""));
            assert!(request.contains("\"name\":\"test\""));
            assert!(request.contains("session_id=test-session"));

            thread::sleep(Duration::from_millis(10));
            let headers = "HTTP/1.1 202 Accepted\r\nContent-Type: text/event-stream\r\n\r\n";
            stream.write_all(headers.as_bytes()).unwrap();
            stream.flush().unwrap();

            thread::sleep(Duration::from_millis(30));
            let event1 = "data: {\"type\": \"connected\", \"session\": \"test\"}\n\n";
            stream.write_all(event1.as_bytes()).unwrap();
            stream.flush().unwrap();

            thread::sleep(Duration::from_millis(30));
            let event2 = "data: {\"type\": \"result\", \"data\": \"ok\"}\n\n";
            stream.write_all(event2.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });

    thread::sleep(Duration::from_millis(50));

    let output = Command::new("./target/debug/pcurl")
        .args(&[
            "mcp",
            "--session-id",
            "test-session",
            &format!("http://127.0.0.1:{}/mcp/message", port),
            "tools/call",
            r#"{"name":"test"}"#,
        ])
        .output()
        .expect("Failed to run pcurl");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("← {\"type\": \"connected\", \"session\": \"test\"}"),
        "Event 1 missing"
    );
    assert!(
        stdout.contains("← {\"type\": \"result\", \"data\": \"ok\"}"),
        "Event 2 missing"
    );
    assert!(stdout.contains("202 Accepted"), "Should show 202 status");
    assert!(
        stdout.contains("text/event-stream"),
        "Should show content-type"
    );

    server.join().unwrap();
}

#[test]
fn test_collection_save_list_and_run() {
    let dir = std::env::temp_dir().join(format!("pcurl-collection-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("api.pcurl");

    // save two requests (second goes into a group via a hand-written file)
    let save = Command::new("./target/debug/pcurl")
        .arg("save")
        .arg("get user")
        .arg("curl https://api.example.com/users/1")
        .arg("--file")
        .arg(&file)
        .output()
        .expect("Failed to run pcurl save");
    assert!(save.status.success());

    std::fs::write(
        &file,
        "@base = http://127.0.0.1:1\n\n### health\ncurl {{base}}/health\n",
    )
    .unwrap();

    // list shows the request name and declares the group/section
    let list = Command::new("./target/debug/pcurl")
        .arg("list")
        .arg(&file)
        .output()
        .expect("Failed to run pcurl list");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.contains("health"),
        "list should show the request name"
    );

    // unknown --only is an error with the available names
    let missing = Command::new("./target/debug/pcurl")
        .args(["run"])
        .arg(&file)
        .args(["--only", "nope"])
        .output()
        .expect("Failed to run pcurl run");
    assert!(!missing.status.success(), "unknown --only must fail");
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("no request named 'nope'"),
        "should list available names"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_collection_missing_file_fails() {
    let output = Command::new("./target/debug/pcurl")
        .args(["list", "/nonexistent/definitely-missing.pcurl"])
        .output()
        .expect("Failed to run pcurl list");
    assert!(
        !output.status.success(),
        "missing collection must exit non-zero"
    );
}
