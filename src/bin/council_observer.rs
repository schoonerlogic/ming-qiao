//! Council Observer — Autonomous polling daemon for the AstralMaris fleet
//!
//! Polls each agent's ming-qiao inbox every OBSERVER_POLL_INTERVAL seconds.
//! When unread messages are detected, fires a runtime-specific wake signal.
//!
//! Wake tiers:
//!   Tier 1 (AgentAPI):    POST to localhost:{port}/message (not currently used)
//!   Tier 2 (bare Enter):  cmux send-key enter
//!   Tier 3 (text inject): cmux send + enter (all agents)
//!
//! Auto-heal: after 5 consecutive cycles where an agent has unread messages
//! that never clear, the Observer kills and relaunches the cmux session.
//! This handles stale MCP sessions (e.g., Gemini CLI after ming-qiao restart).
//!
//! Replaces: astrallation/fleet/lib/council-observer.sh (333 lines)
//!
//! Usage:
//!   council-observer
//!   council-observer --poll-interval 30
//!   council-observer --dry-run

use std::collections::HashMap;
use std::path::PathBuf;
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

    /// Ming-qiao auth token for sending alerts (or set MQ_TOKEN env var)
    #[arg(long, env = "MQ_TOKEN")]
    mq_token: Option<String>,

    /// Agent tokens file (auto-loads token for 'council-observer' or 'jikimi')
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/agent-tokens.json")]
    tokens_file: PathBuf,

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

/// Maximum backoff interval (15 minutes)
const MAX_BACKOFF: Duration = Duration::from_secs(900);

/// After this many consecutive stale cycles, stop waking entirely (circuit breaker)
const CIRCUIT_BREAKER_THRESHOLD: u32 = 5;

struct CooldownTracker {
    last_wake: HashMap<String, Instant>,
    /// Per-agent backoff multiplier (doubles on each consecutive unresponsive cycle)
    backoff_level: HashMap<String, u32>,
    base_interval: Duration,
}

impl CooldownTracker {
    fn new(poll_interval: Duration) -> Self {
        Self {
            last_wake: HashMap::new(),
            backoff_level: HashMap::new(),
            base_interval: poll_interval,
        }
    }

    fn cooldown_for(&self, agent: &str) -> Duration {
        let level = self.backoff_level.get(agent).copied().unwrap_or(0);
        let multiplier = 1u64 << level.min(6); // cap at 2^6 = 64x
        let backoff = self.base_interval * multiplier as u32;
        if backoff > MAX_BACKOFF { MAX_BACKOFF } else { backoff }
    }

    fn is_cooling_down(&self, agent: &str) -> bool {
        self.last_wake
            .get(agent)
            .map(|t| t.elapsed() < self.cooldown_for(agent))
            .unwrap_or(false)
    }

    fn mark_woken(&mut self, agent: &str) {
        self.last_wake.insert(agent.to_string(), Instant::now());
    }

    /// Agent responded — reset backoff to zero
    fn reset_backoff(&mut self, agent: &str) {
        self.backoff_level.remove(agent);
    }

    /// Agent didn't respond — increase backoff
    fn increase_backoff(&mut self, agent: &str) {
        let level = self.backoff_level.entry(agent.to_string()).or_insert(0);
        *level += 1;
        let current_level = *level;
        let multiplier = 1u64 << current_level.min(6);
        let backoff = self.base_interval * multiplier as u32;
        let cd = if backoff > MAX_BACKOFF { MAX_BACKOFF } else { backoff };
        info!(agent, backoff_level = current_level, cooldown_secs = cd.as_secs(),
            "backoff increased — next wake in {}s", cd.as_secs());
    }
}

// ============================================================================
// Stale session tracker — two-phase auto-heal for broken MCP sessions
// ============================================================================

/// Phase 1: after this many consecutive stale cycles, send graceful shutdown signal
const GRACEFUL_THRESHOLD: u32 = 5;
/// Phase 2: after this many total consecutive stale cycles, force kill + relaunch
const FORCE_THRESHOLD: u32 = 7;

