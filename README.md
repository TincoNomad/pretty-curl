# pcurl

HTTP pretty-printer for your terminal. Takes raw `curl` output and makes it readable — like Postman or Bruno, but without leaving your terminal.

```
  ✓  HTTP/2 200 OK    142ms

  HEADERS
  ──────────────────────────────────────────────────────
  Content-Type                   application/json
  X-Request-Id                   abc-def-123

  BODY
  ──────────────────────────────────────────────────────
{
  "id": 42,
  "nombre": "Juan Pérez",
  "permisos": [
    "read",
    "write",
    "delete"
  ]
}
```

## Installation

### Option 1 — Universal Installer (recommended)

**No Rust needed. Downloads precompiled binary for your system:**

```bash
curl -sSL https://raw.githubusercontent.com/TincoNomad/pretty-curl/main/install.sh | bash
```

The installer automatically detects:
- Linux (x64, ARM64)
- macOS (Intel, Apple Silicon)
- Downloads correct binary from GitHub Releases
- Fallback to compilation if no binary available

### Option 2 — Manual Download

Go to [GitHub Releases](https://github.com/TincoNomad/pretty-curl/releases) and download:

- `pcurl-linux-x64` - Linux 64-bit
- `pcurl-linux-arm64` - Linux ARM64
- `pcurl-macos-x64` - macOS Intel
- `pcurl-macos-arm64` - macOS Apple Silicon
- `pcurl-windows-x64.exe` - Windows 64-bit

Then:
```bash
# Linux/macOS
chmod +x pcurl-*
sudo cp pcurl-* /usr/local/bin/pcurl

# Windows
# Move pcurl.exe to a directory in your PATH
```

### Option 3 — Compile from Source

If you have Rust installed:

```bash
git clone https://github.com/TincoNomad/pretty-curl
cd pretty-curl
cargo build --release
sudo cp target/release/pcurl /usr/local/bin/
```

---

## Usage

### HTTP Mode

**Argument mode** — `pcurl` executes `curl` for you and prettifies the response:

```bash
# Simple GET
pcurl 'curl https://api.example.com/users/1'

# POST with JSON (note the escaped inner quotes)
pcurl "curl -X POST https://api.example.com/users \
  -H 'Authorization: Bearer <token>' \
  -d '{\"name\":\"Juan\",\"role\":\"admin\"}'"

# With extra flags
pcurl 'curl -L -k https://api.internal.com/health'
pcurl 'curl -u user:password https://api.example.com/private'

# Direct URL (adds -i -s -N automatically)
pcurl https://api.example.com/users/1
```

**Pipe mode** — execute `curl` yourself and pipe output:

```bash
curl -si https://api.example.com/users/1 | pcurl
curl -si -X DELETE https://api.example.com/users/42 | pcurl
```

> `-s` silences progress bar, `-i` includes headers in stdout.

**Global flags** (work in any HTTP mode):

```bash
pcurl --body-only 'curl https://api.example.com/users/1'   # Only JSON body
pcurl --headers-only 'curl https://api.example.com/users/1' # Only headers + status
pcurl --no-color 'curl https://api.example.com/users/1'    # No colors (also respects NO_COLOR env)
```

---

### Collections

Group your requests in a plain-text file and stop retyping curl commands. A collection is just named curl commands plus variables:

```text
@base = http://localhost:3000
@token = dev-token

## Usuarios
### get user
curl {{base}}/users/1 -H "Authorization: Bearer {{token}}"

### create user
curl -X POST {{base}}/users -d '{"name":"Juan"}'
```

- `###` starts a named request, `##` a group (label only), `@name = value` a variable, `#` a comment.
- A request's curl may span several lines with a trailing `\`.

```bash
pcurl list api.pcurl                                # show what's inside
pcurl run api.pcurl                                 # run everything
pcurl run api.pcurl --only "get user"               # run a single request
pcurl run api.pcurl --env staging.env               # override variables (gitignore the secrets!)
pcurl run api.pcurl --var base=http://localhost:9999  # override one variable inline
pcurl save "get user" 'curl https://api.example.com/users/1'   # append a request
```

Environment files use the same `@name = value` syntax and override the collection's variables:

```text
@base = https://staging.example.com
@token = staging-secret
```

Anything you can paste from your browser's **"Copy as cURL"** works as a request — no new syntax to learn.

---

### WebSocket Mode

```bash
# Native WebSocket subcommand
pcurl ws wss://echo.websocket.org
pcurl ws ws://localhost:8080/chat --verbose

# wscat-compatible alias
pcurl wscat -c wss://echo.websocket.org
pcurl wscat -c wss://echo.websocket.org --verbose
```

Features:
- **Automatic JSON prettifier** for messages
- **Colored prefixes**: `←` incoming (green), `→` outgoing (cyan)
- **Interactive**: Type messages and press Enter
- **`/quit` command** to close connection
- **Connection status** on startup
- `--verbose` shows ping/pong frames

Example session:
```
↔ wss://echo.websocket.org
────────────────────────────────────────────────────────────
✓ Connected! (HTTP 101)
────────────────────────────────────────────────────────────
Type messages and press Enter. /quit to exit.

> hello world
← "hello world"

> {"type":"ping","timestamp":123456}
← {
      "type": "ping",
      "timestamp": 123456
   }
```

---

### MCP Mode (Model Context Protocol)

```bash
# Basic call — full protocol cycle: SSE → session_id → initialize → your method
pcurl mcp http://localhost:8080/mcp/message tools/call '{"name":"test"}'

# With explicit session (skips SSE, direct POST)
pcurl mcp --session-id my-session http://localhost:8080/mcp/message resources/list '{}'

# See the full handshake: SSE events, session info, initialize response
pcurl mcp --verbose http://localhost:8080/mcp/message tools/call '{"name":"test"}'
```

Features:
- **Full protocol cycle**: connects to `/mcp/sse`, obtains `session_id`, sends `initialize`, then your method
- **JSON-RPC body** constructed automatically via `serde_json` (proper escaping, no injection)
- **SSE kept open**: responses arrive through the live SSE stream, matched by JSON-RPC `id`
- **No globbing issues**: JSON params passed as single argument, safe from zsh expansion
- `--session-id <id>` for explicit session control (direct POST, skips SSE)
- `--verbose` shows SSE events, session info, initialize handshake, and server info

Example flow with `--verbose`:
```
  ↻ SSE http://localhost:8080/mcp/sse
  ← event: endpoint  data: /mcp/message?session_id=abc-123
  ✓ Session: abc-123
  → POST http://localhost:8080/mcp/message?session_id=abc-123
  → {"jsonrpc":"2.0","id":1,"method":"initialize",...}
  ← event: message  data: {"jsonrpc":"2.0","id":1,"result":{...}}
  ✓ Initialized: my-server 1.0
  → POST http://localhost:8080/mcp/message?session_id=abc-123
  → {"jsonrpc":"2.0","id":2,"method":"tools/call",...}
  ← event: message  data: {"jsonrpc":"2.0","id":2,"result":{...}}
  ✓ [OK]  142 ms
```

---

## CLI Reference

| Command / Flag | Description |
|---|---|
| `pcurl [FLAGS] [curl_command]` | HTTP mode (arg or pipe) |
| `pcurl run [file] [--only <name>] [--env <file>] [--var k=v]` | Run a collection (default file: `pcurl.collection`) |
| `pcurl list [file]` | List the requests in a collection |
| `pcurl save <name> "<curl…>" [--file <path>]` | Append a request to a collection |
| `pcurl ws <url> [--verbose]` | WebSocket native mode |
| `pcurl wscat -c <url> [--verbose]` | wscat-compatible alias |
| `pcurl mcp <url> <method> <params> [--session-id <id>] [--verbose]` | MCP JSON-RPC call (full SSE cycle) |
| `--body-only` | Show only response body |
| `--headers-only` | Show only headers + status |
| `--no-color` | Disable colors (also via `NO_COLOR` env) |
| `--doctor` | Diagnose installation and PATH |
| `--update` | Update to latest version via install.sh |
| `--version` / `-V` | Show version |
| `--help` / `-h` | Show help |

---

## Why pcurl vs alternatives

| | pcurl | xh | HTTPie | curlie |
|---|---|---|---|---|
| Accepts literal curl commands | ✓ | ✗ | ✗ | partial |
| WebSocket support | ✓ | ✗ | ✓ | ✗ |
| Single binary, no runtime | ✓ | ✓ | ✗ | ✓ |
| Written in | Rust | Rust | Python | Go |

If you already know curl and just want readable output, pcurl requires
zero new syntax. If you're comparing with xh: xh is great but doesn't
support WebSocket and requires learning its own request format.

---

## Self-Update

`pcurl` checks for newer versions automatically:

- **After each successful run** (HTTP, WebSocket, MCP): a silent check runs and prints a notice if a newer release exists
- **`--version` / `--help`**: print and exit immediately — no update check
- **`--update`**: Runs the installer to update to the latest release
- **`--doctor`**: Diagnoses installation issues (binary location, PATH, curl dependency, connectivity)

---

## Security Notice

⚠️ **This tool is designed for local development and testing only.**

- **Auto-update**: Downloads and executes code from GitHub. If you don't trust this, use manual download or compile from source.
- **Pipe mode**: `pcurl` only reads from stdin and displays output — it does not execute arbitrary code.
- **Argument mode**: Passes arguments directly to `curl`. Be careful with untrusted input.
- **For production use**: Review the code and consider security implications before use in sensitive environments.

### Safe Usage Guidelines

✅ Safe: Testing your own APIs locally  
✅ Safe: Connecting to trusted internal services  
⚠️ Caution: Using with `-k` / `--insecure` in production  
❌ Avoid: Piping untrusted network data directly to `pcurl`

---

## Contributing

PRs welcome. Code structure:

- `src/main.rs` — Entry point, CLI dispatch (clap), curl execution
- `src/cli.rs` — CLI definitions (clap derive structs)
- `src/collection.rs` — Plain-text request collections (parsing, variables, save)
- `src/curl_parser.rs` — curl command tokenization and reconstruction
- `src/display.rs` — HTTP response parsing and display (status, headers, body, JSON, XML)
- `src/mcp.rs` — MCP legacy helpers (JSON-RPC body, session persistence)
- `src/mcp_client.rs` — MCP client: SSE connection, JSON-RPC over SSE/ureq
- `src/ws_client.rs` — WebSocket client implementation
- `src/version.rs` — Version checking and self-update
- `src/help.rs` — Doctor diagnostic
