//! Council Observer — Autonomous polling daemon for the AstralMaris fleet
//!
//! Polls each agent's ming-qiao inbox every OBSERVER_POLL_INTERVAL seconds.
//! When unread messages are detected, fires a runtime-specific wake signal.
//!
//! Wake tiers:
//!   Tier 1 (AgentAPI):    POST to localhost:{port}/message (not currently used)
//!   Tier 2 (bare Enter):  cmux send-key enter (hypatia — triggers Gemini tool loop)
//!   Tier 3 (text inject): cmux send + enter (luban, ogma, laozi-jung, mataya, meridian, aleph)
//!
//! Replaces: astrallation/fleet/lib/council-observer.sh (333 lines)
//!
//! Usage:
//!   council-observer
//!   council-observer --poll-interval 30
//!   council-observer --dry-run

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Parser;
use serde::Deserialize;
use tokio::time::sleep;
use tracing::{debug, info, warn};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Parser)]
#[command(name = "council-observer", about = "Autonomous polling daemon for the AstralMaris fleet")]
struct Args {
    /// Poll interval in seconds (also settable via OBSERVER_POLL_INTERVAL env)
    #[arg(long, env = "OBSERVER_POLL_INTERVAL", default_value = "60")]
    poll_interval: u64,

    /// Ming-qiao base URL
    #[arg(long, env = "MINGQIAO_URL", default_value = "http://localhost:7777")]
    mingqiao_url: String,

    /// AgentAPI port (for Tier 1 wake)
    #[arg(long, env = "AGENTAPI_PORT", default_value = "3284")]
    agentapi_port: u16,

    /// cmux socket password (if set)
    #[arg(long, env = "CMUX_SOCKET_PASSWORD")]
    cmux_password: Option<String>,

    /// Log but don't actually wake agents
    #[arg(long)]
    dry_run: bool,
}

/// Timeout for all cmux subprocess calls — the key reliability fix
const CMUX_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for HTTP requests
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// Spinner/busy patterns for collision guard
const BUSY_PATTERNS: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
    "Thinking...", "Running...", "Generating", "Streaming", "▰▰",
];

// ============================================================================
// Fleet roster and wake tiers
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum WakeTier {
    /// POST to AgentAPI /message endpoint
    AgentApi,
    /// cmux send-key enter (triggers Gemini tool loop)
    BareEnter,
    /// cmux send text + enter
    TextInject,
}

#[derive(Debug, Clone)]
struct FleetAgent {
    name: &'static str,
    tier: WakeTier,
}

const FLEET_ROSTER: &[FleetAgent] = &[
    FleetAgent { name: "aleph", tier: WakeTier::TextInject },
    FleetAgent { name: "thales", tier: WakeTier::TextInject },
    FleetAgent { name: "hypatia", tier: WakeTier::TextInject },  // upgraded from BareEnter per Thales directive 2026-03-23
    FleetAgent { name: "luban", tier: WakeTier::TextInject },
    FleetAgent { name: "ogma", tier: WakeTier::TextInject },
    FleetAgent { name: "laozi-jung", tier: WakeTier::TextInject },
    FleetAgent { name: "mataya", tier: WakeTier::TextInject },
    // meridian removed — dormant, was causing "no cmux workspace found" every 60s
];

// ============================================================================
// Cooldown tracker
// ============================================================================

struct CooldownTracker {
    last_wake: HashMap<String, Instant>,
    poll_interval: Duration,
}

impl CooldownTracker {
    fn new(poll_interval: Duration) -> Self {
        Self {
            last_wake: HashMap::new(),
            poll_interval,
        }
    }

    fn is_cooling_down(&self, agent: &str) -> bool {
        self.last_wake
            .get(agent)
            .map(|t| t.elapsed() < self.poll_interval)
            .unwrap_or(false)
    }

    fn mark_woken(&mut self, agent: &str) {
        self.last_wake.insert(agent.to_string(), Instant::now());
    }
}

// ============================================================================
// Workspace cache
// ============================================================================

struct WorkspaceCache {
    refs: HashMap<String, String>,
}

impl WorkspaceCache {
    fn new() -> Self {
        Self {
            refs: HashMap::new(),
        }
    }