#[derive(Debug, Clone, PartialEq)]
enum HealPhase {
    /// Not stale yet
    Healthy,
    /// Graceful signal sent, waiting for agent to exit
    GracefulPending,
    /// Force kill + relaunch executed
    ForceHealed,
}

struct StaleSessionTracker {
    consecutive_unread: HashMap<String, u32>,
}

impl StaleSessionTracker {
    fn new() -> Self {
        Self {
            consecutive_unread: HashMap::new(),
        }
    }

    /// Record that an agent has unread messages this cycle.
    /// Returns the appropriate heal phase.
    fn record_unread(&mut self, agent: &str) -> HealPhase {
        let count = self.consecutive_unread.entry(agent.to_string()).or_insert(0);
        *count += 1;
        if *count >= FORCE_THRESHOLD {
            HealPhase::ForceHealed
        } else if *count >= GRACEFUL_THRESHOLD {
            HealPhase::GracefulPending
        } else {
            HealPhase::Healthy
        }
    }

    /// Agent cleared their inbox — reset counter.
    fn record_clear(&mut self, agent: &str) {
        self.consecutive_unread.remove(agent);
    }

    /// Reset after a force heal so we give the new session time to start.
    fn reset_after_heal(&mut self, agent: &str) {
        self.consecutive_unread.remove(agent);
    }

    fn consecutive_count(&self, agent: &str) -> u32 {
        self.consecutive_unread.get(agent).copied().unwrap_or(0)
    }
}

/// Phase 1: Graceful — inject text telling the agent to save state and exit.
fn graceful_heal_signal(
    cmux_password: &Option<String>,
    agent: &str,
    ws_ref: &str,
    dry_run: bool,
) -> Result<(), String> {
    let msg = format!(
        "OBSERVER AUTO-HEAL: Your MCP session appears broken ({} has had unread messages for 5+ minutes). \
         Please save your work, write your handoff file via 'am-fleet handoff {}', then exit. \
         You will be restarted automatically. If you do not exit within 2 minutes, your session will be force-killed.",
        agent, agent
    );

    if dry_run {
        info!(agent, "DRY-RUN: would send graceful heal signal");
        return Ok(());
    }

    warn!(agent, "AUTO-HEAL PHASE 1: sending graceful shutdown signal");

    // Inject the message
    run_cmux_with_timeout(cmux_password, &["send", "--workspace", ws_ref, &msg])
        .map_err(|e| format!("cmux send failed: {}", e))?;
    run_cmux_with_timeout(cmux_password, &["send-key", "--workspace", ws_ref, "enter"])
        .map_err(|e| format!("cmux send-key enter failed: {}", e))?;

    info!(agent, "AUTO-HEAL PHASE 1: graceful signal sent — waiting 2 cycles for agent to exit");
    Ok(())
}

/// Resolve the terminal surface ref for a workspace.
/// Runs: cmux list-pane-surfaces --workspace <ref> --id-format both
/// and picks the first [terminal] surface.
fn resolve_terminal_surface(
    cmux_password: &Option<String>,
    ws_ref: &str,
) -> Option<String> {
    let output = run_cmux_with_timeout(
        cmux_password,
        &["list-pane-surfaces", "--workspace", ws_ref, "--id-format", "both"],
    ).ok()?;

    // Parse output: lines like "* surface:76  Kimi Code  [selected]"
    // or "  surface:70  OC | Luban agent...  [selected]"
    for line in output.lines() {
        let trimmed = line.trim().trim_start_matches('*').trim();
        if trimmed.starts_with("surface:") {
            if let Some(ref_part) = trimmed.split_whitespace().next() {
                return Some(ref_part.to_string());
            }
        }
    }
    None
}

