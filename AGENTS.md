# AGENTS.md

## Build & Run

- `cargo build --release` - builds binary to `target/release/pcurl`
- `cargo run --bin pcurl -- [args]` - run with curl arguments (recommended)
- `curl -si <url> | cargo run --bin pcurl --` - pipe mode

## Toolchain and CI

- Rust version is pinned in `rust-toolchain.toml` (currently `1.99.0`). CI uses the same version through the `dtolnay/rust-toolchain@<version>` ref in `.github/workflows/quality_checks.yml`.
- When bumping Rust, change **both** files in the same commit; otherwise a new clippy lint can pass locally and fail in CI.
- `Cargo.lock` is committed on purpose. pcurl is a binary, so CI must build with the exact dependency versions tested locally.
- Pre-commit and pre-push run `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`.
- Before a release, also run `cargo build --release` (CI does).

## CLI Options

- `--help` / `-h` - Show help
- `--version` / `-V` - Show version (also checks for updates)
- `--doctor` / `--check` - Diagnose installation and PATH
- `--update` - Update to latest version via install.sh
- `--body-only` - Show only response body
- `--headers-only` - Show only headers
- `--no-color` - Disable colors (also via `NO_COLOR` env)

## Subcommands

- `pcurl init` - Create `.pcurl/` (collection + secrets folder) in the current project
- `pcurl run [file] [--only <name>] [--env <file>] [--var k=v]` - Run a collection
- `pcurl list [file]` - List requests in a collection
- `pcurl save <name> "<curl…>" [--file <path>]` - Append a request to a collection
- `pcurl ws <url>` / `pcurl wscat -c <url>` - WebSocket
- `pcurl mcp <url> <method> <params> [--header "K: V"]` - MCP JSON-RPC call

## Collections

Plain-text request collections (`src/collection.rs`). Layout created by `pcurl init`:

```text
.pcurl/
  collection.pcurl        # commit this
  secrets/
    .gitignore            # contains "*" → git ignores the whole folder
    secrets.env           # secrets, never committed
```

Format:

```text
@base = http://localhost:3000      # @name = value → variable
## Group                            # ## → group label
### request name                    # ### → request
curl {{base}}/users/1              # a curl command...
mcp {{base}}/mcp/message tools/call {"name":"x"}   # ...or an MCP call
```

