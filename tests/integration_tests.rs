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

#[test]
fn test_set_and_unset_variables_from_the_cli() {
    let dir = std::env::temp_dir().join(format!("pcurl-vars-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".pcurl/secrets")).unwrap();
    let collection = dir.join(".pcurl/collection.pcurl");
    std::fs::write(&collection, "### ping\ncurl {{base}}/ping\n").unwrap();
    let secrets = dir.join(".pcurl/secrets/secrets.env");
    std::fs::write(&secrets, "@token =\n").unwrap();

    let bin = std::env::current_dir().unwrap().join("target/debug/pcurl");
    let pcurl = |args: &[&str]| {
        Command::new(&bin)
            .args(args)
            .current_dir(&dir)
            .output()
            .expect("failed to run pcurl")
    };

    // Setting the same key twice updates in place instead of duplicating.
    assert!(pcurl(&["set", "base", "http://127.0.0.1:1"])
        .status
        .success());
    assert!(pcurl(&["set", "base", "http://127.0.0.1:2"])
        .status
        .success());
    let text = std::fs::read_to_string(&collection).unwrap();
    assert_eq!(text.matches("@base").count(), 1, "duplicated variable line");
    assert!(text.contains("@base = http://127.0.0.1:2"), "got: {text}");
    assert!(!text.contains("127.0.0.1:1"), "old value survived: {text}");

    // Secrets land in their own file, and `list` never prints their value.
    assert!(pcurl(&["set", "--secret", "token", "s3cr3t"])
        .status
        .success());
    assert!(std::fs::read_to_string(&secrets)
        .unwrap()
        .contains("@token = s3cr3t"));
    let listed = pcurl(&["list"]);
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(
        stdout.contains("@base = http://127.0.0.1:2"),
        "got: {stdout}"
    );
    assert!(!stdout.contains("s3cr3t"), "list leaked a secret: {stdout}");

    // The CLI-written value is what `run` substitutes into the URL.
    let run = pcurl(&["run", "--only", "ping"]);
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("http://127.0.0.1:2/ping"),
        "variable not substituted: {stdout}"
    );

    // Unset removes the line entirely.
    assert!(pcurl(&["unset", "base"]).status.success());
    assert!(!std::fs::read_to_string(&collection)
        .unwrap()
        .contains("@base"));
    assert!(!pcurl(&["unset", "base"]).status.success(), "should fail");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_call_runs_one_request_and_list_shows_the_method() {
    let dir = std::env::temp_dir().join(format!("pcurl-call-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("api.pcurl");
    std::fs::write(
        &file,
        "@base = http://127.0.0.1:1\n\n### get ping\ncurl {{base}}/ping\n\n\
         ### post thing\ncurl -X POST {{base}}/thing -d '{}'\n",
    )
    .unwrap();

    let list = Command::new("./target/debug/pcurl")
        .arg("list")
        .arg(&file)
        .output()
        .expect("failed to run pcurl list");
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("GET"), "method missing from list: {stdout}");
    assert!(
        stdout.contains("POST"),
        "method missing from list: {stdout}"
    );

    let call = Command::new("./target/debug/pcurl")
        .arg("call")
        .arg("get ping")
        .arg("--file")
        .arg(&file)
        .output()
        .expect("failed to run pcurl call");
    let stdout = String::from_utf8_lossy(&call.stdout);
    assert!(stdout.contains("http://127.0.0.1:1/ping"), "got: {stdout}");
    assert!(!stdout.contains("/thing"), "ran more than one: {stdout}");

    let missing = Command::new("./target/debug/pcurl")
        .arg("call")
        .arg("nope")
        .arg("--file")
        .arg(&file)
        .output()
        .expect("failed to run pcurl call");
    assert!(!missing.status.success(), "unknown name must fail");
    let err = String::from_utf8_lossy(&missing.stderr);
    assert!(err.contains("Available:"), "got: {err}");

    // Saving the same name updates it in place instead of duplicating.
    let saved = Command::new("./target/debug/pcurl")
        .args(["save", "get ping", "curl {{base}}/ping2", "--file"])
        .arg(&file)
        .output()
        .expect("failed to run pcurl save");
    assert!(saved.status.success());
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        text.matches("### get ping").count(),
        1,
        "duplicated: {text}"
    );
    assert!(text.contains("/ping2"), "got: {text}");

    // rm removes the block; a second rm fails.
    let rm = Command::new("./target/debug/pcurl")
        .args(["rm", "get ping", "--file"])
        .arg(&file)
        .output()
        .expect("failed to run pcurl rm");
    assert!(rm.status.success());
    assert!(!std::fs::read_to_string(&file)
        .unwrap()
        .contains("### get ping"));
    let again = Command::new("./target/debug/pcurl")
        .args(["rm", "get ping", "--file"])
        .arg(&file)
        .output()
        .expect("failed to run pcurl rm");
    assert!(!again.status.success(), "removing twice must fail");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_save_reads_the_curl_from_stdin() {
    let dir = std::env::temp_dir().join(format!("pcurl-save-stdin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("api.pcurl");

    let bin = std::env::current_dir().unwrap().join("target/debug/pcurl");
    let mut child = Command::new(&bin)
        .args(["save", "/auth/register", "-", "--file"])
        .arg(&file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn pcurl save");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            b"curl -X POST {{base}}/auth/register \\\n  -d '{\n    \"email\": \"{{email}}\"\n  }'\n",
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("### /auth/register"), "got: {text}");
    assert!(text.contains("-d '{\n"), "multi-line body lost: {text}");
    assert!(!text.ends_with("\n\n"), "trailing blank line: {text:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_argument_mode_reads_collection_variables() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 2048];
        let n = stream.read(&mut buf).unwrap();
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .unwrap();
        head
    });

    let dir = std::env::temp_dir().join(format!("pcurl-argmode-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".pcurl/secrets")).unwrap();
    std::fs::write(
        dir.join(".pcurl/collection.pcurl"),
        format!("@base = http://127.0.0.1:{}\n", port),
    )
    .unwrap();
    std::fs::write(dir.join(".pcurl/secrets/secrets.env"), "@token = tok-abc\n").unwrap();

    let bin = std::env::current_dir().unwrap().join("target/debug/pcurl");
    let output = Command::new(&bin)
        .arg("curl {{base}}/ping -H \"Authorization: Bearer {{token}}\"")
        .current_dir(&dir)
        .output()
        .expect("failed to run pcurl");

    let head = server.join().unwrap();
    assert!(head.starts_with("GET /ping"), "got: {head}");
    assert!(
        head.contains("Authorization: Bearer tok-abc"),
        "got: {head}"
    );
    assert!(output.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_save_group_places_the_request_in_that_group() {
    let dir = std::env::temp_dir().join(format!("pcurl-group-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("api.pcurl");
    std::fs::write(&file, "@base = http://127.0.0.1:1\n").unwrap();

    let save = Command::new("./target/debug/pcurl")
        .args([
            "save",
            "--group",
            "Auth",
            "login",
            "curl {{base}}/auth/login",
            "--file",
        ])
        .arg(&file)
        .output()
        .expect("failed to run pcurl save");
    assert!(
        save.status.success(),
        "got: {}",
        String::from_utf8_lossy(&save.stderr)
    );

    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("## Auth\n### login"), "got: {text}");

    let list = Command::new("./target/debug/pcurl")
        .arg("list")
        .arg(&file)
        .output()
        .expect("failed to run pcurl list");
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("Auth"), "group missing from list: {stdout}");
    assert!(
        stdout.contains("Auth/login"),
        "list must show the qualified name: {stdout}"
    );

    // The qualified name from `list` works with `call`.
    let call = Command::new("./target/debug/pcurl")
        .args(["call", "Auth/login", "--file"])
        .arg(&file)
        .output()
        .expect("failed to run pcurl call");
    let stdout = String::from_utf8_lossy(&call.stdout);
    assert!(
        stdout.contains("http://127.0.0.1:1/auth/login"),
        "qualified name did not resolve: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_pipe_mode_accepts_non_utf8_input() {
    let mut child = Command::new("./target/debug/pcurl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn pcurl");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n\x89PNG\r\n\x1a\n\xff\xfe")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "exited with a panic: {stderr}");
    assert!(!stderr.contains("panicked"), "panicked: {stderr}");
}

#[test]
fn test_quoted_variable_cannot_inject_curl_flags() {
    let dir = std::env::temp_dir().join(format!("pcurl-inject-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".pcurl/secrets")).unwrap();
    let target = dir.join("pwned");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    std::fs::write(
        dir.join(".pcurl/collection.pcurl"),
        format!("@base = http://127.0.0.1:{}\n", port),
    )
    .unwrap();
    // A value that would close the quote and add `-o <file>` if it were pasted
    // into the command before tokenizing.
    std::fs::write(
        dir.join(".pcurl/secrets/secrets.env"),
        format!("@pw = x' -o {} '\n", target.display()),
    )
    .unwrap();

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(pos) = text.find("\r\n\r\n") {
                let len = text[..pos]
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if buf.len() >= pos + 4 + len {
                    break;
                }
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .unwrap();
        String::from_utf8_lossy(&buf).to_string()
    });

    let bin = std::env::current_dir().unwrap().join("target/debug/pcurl");
    let output = Command::new(&bin)
        .arg("{{base}}/ping -X POST -d '{{pw}}'")
        .current_dir(&dir)
        .output()
        .expect("failed to run pcurl");

    let request = server.join().unwrap();
    assert!(
        !target.exists(),
        "the value injected `-o` and wrote {:?}",
        target
    );
    // The whole value must have stayed as the body, `-o` included.
    let body = request.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    assert!(
        body.contains(&format!("-o {} '", target.display())),
        "value did not reach the body intact: {body:?}"
    );
    assert!(output.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

/// Reads one full HTTP request: headers plus the body promised by
/// `Content-Length`.
fn read_full_request(stream: &mut std::net::TcpStream) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(pos) = text.find("\r\n\r\n") {
            let len = text[..pos]
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if buf.len() >= pos + 4 + len {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

#[test]
fn test_broken_pipe_does_not_panic() {
    let mut child = Command::new("./target/debug/pcurl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn pcurl");

    // Close the reader before writing, so the first flush hits EPIPE.
    drop(child.stdout.take());
    let body = "x".repeat(1024 * 1024);
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("HTTP/1.1 200 OK\r\n\r\n{body}").as_bytes());
    let output = child.wait_with_output().unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "panicked on a closed pipe: {stderr}"
    );
}

#[test]
fn test_mcp_sse_cycle() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let mut sse: Option<std::net::TcpStream> = None;
        let mut posts = 0;

        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            let request = read_full_request(&mut stream);

            if request.starts_with("GET /mcp/sse") {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n")
                    .unwrap();
                stream
                    .write_all(b"event: endpoint\ndata: /mcp/message?session_id=abc\n\n")
                    .unwrap();
                stream.flush().unwrap();
                sse = Some(stream);
                continue;
            }

            let (id, result) = if request.contains("initialize") {
                (
                    1,
                    r#"{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"fake","version":"9.9"}}"#,
                )
            } else {
                (2, r#"{"content":[{"type":"text","text":"{\"ok\":true}"}]}"#)
            };

            stream
                .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            stream.flush().unwrap();

            if let Some(sse_stream) = sse.as_mut() {
                let frame = format!(
                    "event: message\ndata: {{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}\n\n",
                    id, result
                );
                let _ = sse_stream.write_all(frame.as_bytes());
                let _ = sse_stream.flush();
            }

            posts += 1;
            if posts == 2 {
                break;
            }
        }
    });

    let output = Command::new("./target/debug/pcurl")
        .args([
            "mcp",
            &format!("http://127.0.0.1:{}/mcp/message", port),
            "tools/call",
            r#"{"name":"anything"}"#,
        ])
        .output()
        .expect("failed to run pcurl mcp");

    server.join().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[OK]"), "no [OK] marker: {stdout}");
    assert!(
        stdout.contains("\"ok\": true") || stdout.contains("\"ok\":true"),
        "tool content was not unwrapped: {stdout}"
    );
}
