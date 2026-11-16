# pcurl

HTTP pretty-printer for your terminal. Takes raw `curl` output and makes it readable — like Postman or Bruno, but without leaving your terminal. Also manages and runs plain-text collections (`pcurl save`, `pcurl call`, `pcurl run`) and MCP servers (`pcurl mcp`).

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
- Verifies the download against the release's `SHA256SUMS` and aborts on a mismatch
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

Argument mode also substitutes `{{name}}` from the collection and
`secrets.env`, so you can reuse `@base` and `@token` without saving anything —
see [Ad-hoc requests](#ad-hoc-requests-can-use-the-same-variables).

**Pipe mode** — execute `curl` yourself and pipe output:

```bash
curl -si https://api.example.com/users/1 | pcurl
curl -si -X DELETE https://api.example.com/users/42 | pcurl
```

> `-s` silences progress bar, `-i` includes headers in stdout.

Binary or non-UTF-8 output is shown with replacement characters instead of
failing, so piping an image or a `.gz` won't crash pcurl.

**Global flags** (work in any HTTP mode):

```bash
pcurl --body-only 'curl https://api.example.com/users/1'   # Only JSON body
pcurl --headers-only 'curl https://api.example.com/users/1' # Only headers + status
pcurl --no-color 'curl https://api.example.com/users/1'    # No colors (also respects NO_COLOR env)
```

---

### Collections

Group your requests in a plain-text file and stop retyping curl commands.

#### Quick start

```bash
pcurl init            # creates .pcurl/ in the current project
pcurl set base http://localhost:3000
pcurl save "health" 'curl {{base}}/health'
pcurl list            # see what's inside, with its method
pcurl call health     # run that one
pcurl run             # run everything
```

`pcurl init` creates:

```
.pcurl/
  collection.pcurl        # commit this
  secrets/
    .gitignore            # contains "*" — git ignores the whole folder
    secrets.env           # API keys, passwords — never committed
```

The `secrets/.gitignore` with `*` means git ignores everything inside, even if
your project has no rule for it. No discipline required.

#### Format

```text
@base = http://localhost:3000
@token = dev-token

## Usuarios
### get user
curl {{base}}/users/1 -H "Authorization: Bearer {{token}}"

### create user
curl -X POST {{base}}/users -d '{"name":"Juan"}'
```

- `###` starts a named request, `##` a group, `@name = value` a variable, `#` a comment.
- A request's curl may span several lines with a trailing `\`.
- A request can also be an MCP call: `mcp <url> <method> <json>`.
- A repeated `@name` line: the **last** one wins, the same as `git config --get`.

Groups are a reading label, and also a prefix `call` understands: `list` prints
each request as `group/name`, and that is a valid name to run. They are not
separate files — everything lives in one collection.

```bash
pcurl list                                          # show what's inside
pcurl run                                           # run everything
pcurl run --only "get user"                         # run a single request
pcurl call "Usuarios/get user"                      # same, by the name `list` prints
pcurl run --var base=http://localhost:9999          # override one variable
pcurl save "get user" 'curl https://api.example.com/users/1'   # append or update a request
pcurl save --group Auth "login" - <<'EOF'           # ...or paste it multi-line into a group
curl -X POST {{base}}/auth/login -d '{
  "email": "{{email}}"
}'
EOF
pcurl rm "get user"                                 # delete a request
pcurl set base http://localhost:3000                # write @base into the collection
pcurl set --secret token eyJhbGciOi...              # write @token into secrets.env
pcurl unset token --secret                          # remove a variable
```

`pcurl list` output shows the method and the name you can paste into `call`:

```text
collection .pcurl/collection.pcurl
  @base = http://localhost:3000

Usuarios
  GET    Usuarios/get user
  POST   Usuarios/create user
```

`save -` reads the curl from stdin, so a heredoc keeps the body multi-line and
literal (`<<'EOF'` stops the shell from expanding anything). Saving a name that
already exists **updates that block in place**, so `call` can never run a stale
duplicate. `--group` puts the request at the end of that group, creating it at
the bottom when missing.

`set` never duplicates a key: running it twice updates the same line, and a new
key joins the existing block of `@` lines. `unset` removes every line for that
key. Values printed by `list` come from the collection only, so a secret's value
is never echoed.

The file is resolved as: explicit path → `.pcurl/collection.pcurl` → `./pcurl.collection`.

#### Secrets and environments

`.pcurl/secrets/secrets.env` is loaded automatically and overrides the collection's variables:

```text
@base = https://staging.example.com
@token = staging-secret
```

For other environments use `--env`. It **replaces** the automatic file instead of
merging, so you never mix a production URL with a development token:

```bash
pcurl run --env staging.env
```

Resolution order: collection variables → secrets (or `--env`) → `--var`.

#### Ad-hoc requests can use the same variables

A curl passed straight to pcurl is substituted too, so `@base` and `@token` work
without saving anything:

```bash
pcurl "{{base}}/auth/login -d '{\"email\":\"a@b.com\",\"password\":\"12345678\"}'"
pcurl "{{base}}/auth/logout -H \"Authorization: Bearer {{token}}\""
```

The collection file is resolved from the current directory, so run it from the
project root. A `{{name}}` that is not defined is reported on stderr instead of
being sent literally.

#### MCP requests inside a collection

```text
### stats
mcp {{base}}/mcp/message tools/call {"name":"get_stats_summary","arguments":{}}
```

The line is read as `mcp <url> <method> <json>`; everything after the method is
treated as JSON, so spaces inside it are safe. If a variable named
`@authorization` exists, it is sent as the `Authorization` header — put it in
`secrets.env`, not in the collection.

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

# Protected server: send a header on both the SSE stream and the POST
pcurl mcp --header "Authorization: Bearer <token>" https://api.example.com/mcp/message tools/list '{}'
```

The URL is the **message** endpoint (`.../mcp/message`), not the SSE one. pcurl
derives `/mcp/sse` from it by replacing `/mcp/message` in the path, so a URL
without that segment will time out after 10s.

Features:
- **Full protocol cycle**: connects to `/mcp/sse`, obtains `session_id`, sends `initialize`, then your method
- **JSON-RPC body** constructed automatically via `serde_json` (proper escaping, no injection)
- **SSE kept open**: responses arrive through the live SSE stream, matched by JSON-RPC `id`
- **Tool errors are reported**: a result with `isError: true` is shown as `✗ [ERROR]`, not as success
- `--header "Key: Value"` (repeatable) is sent on **both** the SSE connection and the POST
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
| `pcurl init` | Create `.pcurl/` (collection + secrets folder) in the current project |
| `pcurl run [file] [--only <name>] [--env <file>] [--var k=v]` | Run a collection; `--only` accepts `name` or `group/name` |
| `pcurl call <name> [--env <file>] [--var k=v] [--file <path>]` | Run one saved request by name (`name` or `group/name`) |
| `pcurl list [file]` | List the requests with their method, as `group/name` |
| `pcurl save <name> "<curl…>" [--group <g>] [--file <path>]` | Save a request, or update it if the name exists (`-` reads the curl from stdin) |
| `pcurl rm <name> [--file <path>]` | Delete a saved request (every block with that name) |
| `pcurl set <key> <value> [--secret] [--file <path>]` | Write `@key = value` into the collection (or secrets) |
| `pcurl unset <key> [--secret] [--file <path>]` | Remove `@key` from the collection (or secrets) |
| `pcurl ws <url> [--verbose]` | WebSocket native mode |
| `pcurl wscat -c <url> [--verbose]` | wscat-compatible alias |
| `pcurl mcp <url> <method> <params> [--header "K: V"] [--session-id <id>] [--verbose]` | MCP JSON-RPC call (full SSE cycle) |
| `--body-only` | Show only response body |
| `--headers-only` | Show only headers + status |
| `--no-color` | Disable colors (also via `NO_COLOR` env) |
| `--doctor` | Diagnose installation and PATH |
| `--update` | Update to latest version via install.sh |
| `--version` / `-V` | Show version |
| `--help` / `-h` | Show help |

### Exit codes

pcurl exits `0` when it could do its job and `1` when it could not. The HTTP
status is data, not a failure: a `500` prints like any other response. What
counts as a failure depends on how you use it:

| Mode | Exit `1` when |
|---|---|
| `pcurl 'curl …'` / pipe | curl produced no HTTP response at all (refused, DNS failure) |
| `pcurl call` / `pcurl run` | any request failed — no response **or** a status outside `2xx` |

`run` and `call` behave like a test runner: every request still runs (a failure
does not abort the rest), and the exit code at the end says whether all of them
succeeded, so the command can gate a CI step. Ad-hoc mode behaves like `curl`
without `-f`, where a `404` is a valid answer worth reading.

---

## Why pcurl vs alternatives

| | pcurl | xh | HTTPie | curlie |
|---|---|---|---|---|
| Accepts literal curl commands | ✓ | ✗ | ✗ | partial |
| Plain-text collections managed from the CLI | ✓ | ✗ | ✗ | ✗ |
| WebSocket support | ✓ | ✗ | ✓ | ✗ |
| Single binary, no runtime | ✓ | ✓ | ✗ | ✓ |
| Written in | Rust | Rust | Python | Go |

If you already know curl and just want readable output, pcurl requires
zero new syntax. If you're comparing with xh: xh is great but doesn't
support WebSocket and requires learning its own request format.

---

## Self-Update

pcurl never talks to the network on its own — no background check, no phone
home, nothing added to the latency of a command:

- **`--version` / `-V`**: prints the version and, if a newer release exists, one line telling you to update. This is the only place pcurl contacts GitHub during normal use, and it gives up after 2 seconds.
- **`--update`**: runs the installer to fetch the latest release. The installer is downloaded from the release **tag**, not from `main`, and checked binaries are verified against the release's `SHA256SUMS`.
- **`--doctor`**: diagnoses installation issues (binary location, PATH, curl dependency, connectivity).

---

## Security Notice

⚠️ **This tool is designed for local development and testing only.**

- **Auto-update**: `--update` downloads the installer from the release tag and runs it; the installer verifies the binary against the release's `SHA256SUMS` and aborts on a mismatch. If you don't trust any of this, use manual download or compile from source.
- **Dependencies**: checked with `cargo audit` in CI, so a known advisory fails the build instead of shipping.
- **Pipe mode**: `pcurl` only reads from stdin and displays output — it does not execute arbitrary code.
- **Argument mode**: Passes arguments directly to `curl`. Be careful with untrusted input.
- **Secrets**: keep API keys in `.pcurl/secrets/secrets.env`. `pcurl init` writes a `.gitignore` with `*` inside that folder, so git ignores it even if your project has no rule. The file itself is **plain text on disk** — it is not encrypted, and `pcurl` does not ask for a passphrase.
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
- `src/collection.rs` — Plain-text request collections (parsing, variables, `set`/`save`/`rm`, group-qualified lookup)
- `src/curl_parser.rs` — curl command tokenization and reconstruction
- `src/display.rs` — HTTP response parsing and display (status, headers, body, JSON, XML)
- `src/mcp.rs` — MCP legacy helpers (JSON-RPC body, session persistence)
- `src/mcp_client.rs` — MCP client: SSE connection, JSON-RPC over SSE/ureq
- `src/ws_client.rs` — WebSocket client implementation
- `src/version.rs` — Version checking and self-update
- `src/help.rs` — Doctor diagnostic

---

## Buy me a coffee

If pcurl saves you time, you can send USDC on Polygon:

<p align="center">
  <img src="docs/donate-qr.webp" width="220" alt="USDC on Polygon QR">
</p>

Or copy the address:

```text
0x829eA0c8F745dd461144443E050c6cf00a58D72b
```

Network: **Polygon PoS** · Token: **USDC** · [verify on Polygonscan](https://polygonscan.com/address/0x829eA0c8F745dd461144443E050c6cf00a58D72b)

Check both the network and the token before sending — anything sent on a different chain may be lost.
