//! Wake Daemon — JetStream subscriber → AgentAPI POST
//!
//! Subscribes to NATS JetStream `am.msg.*` subjects. When a message arrives
//! for an agent, POSTs a wake notification to their AgentAPI endpoint.
//!
//! This replaces the fswatch notification file watcher with a proper
//! JetStream subscriber — same universal delivery path.
//!
//! Usage:
//!   wake-daemon                    # Run with defaults
//!   wake-daemon --dry-run          # Log but don't POST
//!   wake-daemon --manifest /path   # Custom manifest path

use std::collections::HashMap;
use std::time::{Duration, Instant};

use clap::Parser;
use chrono::Utc;
use serde::Deserialize;
use tracing::{error, info, warn};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Parser)]
#[command(name = "wake-daemon", about = "JetStream → AgentAPI wake notifications")]
struct Args {
    /// Fleet manifest path
    #[arg(long, default_value = "/Users/proteus/astralmaris/astrallation/fleet/fleet-manifest.toml")]
    manifest: String,

    /// NATS server URL
    #[arg(long, default_value = "nats://localhost:4222")]
    nats_url: String,

    /// NKey seed file for NATS auth
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/nkeys/aleph.nk")]
    nkey_file: String,

    /// Log but don't POST to AgentAPI
    #[arg(long)]
    dry_run: bool,
}

const WAKE_COOLDOWN: Duration = Duration::from_secs(30);
const AGENTAPI_TIMEOUT: Duration = Duration::from_secs(5);

// ============================================================================
// Fleet manifest (minimal parser)
// ============================================================================

#[derive(Debug, Deserialize)]
struct FleetManifest {
    agents: HashMap<String, ManifestAgent>,
}

#[derive(Debug, Deserialize)]
struct ManifestAgent {
    wake_port: Option<u16>,
    runtime: Option<String>,
    skip_session_launch: Option<bool>,
}

fn load_manifest(path: &str) -> Option<FleetManifest> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

// ============================================================================
// Wake state tracking
// ============================================================================

struct WakeTracker {
    last_wake: HashMap<String, Instant>,
}

impl WakeTracker {
    fn new() -> Self {
        Self {
            last_wake: HashMap::new(),
        }
    }

    fn should_wake(&self, agent: &str) -> bool {
        self.last_wake
            .get(agent)
            .map(|t| t.elapsed() > WAKE_COOLDOWN)
            .unwrap_or(true)
    }

    fn mark_woken(&mut self, agent: &str) {
        self.last_wake.insert(agent.to_string(), Instant::now());
    }
}

// ============================================================================
// AgentAPI wake
// ============================================================================