    /// Refresh workspace list from cmux. Called once per poll cycle.
    fn refresh(&mut self, cmux_password: &Option<String>) {
        self.refs.clear();

        let output = match run_cmux_with_timeout(cmux_password, &["--json", "list-workspaces"]) {
            Ok(output) => output,
            Err(e) => {
                warn!("cmux list-workspaces failed: {}", e);
                return;
            }
        };

        let data: serde_json::Value = match serde_json::from_str(&output) {
            Ok(v) => v,
            Err(e) => {
                warn!("cmux list-workspaces returned invalid JSON: {}", e);
                return;
            }
        };

        if let Some(workspaces) = data["workspaces"].as_array() {
            for ws in workspaces {
                if let (Some(title), Some(ws_ref)) =
                    (ws["title"].as_str(), ws["ref"].as_str())
                {
                    self.refs.insert(title.to_string(), ws_ref.to_string());
                    debug!("workspace {} -> {}", title, ws_ref);
                }
            }
        }
    }

    fn get(&self, agent: &str) -> Option<&str> {
        self.refs.get(agent).map(|s| s.as_str())
    }
}

// ============================================================================
// cmux wrapper with timeout
// ============================================================================

/// Run a cmux command with a 5-second timeout. Returns stdout on success.
/// On macOS there is no coreutils `timeout`, so we spawn and kill manually.
fn run_cmux_with_timeout(
    cmux_password: &Option<String>,
    args: &[&str],
) -> Result<String, String> {
    let mut cmd = Command::new("cmux");
    if let Some(pw) = cmux_password {
        cmd.arg("--password").arg(pw);
    }
    cmd.args(args);

    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn cmux: {}", e))?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                if status.success() {
                    return Ok(stdout);
                } else {
                    let stderr = child
                        .stderr
                        .take()
                        .map(|mut s| {
                            let mut buf = String::new();
                            std::io::Read::read_to_string(&mut s, &mut buf).ok();
                            buf
                        })
                        .unwrap_or_default();
                    return Err(format!(
                        "cmux exited with {}: {}",
                        status,
                        stderr.trim()
                    ));
                }
            }
            Ok(None) => {
                // Still running — check timeout
                if start.elapsed() > CMUX_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait(); // Reap zombie
                    return Err("cmux timed out (5s)".to_string());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(format!("wait error: {}", e));
            }
        }
    }
}

// ============================================================================
// Inbox check
// ============================================================================

#[derive(Debug, Deserialize)]
struct InboxResponse {
    #[serde(default)]
    unread_count: usize,
    #[serde(default)]
    messages: Vec<serde_json::Value>,
}

async fn check_inbox(
    http_client: &reqwest::Client,
    mingqiao_url: &str,
    agent: &str,
) -> usize {
    let url = format!("{}/api/inbox/{}", mingqiao_url, agent);
    match http_client.get(&url).timeout(HTTP_TIMEOUT).send().await {
        Ok(resp) => match resp.json::<InboxResponse>().await {
            Ok(inbox) => {
                // Prefer unread_count; fall back to messages length
                if inbox.unread_count > 0 {
                    inbox.unread_count
                } else {
                    inbox.messages.len()
                }
            }
            Err(e) => {
                debug!("inbox parse failed for {}: {}", agent, e);
                0
            }
        },
        Err(e) => {
            debug!("inbox check failed for {}: {}", agent, e);
            0
        }
    }
}

// ============================================================================
// Wake mechanisms
// ============================================================================

/// Wake result: Ok(true) = woke, Ok(false) = skipped (busy), Err = failure
type WakeResult = Result<bool, String>;

/// Tier 1: POST to AgentAPI /message endpoint
async fn wake_agentapi(
    http_client: &reqwest::Client,
    _agent: &str,
    port: u16,
) -> WakeResult {
    let url = format!("http://localhost:{}/message", port);
    let msg = "You have new messages. Call check_messages to read and respond.";

    match http_client
        .post(&url)
        .json(&serde_json::json!({ "content": msg, "type": "user" }))
        .timeout(Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => Ok(true),
        Ok(resp) => Err(format!("AgentAPI returned {}", resp.status())),
        Err(e) => Err(format!("AgentAPI unreachable: {}", e)),
    }
}

/// Tier 2: bare Enter key via cmux (Hypatia/Gemini)
fn wake_bare_enter(
    cmux_password: &Option<String>,
    ws_ref: &str,
) -> WakeResult {
    run_cmux_with_timeout(cmux_password, &["send-key", "--workspace", ws_ref, "enter"])
        .map(|_| true)
        .map_err(|e| format!("cmux send-key failed: {}", e))
}

