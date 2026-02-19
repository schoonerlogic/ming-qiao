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

use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

use ming_qiao::http::HttpServer;
use ming_qiao::mcp::McpServer;
use ming_qiao::nats::{NatsAgentClient, NatsMessage};
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

/// Spawn a background task that persists incoming NATS messages to SurrealDB.
fn spawn_nats_persistence_bridge(state: &AppState) {
    let mut rx = state.subscribe_nats_messages();
    let persistence = state.persistence().clone();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let result = match &msg {
                        NatsMessage::Presence(p) => {
                            persistence.store_presence(p).await
                        }
                        NatsMessage::TaskAssignment(ta) => {
                            // Use "mingqiao" as default project for now
                            persistence.store_task_assignment(ta, "mingqiao").await
                        }
                        NatsMessage::TaskStatusUpdate(ts) => {
                            persistence.store_task_status(ts, "mingqiao").await
                        }
                        NatsMessage::SessionNote(sn) => {
                            persistence.store_session_note(sn).await
                        }
                    };
                    if let Err(e) = result {
                        warn!("Failed to persist NATS message: {}", e);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("NATS persistence bridge lagged by {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    info!("NATS message channel closed, stopping persistence bridge");
                    break;
                }
            }
        }
    });
}

/// Run the HTTP server
async fn run_http_server() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = env::var("MING_QIAO_CONFIG").unwrap_or_else(|_| "ming-qiao.toml".to_string());

    let state = match AppState::load(&config_path).await {
        Ok(s) => {
            info!("Loaded config from {}", config_path);
            s
        }
        Err(e) => {
            info!("Using default config ({})", e);
            AppState::new().await
        }
    };

    // Ensure data directories exist
    state.ensure_dirs()?;

    // Connect NATS agent client if enabled
    let nats_config = state.config().await.nats;
    let agent_id = state
        .agent_id()
        .unwrap_or("http-server")
        .to_string();
    if let Some(mut client) = NatsAgentClient::connect(&nats_config, &agent_id, "mingqiao").await {
        let nats_tx = state.nats_message_sender();

        if let Err(e) = client.subscribe_all_tasks(nats_tx.clone()).await {
            error!("Failed to subscribe to tasks: {}", e);
        }
        if let Err(e) = client.subscribe_notes(nats_tx.clone()).await {
            error!("Failed to subscribe to notes: {}", e);
        }
        if let Err(e) = client.subscribe_presence(nats_tx).await {
            error!("Failed to subscribe to presence: {}", e);
        }

        client.start_presence_heartbeat(
            "main".to_string(),
            "serving HTTP".to_string(),
        );

        state.set_nats_client(client).await;

        // Bridge NATS messages → SurrealDB persistence
        spawn_nats_persistence_bridge(&state);

        info!("NATS agent client active for HTTP server (subscriptions + heartbeat + persistence)");
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

    let config_path = env::var("MING_QIAO_CONFIG").unwrap_or_else(|_| "ming-qiao.toml".to_string());

    let state = match AppState::load(&config_path).await {
        Ok(s) => s,
        Err(_) => AppState::new().await,
    };

    state.ensure_dirs()?;

    // Connect NATS agent client if enabled
    let nats_config = state.config().await.nats;
    if let Some(mut client) = NatsAgentClient::connect(&nats_config, &agent_id, "mingqiao").await {
        let nats_tx = state.nats_message_sender();

        if let Err(e) = client.subscribe_own_tasks(nats_tx.clone()).await {
            error!("Failed to subscribe to own tasks: {}", e);
        }
        if let Err(e) = client.subscribe_all_tasks(nats_tx.clone()).await {
            error!("Failed to subscribe to all tasks: {}", e);
        }
        if let Err(e) = client.subscribe_notes(nats_tx.clone()).await {
            error!("Failed to subscribe to notes: {}", e);
        }
        if let Err(e) = client.subscribe_presence(nats_tx).await {
            error!("Failed to subscribe to presence: {}", e);
        }

        let branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        client.start_presence_heartbeat(branch, "available".to_string());

        state.set_nats_client(client).await;

        // Bridge NATS messages → SurrealDB persistence
        spawn_nats_persistence_bridge(&state);

        info!("NATS agent client active for MCP server (subscriptions + heartbeat + persistence)");
    }

    info!("Starting MCP server for agent: {}", agent_id);

    let mut server = McpServer::with_state(agent_id, state);
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