async fn wake_agent(
    http_client: &reqwest::Client,
    agent: &str,
    port: u16,
    from: &str,
    subject: &str,
    dry_run: bool,
) -> bool {
    let url = format!("http://localhost:{}/message", port);
    let message = format!(
        "You have a new message from {}: {}. Call check_messages to read and respond.",
        from, subject
    );

    if dry_run {
        info!("DRY-RUN: would POST to {} for {}: {}", url, agent, message);
        return true;
    }

    match http_client
        .post(&url)
        .json(&serde_json::json!({ "content": message, "type": "user" }))
        .timeout(AGENTAPI_TIMEOUT)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            info!("WAKE: {} (port {}) ← {} re: {}", agent, port, from, subject);
            true
        }
        Ok(resp) => {
            warn!("WAKE FAILED: {} returned {}", agent, resp.status());
            false
        }
        Err(e) => {
            // Agent not running or AgentAPI not available — expected for offline agents
            info!("WAKE SKIP: {} (port {}) — {}", agent, port, e);
            false
        }
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let manifest = load_manifest(&args.manifest)
        .ok_or_else(|| anyhow::anyhow!("Failed to load manifest from {}", args.manifest))?;

    // Build agent → port mapping
    let agent_ports: HashMap<String, u16> = manifest
        .agents
        .iter()
        .filter(|(_, a)| {
            a.wake_port.is_some() && !a.skip_session_launch.unwrap_or(false)
        })
        .map(|(name, a)| (name.clone(), a.wake_port.unwrap()))
        .collect();

    info!("════════════════════════════════════════");
    info!("Wake Daemon");
    info!("  Manifest: {}", args.manifest);
    info!("  NATS: {}", args.nats_url);
    info!("  Dry run: {}", args.dry_run);
    info!("  Agents: {:?}", agent_ports);
    info!("════════════════════════════════════════");

    // Connect to NATS with NKey auth
    let nkey_seed = std::fs::read_to_string(&args.nkey_file)
        .map_err(|e| anyhow::anyhow!("Failed to read NKey file {}: {}", args.nkey_file, e))?;

    let client = async_nats::ConnectOptions::with_nkey(nkey_seed.trim().to_string())
        .name("wake-daemon")
        .connect(&args.nats_url)
        .await
        .map_err(|e| anyhow::anyhow!("NATS connect failed: {}", e))?;

    info!("NATS connected");

    // Get JetStream context
    let js = async_nats::jetstream::new(client);

    // Get the AGENT_MESSAGES stream
    let stream = js
        .get_stream("AGENT_MESSAGES")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get AGENT_MESSAGES stream: {}", e))?;

    // Create or get a durable consumer for the wake daemon
    let consumer_config = async_nats::jetstream::consumer::pull::Config {
        durable_name: Some("wake-daemon".to_string()),
        filter_subject: "am.msg.>".to_string(),
        ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
        deliver_policy: async_nats::jetstream::consumer::DeliverPolicy::New,
        ..Default::default()
    };

    let consumer = stream
        .get_or_create_consumer("wake-daemon", consumer_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create consumer: {}", e))?;

    let mut messages = consumer
        .messages()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start consuming: {}", e))?;

    info!("JetStream consumer active (wake-daemon on am.msg.>)");

    let http_client = reqwest::Client::builder()
        .timeout(AGENTAPI_TIMEOUT)
        .build()?;

    let mut tracker = WakeTracker::new();

    use futures_util::StreamExt;
    while let Some(msg_result) = messages.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                warn!("JetStream error: {}", e);
                continue;
            }
        };

        // Parse the event envelope to extract recipient and sender
        let (to_agent, from_agent, subject) = match serde_json::from_slice::<serde_json::Value>(&msg.payload) {
            Ok(event) => {
                // Event structure: {"payload":{"type":"message","data":{"from":...,"to":...}}}
                let data = &event["payload"]["data"];
                let to = data["to"].as_str().unwrap_or("").to_string();
                let from = data["from"].as_str().unwrap_or("").to_string();
                let subj = data["subject"].as_str().unwrap_or("").to_string();
                (to, from, subj)
            }
            Err(e) => {
                warn!("Failed to parse event: {}", e);
                if let Err(e) = msg.ack().await {
                    warn!("Failed to ack bad message: {}", e);
                }
                continue;
            }
        };

        // Only wake for request/discuss intents
        let intent = serde_json::from_slice::<serde_json::Value>(&msg.payload)
            .ok()
            .and_then(|e| e["payload"]["data"]["intent"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();

        let is_wakeable = matches!(intent.as_str(), "Request" | "Discuss" | "request" | "discuss");

        // Ack the message (we've seen it regardless)
        if let Err(e) = msg.ack().await {
            warn!("Failed to ack: {}", e);
        }

        // Only wake for wakeable intents to matching agents
        if !is_wakeable {
            continue;
        }

        if let Some(&port) = agent_ports.get(&to_agent) {
            if tracker.should_wake(&to_agent) {
                wake_agent(&http_client, &to_agent, port, &from_agent, &subject, args.dry_run).await;
                tracker.mark_woken(&to_agent);
            } else {
                info!("COOLDOWN: {} — skipping wake", to_agent);
            }
        }
    }

    info!("Wake daemon ended");
    Ok(())
}
