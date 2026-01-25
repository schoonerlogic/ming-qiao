//! Ming-Qiao: Communication bridge for the Council of Wizards
//!
//! This is the main entry point for the ming-qiao server.
//!
//! ## Usage
//!
//! ```bash
//! # Run HTTP server (default)
//! ming-qiao serve
//!
//! # Run MCP server (for Claude CLI)
//! ming-qiao mcp-serve
//!
//! # Show help
//! ming-qiao --help
//! ```

use std::env;
use std::process::ExitCode;

use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

use ming_qiao::http::HttpServer;
use ming_qiao::mcp::McpServer;
use ming_qiao::state::AppState;

/// Initialize logging with tracing
fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,ming_qiao=debug"));

    fmt().with_env_filter(filter).with_target(true).init();
}

/// Print usage information
fn print_usage() {
    eprintln!(
        r#"Ming-Qiao: Communication bridge for the Council of Wizards

USAGE:
    ming-qiao <COMMAND>

COMMANDS:
    serve       Run the HTTP server (for Thales and dashboard)
    mcp-serve   Run the MCP server (for Aleph via Claude CLI)
    help        Print this help message

ENVIRONMENT:
    MING_QIAO_CONFIG     Path to config file (default: ming-qiao.toml)
    MING_QIAO_DATA_DIR   Path to data directory (default: data)
    MING_QIAO_AGENT_ID   Agent identity for MCP mode (required for mcp-serve)
    RUST_LOG             Log level filter (default: info,ming_qiao=debug)

EXAMPLES:
    # Start HTTP server on default port 7777
    ming-qiao serve

    # Start MCP server for Aleph
    MING_QIAO_AGENT_ID=aleph ming-qiao mcp-serve
"#
    );
}

/// Run the HTTP server
async fn run_http_server() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = env::var("MING_QIAO_CONFIG").unwrap_or_else(|_| "ming-qiao.toml".to_string());

    let state = match AppState::load(&config_path) {
        Ok(s) => {
            info!("Loaded config from {}", config_path);
            s
        }
        Err(e) => {
            info!("Using default config ({})", e);
            AppState::default()
        }
    };

    // Ensure data directories exist
    state.ensure_dirs()?;

    // Auto-refresh indexer with event log
    if let Err(e) = state.refresh_indexer().await {
        info!("Indexer refresh failed: {}", e);
        info!("Server will start with empty indexer");
    }
 
    let server = HttpServer::new(state);
    info!("Starting HTTP server at http://{}", server.address());

    server.run().await?;
    Ok(())
}

/// Run the MCP server
async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    let agent_id = env::var("MING_QIAO_AGENT_ID").unwrap_or_else(|_| {
        error!("MING_QIAO_AGENT_ID not set, using 'unknown'");
        "unknown".to_string()
    });

    info!("Starting MCP server for agent: {}", agent_id);

    let mut server = McpServer::new(agent_id);
    server.run().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return ExitCode::FAILURE;
    }

    let command = &args[1];

    // Initialize logging (skip for mcp-serve to keep stdio clean)
    if command != "mcp-serve" {
        init_logging();
    }

    let result = match command.as_str() {
        "serve" => run_http_server().await,
        "mcp-serve" => run_mcp_server().await,
        "help" | "--help" | "-h" => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("Server error: {}", e);
            ExitCode::FAILURE
        }
    }
}
