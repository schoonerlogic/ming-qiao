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
use ming_qiao::nats::streams;
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
fn spawn_nats_persistence_bridge(state: &AppState, project: &str) {
    let mut rx = state.subscribe_nats_messages();
    let persistence = state.persistence().clone();
    let project = project.to_string();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let result = match &msg {
                        NatsMessage::Presence(p) => {
                            persistence.store_presence(p).await
                        }
                        NatsMessage::TaskAssignment(ta) => {
                            persistence.store_task_assignment(ta, &project).await
                        }
                        NatsMessage::TaskStatusUpdate(ts) => {
                            persistence.store_task_status(ts, &project).await
                        }
                        NatsMessage::SessionNote(sn) => {
                            persistence.store_session_note(sn).await
                        }
                        NatsMessage::MessageNotification(_) => {
                            // Ephemeral hint — no persistence needed
                            Ok(())
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

/// Spawn a background task that publishes local events to NATS for cross-process sync.
///
/// Subscribes to the AppState event broadcast channel and publishes each
/// EventEnvelope to the shared NATS events subject. The subscriber on the
/// remote process picks it up and feeds it to its Indexer (dedup handles echo).
fn spawn_event_nats_publisher(
    state: &AppState,
    client: async_nats::Client,
    subject: String,
    use_tracing: bool,
) {
    let mut rx = state.subscribe_events();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    match serde_json::to_vec(&event) {
                        Ok(payload) => {
                            if let Err(e) = client.publish(subject.clone(), payload.into()).await {
                                if use_tracing {
                                    warn!("Event NATS publish failed: {}", e);
                                } else {
                                    eprintln!("[ming-qiao] Event NATS publish failed: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            if use_tracing {
                                warn!("Event serialization failed: {}", e);
                            } else {
                                eprintln!("[ming-qiao] Event serialization failed: {}", e);
                            }
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    if use_tracing {
                        warn!("Event NATS publisher lagged by {} messages", n);
                    } else {
                        eprintln!("[ming-qiao] Event NATS publisher lagged by {} messages", n);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    if use_tracing {
                        info!("Event channel closed, stopping NATS publisher");
                    } else {
                        eprintln!("[ming-qiao] Event channel closed, stopping NATS publisher");
                    }
                    break;
                }
            }
        }
    });
}

/// Spawn a background task that receives events from NATS and feeds the local Indexer.
///
/// Does NOT call `broadcast_event()` to avoid echo loops back to the publisher.
/// Dedup in the Indexer handles the case where a local event echoes back via NATS.
fn spawn_event_nats_subscriber(
    state: &AppState,
    client: async_nats::Client,
    subject: String,
    use_tracing: bool,
) {
    let state = state.clone();

    tokio::spawn(async move {
        let subscription = match client.subscribe(subject.clone()).await {
            Ok(s) => s,
            Err(e) => {
                if use_tracing {
                    error!("Failed to subscribe to event sync subject {}: {}", subject, e);
                } else {
                    eprintln!("[ming-qiao] Failed to subscribe to event sync subject {}: {}", subject, e);
                }
                return;
            }
        };

        if use_tracing {
            info!("Event NATS subscriber active on {}", subject);
        } else {
            eprintln!("[ming-qiao] Event NATS subscriber active on {}", subject);
        }

        use futures_util::StreamExt;
        let mut subscription = subscription;
        while let Some(msg) = subscription.next().await {
            match serde_json::from_slice::<ming_qiao::events::EventEnvelope>(&msg.payload) {
                Ok(event) => {
                    let mut indexer = state.indexer_mut().await;
                    if let Err(e) = indexer.process_event(&event) {
                        if use_tracing {
                            warn!("Indexer rejected remote event {}: {}", event.id, e);
                        } else {
                            eprintln!("[ming-qiao] Indexer rejected remote event {}: {}", event.id, e);
                        }
                    }
                }
                Err(e) => {
                    if use_tracing {
                        warn!("Failed to deserialize event from NATS: {}", e);
                    } else {
                        eprintln!("[ming-qiao] Failed to deserialize event from NATS: {}", e);
                    }
                }
            }
        }

        if use_tracing {
            info!("Event NATS subscription on {} ended", subject);
        } else {
            eprintln!("[ming-qiao] Event NATS subscription on {} ended", subject);
        }
    });
}

/// Spawn a background task that consumes message events from JetStream
/// and writes them to SurrealDB + Indexer.
///
/// This is the durable delivery consumer (Phase 2). MCP subprocesses publish
/// EventEnvelopes to the AGENT_MESSAGES stream when the HTTP API is unreachable.
/// This consumer ingests them on the HTTP server side, ensuring no messages are lost.
///
/// On startup, any unacked messages (published while HTTP was down) are replayed.
fn spawn_jetstream_message_consumer(state: &AppState, js: async_nats::jetstream::Context) {
    let state = state.clone();

    tokio::spawn(async move {
        let stream = match js.get_stream(streams::STREAM_AGENT_MESSAGES).await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get AGENT_MESSAGES stream: {}", e);
                return;
            }
        };

        let (consumer_name, config) = streams::message_ingester_consumer_config();
        let consumer = match stream.get_or_create_consumer(&consumer_name, config).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to create message ingester consumer: {}", e);
                return;
            }
        };

        let mut messages = match consumer.messages().await {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to start message ingester: {}", e);
                return;
            }
        };

        info!("JetStream message ingester active (consumer: {})", consumer_name);

        use futures_util::StreamExt;
        while let Some(msg_result) = messages.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    warn!("JetStream message ingester error: {}", e);
                    continue;
                }
            };

            match serde_json::from_slice::<ming_qiao::events::EventEnvelope>(&msg.payload) {
                Ok(event) => {
                    // Write to SurrealDB
                    if let Err(e) = state.persistence().store_event(&event).await {
                        // Check if it's a duplicate (already in DB)
                        let err_str = e.to_string();
                        if err_str.contains("already exists") || err_str.contains("duplicate") {
                            // Expected during replay — event was already written by HTTP path
                            info!("JetStream ingester: event {} already in SurrealDB (dedup)", event.id);
                        } else {
                            warn!("JetStream ingester: SurrealDB write failed for {}: {} (will redeliver)", event.id, e);
                            // Don't ack — JetStream will redeliver
                            continue;
                        }
                    }

                    // Update Indexer (dedup via seen_ids)
                    {
                        let mut indexer = state.indexer_mut().await;
                        if let Err(e) = indexer.process_event(&event) {
                            // Dedup rejection is expected, not an error
                            info!("JetStream ingester: indexer skipped event {}: {}", event.id, e);
                        }
                    }

                    // Push to connected Streamable HTTP agents via PushBroker
                    // This is the universal delivery path — fires for ALL messages
                    // regardless of origin (HTTP, MCP stdio, MCP Streamable HTTP).
                    if event.event_type == ming_qiao::events::EventType::MessageSent {
                        if let ming_qiao::events::EventPayload::Message(ref m) = event.payload {
                            state.push_broker().publish(
                                &m.to,
                                ming_qiao::mcp::streamable_http::PushEvent {
                                    from: m.from.clone(),
                                    subject: m.subject.clone(),
                                    intent: format!("{:?}", m.intent),
                                    message_id: event.id.to_string(),
                                },
                            ).await;
                        }
                    }

                    // Broadcast to WebSocket listeners
                    state.broadcast_event(event);

                    // Ack — message successfully processed
                    if let Err(e) = msg.ack().await {
                        warn!("JetStream ingester: failed to ack message: {}", e);
                    }
                }
                Err(e) => {
                    warn!("JetStream ingester: bad EventEnvelope payload: {}", e);
                    // Ack garbage so it doesn't redeliver forever
                    if let Err(e) = msg.ack().await {
                        warn!("JetStream ingester: failed to ack bad message: {}", e);
                    }
                }
            }
        }

        info!("JetStream message ingester ended");
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
    let config_snapshot = state.config().await;
    let nats_config = config_snapshot.nats;
    let project = config_snapshot.project;
    let agent_id = state
        .agent_id()
        .unwrap_or("http-server")
        .to_string();
    if let Some(mut client) = NatsAgentClient::connect(&nats_config, &agent_id, &project).await {
        // Extract event sync parts before moving client into state
        let (nats_raw, events_subject) = client.event_sync_parts();

        // Store JetStream context for durable message publishing (Phase 2)
        let js_context = client.jetstream().clone();
        state.set_jetstream_context(js_context.clone()).await;

        let nats_tx = state.nats_message_sender();

        if let Err(e) = client.subscribe_all_tasks(nats_tx.clone()).await {
            error!("Failed to subscribe to tasks: {}", e);
        }
        if let Err(e) = client.subscribe_notes(nats_tx.clone()).await {
            error!("Failed to subscribe to notes: {}", e);
        }
        if let Err(e) = client.subscribe_presence(nats_tx.clone()).await {
            error!("Failed to subscribe to presence: {}", e);
        }
        if let Err(e) = client.subscribe_own_messages(nats_tx).await {
            error!("Failed to subscribe to own messages: {}", e);
        }

        client.start_presence_heartbeat(
            "main".to_string(),
            "serving HTTP".to_string(),
        );

        state.set_nats_client(client).await;

        // Bridge NATS messages → SurrealDB persistence
        spawn_nats_persistence_bridge(&state, &project);

        // Bridge local events ↔ NATS for cross-process Indexer sync
        spawn_event_nats_publisher(&state, nats_raw.clone(), events_subject.clone(), true);
        spawn_event_nats_subscriber(&state, nats_raw, events_subject, true);

        // JetStream durable message consumer (Phase 2)
        // Ingests EventEnvelopes published by MCP subprocesses when HTTP was unreachable
        spawn_jetstream_message_consumer(&state, js_context);

        info!("NATS agent client active for HTTP server (subscriptions + heartbeat + persistence + event sync + JetStream ingester)");
    }

    // Start watcher dispatcher for observer agents (e.g. Laozi-Jung)
    let config = state.config().await;
    let _watcher_dispatcher =
        ming_qiao::watcher::WatcherDispatcher::start(&state, &config.watchers, &config.project).await;

    let server = HttpServer::new(state);
    info!("Starting HTTP server at http://{}", server.address());

    server.run().await?;
    Ok(())
}

