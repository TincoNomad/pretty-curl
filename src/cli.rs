use clap::{Parser, Subcommand};

// ------------------------------------------------------------
// Main CLI
// ------------------------------------------------------------

/// HTTP pretty-printer for your terminal.
/// Runs curl and beautifies the response — like Postman, without leaving your terminal.
#[derive(Parser, Debug)]
#[command(
    name = "pcurl",
    version,                          // reads version from Cargo.toml automatically
    author,
    about,
    long_about = None,
    after_help = "EXAMPLES:\n  pcurl 'curl https://api.example.com/users/1'\n  pcurl 'curl -X POST https://api.example.com/users -H \"Content-Type: application/json\" -d \\'{}\\'\n  curl -si https://api.example.com/users | pcurl\n  pcurl init\n  pcurl run --only \"health\"\n  pcurl list\n  pcurl save \"get user\" 'curl https://api.example.com/users/1'\n  pcurl ws ws://localhost:8080/chat\n  pcurl mcp http://localhost:8080/mcp/message tools/call '{\"name\":\"test\"}'",
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Full curl command in quotes, or direct URL.
    /// If omitted, pcurl reads from stdin (pipe mode).
    pub curl_command: Option<String>,

    /// Show only the body, without headers or status
    #[arg(long, global = true)]
    pub body_only: bool,

    /// Show only the headers, without body
    #[arg(long, global = true)]
    pub headers_only: bool,

    /// Force output without colors (useful for pipes or logs)
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Check installation and dependencies
    #[arg(long)]
    pub doctor: bool,

    /// Update pcurl to the latest available version
    #[arg(long)]
    pub update: bool,
}

// ------------------------------------------------------------
// Subcommands
// ------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create .pcurl/ in the current project (collection + secrets folder)
    Init,

    /// Run every request in a collection file (or a single one with --only)
    Run {
        /// Collection file (default: .pcurl/collection.pcurl, then ./pcurl.collection)
        file: Option<String>,

        /// Run only the request with this exact name
        #[arg(long)]
        only: Option<String>,

        /// Environment file with `@name = value` lines (overrides the collection's variables)
        #[arg(long)]
        env: Option<String>,

        /// Override a variable (repeatable): --var key=value
        #[arg(long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,
    },

    /// List the requests stored in a collection file
    List {
        /// Collection file (default: .pcurl/collection.pcurl, then ./pcurl.collection)
        file: Option<String>,
    },

    /// Save a curl command as a named request in a collection file
    Save {
        /// Name of the request
        name: String,

        /// The curl command (quote it)
        curl: String,

        /// Collection file to append to
        #[arg(long)]
        file: Option<String>,
    },

    /// Connect to a WebSocket server interactively
    Ws {
        /// WebSocket URL (ws:// or wss://)
        url: String,

        /// Show ping/pong messages
        #[arg(long)]
        verbose: bool,
    },

    /// wscat-compatible alias: pcurl wscat -c <url>
    Wscat {
        /// WebSocket URL
        #[arg(short = 'c', long)]
        connect: String,

        /// Show ping/pong messages
        #[arg(long)]
        verbose: bool,
    },

    /// Build and execute an MCP call (JSON-RPC over SSE).
    /// Connects to /mcp/sse, gets session_id, sends initialize, then your method.
    /// Use --session-id to skip SSE and POST directly.
    Mcp {
        /// MCP endpoint URL (e.g. http://localhost:8080/mcp/message)
        url: String,

        /// JSON-RPC method to call (e.g. tools/call)
        method: String,

        /// JSON-formatted parameters (e.g. '{"name":"test"}')
        params: String,

        /// Session ID to reuse an existing session
        #[arg(long)]
        session_id: Option<String>,

        /// Extra request header, sent on both SSE and POST (repeatable): --header "Authorization: Bearer <token>"
        #[arg(long = "header", value_name = "KEY: VALUE")]
        headers: Vec<String>,

        /// Show the internally constructed curl command
        #[arg(long)]
        verbose: bool,
    },
}

// ------------------------------------------------------------
// Helper struct for output options
// ------------------------------------------------------------

pub struct OutputMode {
    pub body_only: bool,
    pub headers_only: bool,
    pub no_color: bool,
}