/// Phase 2: Force restart — respawn the terminal surface in the existing workspace.
/// Fallback: am-fleet agent <name> restart (close + relaunch).
fn force_heal_session(
    cmux_password: &Option<String>,
    agent: &str,
    ws_ref: &str,
    dry_run: bool,
) -> Result<(), String> {
    if dry_run {
        info!(agent, "DRY-RUN: would force-heal (respawn terminal surface in workspace)");
        return Ok(());
    }

    warn!(agent, "AUTO-HEAL PHASE 2: respawning stale session (cmux workspace {})", ws_ref);

    // Step 1: Resolve the terminal surface ref (avoids "Surface is not a terminal" error)
    let surface_ref = resolve_terminal_surface(cmux_password, ws_ref);

    // Step 2: respawn-pane with explicit --surface targeting the terminal
    let launch_script = format!(
        "/Users/proteus/astralmaris/astrallation/fleet/launch-{}.sh",
        agent
    );

    let respawn_result = if let Some(ref sref) = surface_ref {
        info!(agent, surface = %sref, "targeting terminal surface for respawn");
        run_cmux_with_timeout(cmux_password, &[
            "respawn-pane", "--workspace", ws_ref, "--surface", sref, "--command", &launch_script,
        ])
    } else {
        warn!(agent, "could not resolve terminal surface — trying respawn without --surface");
        run_cmux_with_timeout(cmux_password, &[
            "respawn-pane", "--workspace", ws_ref, "--command", &launch_script,
        ])
    };

    match respawn_result {
        Ok(_) => {
            info!(agent, "AUTO-HEAL PHASE 2: respawn-pane succeeded — new session starting");
            return Ok(());
        }
        Err(e) => {
            warn!(agent, error = %e, "respawn-pane failed — falling back to am-fleet agent restart");
        }
    }

    // Fallback: am-fleet agent <name> restart (does close-workspace + full relaunch)
    info!(agent, "AUTO-HEAL PHASE 2: falling back to am-fleet agent {} restart", agent);
    let fleet_script = "/Users/proteus/astralmaris/astrallation/fleet/am-fleet.sh";
    match Command::new("bash")
        .args([fleet_script, "agent", agent, "restart"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success() {
                info!(agent, "AUTO-HEAL PHASE 2: am-fleet agent restart succeeded");
                if !stdout.is_empty() { info!(agent, stdout = %stdout.trim(), "am-fleet output"); }
                Ok(())
            } else {
                warn!(agent, code = output.status.code(), stdout = %stdout.trim(), stderr = %stderr.trim(),
                    "am-fleet agent restart failed");
                Err(format!("am-fleet agent {} restart failed (exit {}): {}", agent, output.status, stderr.trim()))
            }
        }
        Err(e) => Err(format!("failed to run am-fleet agent {} restart: {}", agent, e)),
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
// Stale session alert — notify merlin (Proteus) via ming-qiao
// ============================================================================

async fn alert_merlin(
    http_client: &reqwest::Client,
    mingqiao_url: &str,
    agent: &str,
    consecutive: u32,
    unread: usize,
    mq_token: &Option<String>,
) {
    let body = serde_json::json!({
        "from_agent": "jikimi",
        "to_agent": "merlin",
        "subject": format!("am.observer.stale-session — {}", agent),
        "content": format!(
            "STALE SESSION ALERT: **{}** has had {} unread message(s) for {} consecutive poll cycles (~{} minutes) without clearing them.\n\n\
             The agent's session may be broken (stale MCP connection, hung process, or unresponsive runtime).\n\n\
             **Action required:** Check the agent's cmux workspace and restart if needed.\n\n\
             — council-observer (automated)",
            agent, unread, consecutive, consecutive
        ),
        "intent": "request",
        "priority": "high"
    });

    let url = format!("{}/api/threads", mingqiao_url);
    let mut req = http_client
        .post(&url)
        .json(&body)
        .timeout(HTTP_TIMEOUT);

    if let Some(token) = mq_token {
        req = req.header("Authorization", format!("Bearer {}", token));
    }

    match req.send().await
    {
        Ok(resp) if resp.status().is_success() => {
            info!(agent, "stale session alert sent to merlin");
        }
        Ok(resp) => {
            warn!(agent, "failed to alert merlin (HTTP {})", resp.status());
        }
        Err(e) => {
            warn!(agent, error = %e, "failed to alert merlin");
        }
    }
}

// ============================================================================
// Poll cycle
// ============================================================================

async fn poll_cycle(
    http_client: &reqwest::Client,
    args: &Args,
    tracker: &mut CooldownTracker,
    ws_cache: &mut WorkspaceCache,
    stale_tracker: &mut StaleSessionTracker,
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
            stale_tracker.record_clear(agent);
            tracker.reset_backoff(agent);
            continue;
        }

        // Track consecutive unread cycles — circuit breaker + alert
        let heal_phase = stale_tracker.record_unread(agent);
        let consecutive = stale_tracker.consecutive_count(agent);

        // Circuit breaker: after threshold, STOP waking — just alert and back off
        if consecutive >= CIRCUIT_BREAKER_THRESHOLD {
            match heal_phase {
                HealPhase::GracefulPending if consecutive == GRACEFUL_THRESHOLD => {
                    // First time hitting circuit breaker — alert merlin
                    warn!(agent, consecutive, unread,
                        "CIRCUIT BREAKER: {} consecutive stale cycles — STOPPING wake attempts, alerting merlin",
                        consecutive);
                    alert_merlin(http_client, &args.mingqiao_url, agent, consecutive, unread, &args.mq_token).await;
                }
                HealPhase::ForceHealed => {
                    // Re-alert merlin periodically
                    warn!(agent, consecutive, unread,
                        "CIRCUIT BREAKER: still stale after {} cycles — re-alerting merlin", consecutive);
                    alert_merlin(http_client, &args.mingqiao_url, agent, consecutive, unread, &args.mq_token).await;
                    stale_tracker.reset_after_heal(agent);
                }
                _ => {
                    debug!(agent, consecutive, "circuit breaker active — not waking");
                }
            }
            tracker.increase_backoff(agent);
            tracker.mark_woken(agent); // apply backoff cooldown
            continue; // DO NOT wake — session is dead
        }

        // Below circuit breaker threshold — proceed with wake
        match heal_phase {
            HealPhase::Healthy => {
                // Not stale yet — normal wake
            }
            _ => {
                // Should not reach here (circuit breaker catches >= GRACEFUL_THRESHOLD)
                // but handle gracefully
            }
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

    let mut args = Args::parse();
    let poll_interval = Duration::from_secs(args.poll_interval);

    // Auto-load MQ token from tokens file (try jikimi, then merlin)
    if args.mq_token.is_none() {
        if let Ok(content) = std::fs::read_to_string(&args.tokens_file) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(t) = data["tokens"]["jikimi"].as_str() {
                    args.mq_token = Some(t.to_string());
                    info!("Loaded MQ token for 'jikimi' from {}", args.tokens_file.display());
                } else if let Some(t) = data["tokens"]["merlin"].as_str() {
                    args.mq_token = Some(t.to_string());
                    info!("Loaded MQ token for 'merlin' from {}", args.tokens_file.display());
                }
            }
        }
        if args.mq_token.is_none() {
            warn!("No MQ token found — alert_merlin calls will fail (401). Set --mq-token or MQ_TOKEN env.");
        }
    }

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
    let mut stale_tracker = StaleSessionTracker::new();

    info!("  Auto-heal: graceful at {} cycles, force at {} cycles", GRACEFUL_THRESHOLD, FORCE_THRESHOLD);

    loop {
        debug!("--- poll cycle start ---");
        poll_cycle(&http_client, &args, &mut tracker, &mut ws_cache, &mut stale_tracker).await;
        sleep(poll_interval).await;
    }
}