/// Run the MCP server
async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    // All diagnostics use eprintln! (stderr) to keep stdout clean for JSON-RPC
    let agent_id = env::var("MING_QIAO_AGENT_ID").unwrap_or_else(|_| {
        eprintln!("[ming-qiao] MING_QIAO_AGENT_ID not set, using 'unknown'");
        "unknown".to_string()
    });

    let config_env_set = env::var("MING_QIAO_CONFIG").ok();
    let config_path = config_env_set
        .clone()
        .unwrap_or_else(|| "ming-qiao.toml".to_string());

    // Warn loudly if the config file doesn't exist — silent fallback to in-memory
    // DB is the #1 cause of "check_messages returns empty"
    if !std::path::Path::new(&config_path).exists() {
        let cwd = env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        eprintln!("[ming-qiao] WARNING: Config '{}' not found (CWD: {})", config_path, cwd);
        eprintln!("[ming-qiao] Using in-memory database — inbox reads will be empty");
        eprintln!("[ming-qiao] Set MING_QIAO_CONFIG=/path/to/ming-qiao.toml to fix");
    }

    let state = match AppState::load(&config_path).await {
        Ok(s) => {
            eprintln!("[ming-qiao] Loaded config from {}", config_path);
            s
        }
        Err(e) => {
            eprintln!("[ming-qiao] Config load failed ({}), using defaults", e);
            AppState::new().await
        }
    };

    state.ensure_dirs()?;
    eprintln!("[ming-qiao] State initialized, connecting NATS...");

    // Connect NATS agent client if enabled
    let config_snapshot = state.config().await;
    let nats_config = config_snapshot.nats;
    let project = config_snapshot.project;
    eprintln!("[ming-qiao] NATS config: enabled={}, url={}", nats_config.enabled, nats_config.url);
    if let Some(mut client) = NatsAgentClient::connect(&nats_config, &agent_id, &project).await {
        // Extract event sync parts before moving client into state
        let (nats_raw, events_subject) = client.event_sync_parts();

        let nats_tx = state.nats_message_sender();

        if let Err(e) = client.subscribe_own_tasks(nats_tx.clone()).await {
            eprintln!("[ming-qiao] NATS subscribe own tasks failed: {}", e);
        }
        if let Err(e) = client.subscribe_all_tasks(nats_tx.clone()).await {
            eprintln!("[ming-qiao] NATS subscribe all tasks failed: {}", e);
        }
        if let Err(e) = client.subscribe_notes(nats_tx.clone()).await {
            eprintln!("[ming-qiao] NATS subscribe notes failed: {}", e);
        }
        if let Err(e) = client.subscribe_presence(nats_tx.clone()).await {
            eprintln!("[ming-qiao] NATS subscribe presence failed: {}", e);
        }
        if let Err(e) = client.subscribe_own_messages(nats_tx).await {
            eprintln!("[ming-qiao] NATS subscribe own messages failed: {}", e);
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
        spawn_nats_persistence_bridge(&state, &project);

        // Bridge local events ↔ NATS for cross-process Indexer sync
        spawn_event_nats_publisher(&state, nats_raw.clone(), events_subject.clone(), false);
        spawn_event_nats_subscriber(&state, nats_raw, events_subject, false);

        eprintln!("[ming-qiao] NATS connected for agent '{}' (event sync active)", agent_id);
    } else {
        eprintln!("[ming-qiao] NATS not enabled or connection failed, running without NATS");
    }

    // Start watcher dispatcher for observer agents (e.g. Laozi-Jung)
    let config = state.config().await;
    let _watcher_dispatcher =
        ming_qiao::watcher::WatcherDispatcher::start(&state, &config.watchers, &config.project).await;

    let mut server = McpServer::with_state(agent_id, state.clone());
    server.run(&state).await?;
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

    // Initialize tracing (skip for mcp-serve — tracing pollutes stdout on some
    // platforms despite claiming stderr default; MCP diagnostics use eprintln!)
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
            eprintln!("[ming-qiao] FATAL: {}", e);
            error!("Server error: {}", e);
            ExitCode::FAILURE
        }
    }
}