- Variables substituted via `{{name}}` (single pass, no nesting).
- Path resolution: explicit arg → `.pcurl/collection.pcurl` → `./pcurl.collection`.
- `.pcurl/secrets/secrets.env` loads automatically; `--env <file>` **replaces** it (no stacking, so envs don't mix credentials); `--var k=v` wins.
- `@authorization` is sent as the `Authorization` header on MCP requests.
- Curl requests reuse `CurlCommand::parse` + `run_http_argument_mode`; MCP requests reuse `run_mcp_once`.

## Testing

- `cargo test` - runs unit + integration tests
- Unit tests: in `src/main.rs` (`#[cfg(test)]`) and `src/curl_parser.rs` (`#[cfg(test)]`)
- Integration tests: in `tests/integration_tests.rs`
  - Pipe-mode tests spawn binary via `cargo run --bin pcurl --` or `./target/debug/pcurl`
  - Streaming test (`test_streaming_headers_appear_before_body`) uses `TcpListener` local server
  - SSE test (`test_streaming_sse_events_appear_in_realtime`) uses TcpListener with event delays
  - MCP test (`test_mcp_subcommand`) uses TcpListener validating JSON-RPC + SSE response

## Release

- Push a tag `v*` to trigger GitHub Actions release workflow
- Builds for: Linux x64/ARM64, macOS x64/ARM64, Windows x64
- Artifacts uploaded to GitHub Releases automatically

## WebSocket Support

- Built-in: `pcurl ws ws://<url>` or `pcurl ws wss://<url>`
- Also: `pcurl wscat -c wss://<url>`
- `--verbose` flag shows ping/pong frames

## SSE / Event Streaming

- **Auto-detection**: `Content-Type: text/event-stream` detected in response headers
- **Real-time display**: Each `data:` line printed immediately with `←` prefix
- **No buffering**: `-N` (no-buffer) auto-injected to curl for instant output
- **Falls back** to normal body display for non-SSE responses

## MCP Support

- Built-in: `pcurl mcp <url> <method> <params>`
- **Full protocol cycle**: connects to `/mcp/sse`, obtains `session_id` from the `endpoint` event, sends `initialize`, then sends the user's method
- SSE connection kept open via `curl -N` subprocess, read in a background thread via `mpsc` channel
- POST requests sent via `ureq` (synchronous, no tokio needed)
- JSON-RPC body constructed with `serde_json::json!()` macro (auto-escaping, no injection)
- Responses matched to requests by JSON-RPC `id` field
- `--session-id` flag skips SSE and uses direct POST (old flow)
- `--verbose` shows SSE events, session info, and server handshake
- Falls back gracefully: if SSE fails, reports error and exits (no silent fallback)

### Architecture

- `src/mcp_client.rs` — `McpClient` struct: `.connect(message_url, headers, verbose)` → `.send_request(method, params)` → `.close()`
  - `headers` (e.g. `Authorization: Bearer …`) are sent on both the SSE `curl` connection and the `ureq` POST
  - SSE reader thread sends parsed `SseEvent` (type + data) through `mpsc::channel`
  - `send_request()` POSTs via `ureq`, then blocks on channel until response with matching `id` arrives
  - Timeout: 10s for initial `session_id`, 30s for response  
- `src/mcp.rs` — legacy helpers: `build_json_rpc_body()`, `get_or_create_session()` (used by direct fallback only)
- `src/main.rs` — `run_mcp_mode()` dispatches to `run_mcp_sse()` (new) or `run_mcp_direct()` (old, with `--session-id`)

## Project Structure

- `src/main.rs` - Entry point, CLI dispatch, curl execution with **streaming** (`.spawn()` + `BufReader`)
  - `run_http_argument_mode()` - spawns curl, streams headers then body line-by-line
  - `run_pipe_mode()` - batch reads stdin, uses `display_response()`
  - `run_collection()` / `list_collection()` / `save_request()` - collection subcommands
  - `run_mcp_mode()` - builds JSON-RPC and delegates to `execute_curl_and_stream()`
  - `execute_curl_and_stream()` - core streaming loop (used by HTTP + MCP modes)
- `src/collection.rs` - Plain-text request collections: parse, `{{var}}` substitution, append
- `src/cli.rs` - CLI definitions (clap derive structs: Cli, Commands, OutputMode)
- `src/display.rs` - HTTP response parsing and display (status, headers, body, JSON, XML)
  - `display_response()` - batch display (pipe mode)
  - `display_status_and_headers()` - streaming display of headers only
  - `display_body_section()` - streaming display of body only
  - `parse_status_code()` - extract HTTP status code from status line
  - `display_body()` - body formatter (JSON pretty-print, XML indent, plain text)
  - `is_sse_content_type()` - detect SSE from headers
  - `display_sse_line()` - display a single SSE event line
- `src/mcp.rs` - MCP legacy helpers (JSON-RPC body, session persistence)
- `src/mcp_client.rs` - MCP client: SSE connection, JSON-RPC over SSE/ureq
- `src/help.rs` - Doctor diagnostic
- `src/version.rs` - Version checking and self-update
- `src/ws_client.rs` - WebSocket client implementation
- `src/curl_parser.rs` - curl command tokenization and reconstruction
- `tests/integration_tests.rs` - End-to-end tests (pipe mode + streaming via TcpListener)
- `install.sh` - Universal installer script
- `.github/workflows/release.yml` - Multi-platform builds and releases
- `.github/workflows/quality_checks.yml` - CI: fmt, clippy, test, build on push/PR

## Dependencies

- `serde_json` - JSON parsing and formatting
- `colored` - Terminal colors
- `std::io::IsTerminal` - Stdin detection (pipe mode, std instead of atty)
- `tokio` + `tokio-tungstenite` + `futures-util` - WebSocket async runtime
- `url` - URL parsing for WebSocket
- `openssl` (vendored) - TLS support
- `ureq` - HTTP client for version checking (GitHub API)
- `clap` - CLI argument parsing (derive API, features: derive, color, wrap_help)

## Development Guidelines

- **New HTTP features**: Add to `src/display.rs` in display functions
- **New WebSocket features**: Add to `src/ws_client.rs`
- **Curl parsing issues**: Fix in `src/curl_parser.rs`
- **New CLI flags**: Add field to `Cli` or variant to `Commands` in `src/cli.rs`
- **Always test**: Add integration tests for new features
- **Help auto-generated**: Customize via `#[command(...)]` attributes in `src/cli.rs`
- **Version checking**: Uses `ureq` to query GitHub Releases API (`check_latest_version()` in `src/version.rs`)
- **Silent update notification**: `check_for_update_notification()` in `src/version.rs` runs on every HTTP request

### Streaming architecture

- `run_http_argument_mode()` and `run_mcp_mode()` both use `execute_curl_and_stream()` with `.spawn()` + `BufReader<stdout>`
- State machine: reads lines, detects HTTP/ status lines, accumulates header blocks
- Redirect blocks (3xx) are saved and only displayed if no final response follows (no `-L`)
- Final response headers are displayed immediately via `display_status_and_headers()`
- Remaining lines are collected as body and displayed via `display_body_section()`
- If Content-Type is `text/event-stream`, body lines are displayed inline via `display_sse_line()`
- Stderr is collected in a separate thread and filtered for real errors only

## Common Tasks

### Add new output format
1. Add detection in `display_body()` function in `src/display.rs`
2. Create formatting function (e.g., `print_yaml()`)
3. Add integration test in `tests/integration_tests.rs`

### Add SSE support for new content types
1. Edit `is_sse_content_type()` in `src/display.rs` to detect the new content type
2. SSE display happens in `display_sse_line()` — same function handles all SSE
3. Detection is called in `main.rs` right after the final response headers are found

### Fix curl parsing
1. Check tokenization in `curl_parser.rs`
2. Test with problematic command
3. Update parser logic if needed

### Modify MCP behavior
1. Session management: edit `get_or_create_session()` in `src/mcp.rs`
2. JSON-RPC construction: edit `build_json_rpc_body()` in `src/mcp.rs`
3. Dispatch: edit `run_mcp_mode()` in `src/main.rs`
4. Add integration test in `tests/integration_tests.rs`

### Add new CLI flag
1. Add field to `Cli` or variant to `Commands` in `src/cli.rs`
2. Create handler function (e.g., `print_doctor()` in `src/help.rs`)
3. Dispatch in `main()` if subcommand or flag
4. Add integration test if applicable

### Modify streaming behavior
1. Edit the state machine in `run_http_argument_mode()` in `src/main.rs`
2. Header detection happens in the main loop (looks for `HTTP/` prefix + empty line)
3. Body display calls `display_body_section()` in `src/display.rs`
4. Redirect handling: saved via `saved_redirect_headers`/`saved_redirect_body`, displayed only if no final response follows
5. SSE auto-detection: set `is_sse = true` when Content-Type is `text/event-stream`; body lines bypass collection and call `display_sse_line()` directly

### Update version
1. Change version in `Cargo.toml`
2. Update version in `install.sh` (hardcoded in banner)
3. Tag with `git tag v1.0.1`
4. Push: `git push --tags`