/// Tier 3: text injection with collision guard
fn wake_text_inject(
    cmux_password: &Option<String>,
    ws_ref: &str,
) -> WakeResult {
    // Collision guard: read last 3 lines of terminal screen
    if let Ok(screen) =
        run_cmux_with_timeout(cmux_password, &["read-screen", "--workspace", ws_ref, "--lines", "3"])
    {
        for pattern in BUSY_PATTERNS {
            if screen.contains(pattern) {
                debug!(
                    ws_ref,
                    "appears busy — skipping wake (matched: {})", pattern
                );
                return Ok(false); // Skipped, not a failure
            }
        }
    }
    // screen read failure is non-fatal — proceed with wake

    let msg = "You have new messages. Call check_messages to read and respond.";

    // Send text
    run_cmux_with_timeout(cmux_password, &["send", "--workspace", ws_ref, msg])
        .map_err(|e| format!("cmux send failed: {}", e))?;

    // Send enter
    run_cmux_with_timeout(cmux_password, &["send-key", "--workspace", ws_ref, "enter"])
        .map_err(|e| format!("cmux send-key enter failed: {}", e))?;

    Ok(true)
}

// ============================================================================
// Poll cycle
// ============================================================================

async fn poll_cycle(
    http_client: &reqwest::Client,
    args: &Args,
    tracker: &mut CooldownTracker,
    ws_cache: &mut WorkspaceCache,
) {
    // Refresh workspace cache once per cycle
    ws_cache.refresh(&args.cmux_password);

    for fleet_agent in FLEET_ROSTER {
        let agent = fleet_agent.name;

        // Cooldown check
        if tracker.is_cooling_down(agent) {
            debug!(agent, "cooldown active — skipping");
            continue;
        }

        // Check inbox
        let unread = check_inbox(http_client, &args.mingqiao_url, agent).await;
        debug!(agent, unread, "inbox check");

        if unread == 0 {
            continue;
        }

        let tier_label = match fleet_agent.tier {
            WakeTier::AgentApi => "tier 1 (AgentAPI)",
            WakeTier::BareEnter => "tier 2 (bare enter)",
            WakeTier::TextInject => "tier 3 (text inject)",
        };
        info!(
            agent,
            unread, tier_label, "has unread messages — waking"
        );

        if args.dry_run {
            info!(agent, tier_label, "DRY-RUN: would wake");
            tracker.mark_woken(agent);
            continue;
        }

        // Dispatch wake by tier
        let result = match fleet_agent.tier {
            WakeTier::AgentApi => {
                wake_agentapi(http_client, agent, args.agentapi_port).await
            }
            WakeTier::BareEnter => {
                match ws_cache.get(agent) {
                    Some(ws_ref) => wake_bare_enter(&args.cmux_password, ws_ref),
                    None => {
                        warn!(agent, "no cmux workspace found — cannot wake");
                        continue;
                    }
                }
            }
            WakeTier::TextInject => {
                match ws_cache.get(agent) {
                    Some(ws_ref) => {
                        // Need owned string since ws_ref borrows ws_cache
                        let ws_ref_owned = ws_ref.to_string();
                        wake_text_inject(&args.cmux_password, &ws_ref_owned)
                    }
                    None => {
                        warn!(agent, "no cmux workspace found — cannot wake");
                        continue;
                    }
                }
            }
        };

        match result {
            Ok(true) => {
                info!(agent, "woke successfully");
                tracker.mark_woken(agent);
            }
            Ok(false) => {
                info!(agent, "busy — will retry next cycle");
            }
            Err(e) => {
                warn!(agent, error = %e, "wake failed — will retry next cycle");
            }
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
    let poll_interval = Duration::from_secs(args.poll_interval);

    info!("════════════════════════════════════════");
    info!("Council Observer (Rust)");
    info!("  Poll interval: {}s", args.poll_interval);
    info!("  Ming-qiao: {}", args.mingqiao_url);
    info!("  AgentAPI port: {} (for tier 1)", args.agentapi_port);
    info!("  Dry run: {}", args.dry_run);
    info!(
        "  Fleet: {}",
        FLEET_ROSTER
            .iter()
            .map(|a| a.name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    info!("════════════════════════════════════════");

    // Verify ming-qiao is reachable
    let http_client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?;

    match http_client
        .get(format!("{}/api/threads", args.mingqiao_url))
        .timeout(Duration::from_secs(3))
        .send()
        .await
    {
        Ok(_) => info!("Ming-qiao reachable"),
        Err(_) => warn!(
            "Ming-qiao not reachable at {} — will retry each cycle",
            args.mingqiao_url
        ),
    }

    let mut tracker = CooldownTracker::new(poll_interval);
    let mut ws_cache = WorkspaceCache::new();

    loop {
        debug!("--- poll cycle start ---");
        poll_cycle(&http_client, &args, &mut tracker, &mut ws_cache).await;
        sleep(poll_interval).await;
    }
}
