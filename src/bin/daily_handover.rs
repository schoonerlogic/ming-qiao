//! Daily Watch Handover Script — Merlin-operated morning initialization
//!
//! A manual CLI tool for Proteus/Merlin to run each morning. NOT a daemon.
//!
//! Workflow:
//!   Phase 1: Verify Observer daemon health; restart if dead/hung
//!   Phase 2: Broadcast fleet warning — commit work, update handoff files
//!   Phase 3: Wait for agent readiness ACKs (configurable timeout)
//!   Phase 4: Interactive signoff — show readiness dashboard, execute on confirmation
//!
//! Usage:
//!   daily-handover                     # Full workflow (from_agent defaults to thales)
//!   daily-handover --check-only        # Phase 1 only (observer + jikimi health)
//!   daily-handover --timeout 180       # Custom ACK wait timeout (seconds)
//!   daily-handover --skip-restart      # Skip the restart trigger in Phase 4
//!   daily-handover --dry-run           # Print what would happen, don't execute
//!   daily-handover --from-agent merlin # Override sender identity

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use serde::Deserialize;
use tokio::time::sleep;
use tracing::{error, info, warn};

// ============================================================================
// CLI arguments
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "daily-handover", about = "Daily Watch Handover — morning fleet initialization")]
struct Args {
    /// Fleet manifest path
    #[arg(long, default_value = "/Users/proteus/astralmaris/astrallation/fleet/fleet-manifest.toml")]
    manifest: PathBuf,

    /// Ming-Qiao API base URL
    #[arg(long, default_value = "http://localhost:7777")]
    api_url: String,

    /// Bearer token for ming-qiao API (or set MQ_TOKEN env var)
    #[arg(long)]
    token: Option<String>,

    /// Agent identity for sending messages
    #[arg(long, default_value = "thales")]
    from_agent: String,

    /// Agent tokens file (auto-loads token for from_agent)
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/agent-tokens.json")]
    tokens_file: PathBuf,

    /// Timeout in seconds to wait for agent ACKs (Phase 3)
    #[arg(long, default_value_t = 120)]
    timeout: u64,

    /// Only check observer health (Phase 1), then exit
    #[arg(long)]
    check_only: bool,

    /// Skip restart trigger in Phase 4
    #[arg(long)]
    skip_restart: bool,

    /// Print what would happen without executing
    #[arg(long)]
    dry_run: bool,

    /// Handoff state directory
    #[arg(long, default_value = "/Users/proteus/astralmaris/astrallation/fleet/state")]
    state_dir: PathBuf,
}

// ============================================================================
// Fleet manifest (minimal parser)
// ============================================================================

#[derive(Debug, Deserialize)]
struct FleetManifest {
    agents: HashMap<String, ManifestAgent>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ManifestAgent {
    role: Option<String>,
    runtime: Option<String>,
    skip_session_launch: Option<bool>,
}

fn load_manifest(path: &PathBuf) -> Result<FleetManifest> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read manifest: {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse manifest: {}", path.display()))
}

/// Fleet roster (per Thales directive 2026-03-23)
/// Core: session agents that must ACK for readiness
/// Optional: restart only if paired core agent also restarts
/// Excluded: merlin (human), meridian (dormant), jikimi (daemon — health-checked separately)
struct FleetRoster {
    core: Vec<String>,
    optional: Vec<String>,
}

fn active_agents(_manifest: &FleetManifest) -> FleetRoster {
    FleetRoster {
        core: vec![
            "aleph".to_string(),
            "thales".to_string(),
            "hypatia".to_string(),
            "luban".to_string(),
            "ogma".to_string(),
            "mataya".to_string(),
            "laozi-jung".to_string(),
        ],
        optional: vec![
            "hypatia-adjutant".to_string(), // Gemini Flash, restart only with hypatia
        ],
    }
}

/// All agents (core + optional) for broadcasting
fn all_agents(roster: &FleetRoster) -> Vec<String> {
    let mut all = roster.core.clone();
    all.extend(roster.optional.clone());
    all
}

// ============================================================================
// Jikimi daemon health check (not a session agent — launchd daemons only)
// ============================================================================

const JIKIMI_DAEMONS: &[&str] = &[
    "com.astralmaris.jikimi-healthd",
    "com.astralmaris.jikimi-watchdog",
    "com.astralmaris.jikimi-synthetic-test",
];

fn check_jikimi_health() {
    println!("  Jikimi daemons (health check only — not a session agent):");
    for label in JIKIMI_DAEMONS {
        let output = Command::new("launchctl")
            .args(["list", label])
            .output();
        let status = match output {
            Ok(out) if out.status.success() => "RUNNING",
            _ => "STOPPED",
        };
        let marker = if status == "RUNNING" { "+" } else { "!" };
        println!("    [{}] {} {}", marker, label, status);
    }
    println!();
}

// ============================================================================
// Phase 1: Observer health check
// ============================================================================

#[derive(Debug, PartialEq)]
enum ObserverStatus {
    Running,
    Stopped,
    Hung,
}

fn check_observer_health() -> ObserverStatus {
    // Check launchctl for the service
    let output = Command::new("launchctl")
        .args(["list", "com.astralmaris.council-observer"])
        .output();

    match output {
        Ok(out) => {
            if !out.status.success() {
                return ObserverStatus::Stopped;
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Parse PID from launchctl output
            if stdout.contains("\"PID\"") {
                // Check if the process is responsive by reading the log
                // Look for recent log entries (within last 5 minutes)
                let log_path = "/Users/proteus/astralmaris/ming-qiao/logs/observer.log";
                if let Ok(metadata) = std::fs::metadata(log_path) {
                    if let Ok(modified) = metadata.modified() {
                        let age = modified.elapsed().unwrap_or(Duration::from_secs(u64::MAX));
                        if age > Duration::from_secs(300) {
                            return ObserverStatus::Hung;
                        }
                    }
                }
                ObserverStatus::Running
            } else {
                ObserverStatus::Stopped
            }
        }
        Err(_) => ObserverStatus::Stopped,
    }
}

fn restart_observer(dry_run: bool) -> Result<()> {
    let label = "com.astralmaris.council-observer";
    let plist = "/Users/proteus/astralmaris/astrallation/fleet/lib/com.astralmaris.council-observer.plist";

    if dry_run {
        info!("[DRY RUN] Would restart observer: bootout + bootstrap");
        return Ok(());
    }

    // Stop existing
    info!("Stopping observer daemon...");
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("gui/{}", get_current_uid()), label])
        .output();

    // Brief pause
    std::thread::sleep(Duration::from_secs(2));

    // Start fresh
    info!("Starting observer daemon...");
    let output = Command::new("launchctl")
        .args(["bootstrap", &format!("gui/{}", get_current_uid()), plist])
        .output()
        .context("Failed to bootstrap observer")?;

    if output.status.success() {
        info!("Observer restarted successfully");
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Error 37 = "already loaded" — that's fine
        if stderr.contains("37:") || stderr.contains("Already loaded") {
            info!("Observer already loaded, sending SIGHUP to reload");
            let _ = Command::new("pkill")
                .args(["-HUP", "-f", "council-observer"])
                .output();
        } else {
            error!("Observer restart failed: {}", stderr);
            anyhow::bail!("Observer restart failed");
        }
    }

    Ok(())
}

// ============================================================================
// Phase 2: Broadcast fleet warning
// ============================================================================

async fn broadcast_warning(
    agents: &[String],
    http_client: &reqwest::Client,
    args: &Args,
) -> Result<u32> {
    let mut sent = 0u32;

    let content = "DAILY WATCH HANDOVER — Merlin morning initialization.

Fleet restart is imminent. Please:
1. Commit any in-progress work.
2. Update your memory files.
3. Write your handoff payload via 'am-fleet handoff <your-agent-id>'.
4. Reply to this message with 'READY' when complete.

Timeout: you have {} seconds before forced restart.

— daily-handover (Merlin-operated)";

    let content = content.replace("{}", &args.timeout.to_string());

    for agent in agents {
        if args.dry_run {
            info!("[DRY RUN] Would send warning to {}", agent);
            sent += 1;
            continue;
        }

        let body = serde_json::json!({
            "from_agent": args.from_agent,
            "to_agent": agent,
            "subject": "DAILY HANDOVER — Commit work and submit handoff",
            "intent": "request",
            "content": content,
            "priority": "critical"
        });

        let mut req = http_client
            .post(format!("{}/api/threads", args.api_url))
            .header("Content-Type", "application/json");

        if let Some(token) = &args.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        match req.json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!("Warning sent to {}", agent);
                sent += 1;
            }
            Ok(resp) => {
                warn!("Failed to send warning to {} (HTTP {})", agent, resp.status());
            }
            Err(e) => {
                warn!("Failed to send warning to {}: {}", agent, e);
            }
        }
    }

    Ok(sent)
}

// ============================================================================
// Phase 3: Wait for readiness ACKs
// ============================================================================

async fn wait_for_readiness(
    agents: &[String],
    http_client: &reqwest::Client,
    args: &Args,
    broadcast_time: chrono::DateTime<Utc>,
) -> HashMap<String, bool> {
    let mut readiness: HashMap<String, bool> = agents.iter().map(|a| (a.clone(), false)).collect();

    if args.dry_run {
        info!("[DRY RUN] Would wait {}s for ACKs from {} agents", args.timeout, agents.len());
        return readiness;
    }

    let deadline = Instant::now() + Duration::from_secs(args.timeout);
    let poll_interval = Duration::from_secs(5);

    println!();
    println!("Waiting for agent readiness (timeout: {}s)...", args.timeout);
    println!();

    while Instant::now() < deadline {
        let mut all_ready = true;

        for agent in agents {
            if *readiness.get(agent.as_str()).unwrap_or(&false) {
                continue;
            }

            // Check for handoff file freshness
            let handoff_file = args.state_dir.join(format!("last-handoff-{}.json", agent));
            if let Ok(content) = std::fs::read_to_string(&handoff_file) {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(epoch) = data.get("last_updated_epoch").and_then(|v| v.as_i64()) {
                        let file_time = chrono::DateTime::from_timestamp(epoch, 0);
                        if let Some(ft) = file_time {
                            if ft > broadcast_time {
                                info!("{}: handoff received (epoch {})", agent, epoch);
                                readiness.insert(agent.clone(), true);
                                continue;
                            }
                        }
                    }
                }
            }

            // Also check for "READY" reply in merlin's inbox
            let inbox_url = format!(
                "{}/api/inbox/{}?unread_only=true&limit=20",
                args.api_url, args.from_agent
            );
            let mut req = http_client.get(&inbox_url);
            if let Some(token) = &args.token {
                req = req.header("Authorization", format!("Bearer {}", token));
            }

            if let Ok(resp) = req.send().await {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(msgs) = body["messages"].as_array() {
                        for msg in msgs {
                            if msg["from"].as_str() == Some(agent.as_str()) {
                                let content = msg["content"].as_str().unwrap_or("");
                                if content.to_uppercase().contains("READY")
                                    || content.to_uppercase().contains("ACK")
                                    || content.to_uppercase().contains("HANDOFF")
                                {
                                    info!("{}: readiness ACK received", agent);
                                    readiness.insert(agent.clone(), true);
                                }
                            }
                        }
                    }
                }
            }

            if !readiness.get(agent.as_str()).unwrap_or(&false) {
                all_ready = false;
            }
        }

        let ready_count = readiness.values().filter(|v| **v).count();
        let remaining = deadline.duration_since(Instant::now()).as_secs();
        print!("\r  {}/{} ready ({:>3}s remaining)", ready_count, agents.len(), remaining);
        let _ = io::stdout().flush();

        if all_ready {
            println!();
            info!("All agents ready!");
            return readiness;
        }

        sleep(poll_interval).await;
    }

    println!();
    warn!("Timeout reached — not all agents responded");
    readiness
}

// ============================================================================
// Phase 4: Interactive signoff
// ============================================================================

fn display_readiness_dashboard(roster: &FleetRoster, readiness: &HashMap<String, bool>) {
    println!();
    println!("════════════════════════════════════════");
    println!("  Daily Watch Handover — Readiness");
    println!("════════════════════════════════════════");
    println!();

    let mut core_ready = 0;
    println!("  Core agents:");
    for agent in &roster.core {
        let status = if *readiness.get(agent.as_str()).unwrap_or(&false) {
            core_ready += 1;
            "READY"
        } else {
            "MISSING"
        };
        let marker = if status == "READY" { "+" } else { "!" };
        println!("    [{}] {:18} {}", marker, agent, status);
    }

    if !roster.optional.is_empty() {
        println!();
        println!("  Optional agents:");
        for agent in &roster.optional {
            let status = if *readiness.get(agent.as_str()).unwrap_or(&false) {
                "READY"
            } else {
                "SKIPPED"
            };
            let marker = if status == "READY" { "+" } else { "~" };
            println!("    [{}] {:18} {}", marker, agent, status);
        }
    }

    println!();
    println!("  Core: {}/{} ready", core_ready, roster.core.len());
    println!();
}

/// Interactive options: recheck, resend, continue, or quit
fn prompt_interactive() -> char {
    println!("  Options:");
    println!("    [r] Recheck — poll missing agents again");
    println!("    [s] Resend — resend notification to missing agents only");
    println!("    [c] Continue — proceed with fleet restart for READY agents");
    println!("    [q] Quit — exit without restart (default)");
    println!();
    print!("  Choice [r/s/c/q]: ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        match input.trim().to_lowercase().chars().next() {
            Some('r') => 'r',
            Some('s') => 's',
            Some('c') => 'c',
            _ => 'q',
        }
    } else {
        'q'
    }
}

// ============================================================================
// Handoff schema validation (#9)
// ============================================================================

/// Minimum required fields for last-handoff-{agent}.json:
///   schema_version, agent, session_date, last_updated_epoch, state_summary, next_actions
fn validate_handoff_schemas(
    agents: &[String],
    state_dir: &PathBuf,
    broadcast_time: chrono::DateTime<Utc>,
) {
    let required_fields = ["schema_version", "agent", "session_date", "last_updated_epoch", "state_summary", "next_actions"];
    let mut issues: Vec<String> = Vec::new();

    for agent in agents {
        let path = state_dir.join(format!("last-handoff-{}.json", agent));
        if !path.exists() {
            issues.push(format!("  [!] {}: no handoff file", agent));
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                issues.push(format!("  [!] {}: unreadable — {}", agent, e));
                continue;
            }
        };

        let data: serde_json::Value = match serde_json::from_str(&content) {
            Ok(d) => d,
            Err(e) => {
                issues.push(format!("  [!] {}: invalid JSON — {}", agent, e));
                continue;
            }
        };

        let mut missing: Vec<&str> = Vec::new();
        for field in &required_fields {
            if data.get(field).is_none() {
                missing.push(field);
            }
        }

        if !missing.is_empty() {
            issues.push(format!("  [!] {}: missing fields: {}", agent, missing.join(", ")));
        }

        // Check staleness: epoch should be after broadcast
        if let Some(epoch) = data.get("last_updated_epoch").and_then(|v| v.as_i64()) {
            if let Some(ft) = chrono::DateTime::from_timestamp(epoch, 0) {
                if ft < broadcast_time {
                    issues.push(format!("  [~] {}: handoff file is stale (pre-broadcast)", agent));
                }
            }
        }
    }

    if !issues.is_empty() {
        println!("  Handoff schema warnings:");
        for issue in &issues {
            println!("{}", issue);
        }
        println!();
    }
}


// ============================================================================
// UID helper (macOS-compatible, no external crate needed)
// ============================================================================

fn get_current_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    // SAFETY: getuid() is always safe on POSIX systems
    unsafe { getuid() }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut args = Args::parse();

    // Fallback: read token from MQ_TOKEN env if not provided via CLI
    if args.token.is_none() {
        args.token = std::env::var("MQ_TOKEN").ok();
    }

    // Fallback: auto-load from tokens file keyed on from_agent
    if args.token.is_none() {
        if let Ok(content) = std::fs::read_to_string(&args.tokens_file) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(t) = data["tokens"][&args.from_agent].as_str() {
                    args.token = Some(t.to_string());
                    info!("Loaded token for '{}' from {}", args.from_agent, args.tokens_file.display());
                }
            }
        }
    }

    if args.token.is_none() {
        eprintln!("WARNING: No token found for '{}'. API calls will fail (401).", args.from_agent);
        eprintln!("  Set --token, MQ_TOKEN env, or ensure {} has a token for '{}'",
            args.tokens_file.display(), args.from_agent);
    }

    println!("════════════════════════════════════════");
    println!("  Daily Watch Handover v1.2");
    println!("  {}", Utc::now().format("%Y-%m-%d %H:%M UTC"));
    if args.dry_run {
        println!("  [DRY RUN MODE]");
    }
    println!("════════════════════════════════════════");
    println!();

    let manifest = load_manifest(&args.manifest)?;
    let roster = active_agents(&manifest);
    let agents = all_agents(&roster);
    info!("Fleet roster: {} core + {} optional agents", roster.core.len(), roster.optional.len());

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("Failed to build HTTP client")?;

    // ── Phase 1: Observer health ──
    println!("--- Phase 1: Observer Health ---");
    println!();

    let observer_status = check_observer_health();
    match observer_status {
        ObserverStatus::Running => {
            println!("  Observer: RUNNING");
        }
        ObserverStatus::Stopped => {
            println!("  Observer: STOPPED — restarting...");
            restart_observer(args.dry_run)?;
        }
        ObserverStatus::Hung => {
            println!("  Observer: HUNG (log stale >5min) — restarting...");
            restart_observer(args.dry_run)?;
        }
    }
    println!();

    // Jikimi daemon health check (not a session agent)
    check_jikimi_health();

    if args.check_only {
        println!("Phase 1 complete (--check-only). Exiting.");
        return Ok(());
    }

    // ── Preflight: am-fleet integrity gate ──
    println!("--- Preflight: Integrity Gate ---");
    println!();
    let preflight = Command::new("bash")
        .args(["/Users/proteus/astralmaris/astrallation/fleet/am-fleet.sh", "validate"])
        .output();
    match preflight {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if out.status.success() {
                println!("  Preflight: PASS");
            } else {
                print!("{}", stdout);
                eprintln!();
                eprintln!("  Preflight: FAIL — fix issues above before broadcasting.");
                eprintln!("  Hint: commit uncommitted fleet changes, then re-run.");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("  Preflight: WARNING — could not run am-fleet validate: {}", e);
        }
    }
    println!();

    // ── Phase 2: Broadcast warning ──
    println!("--- Phase 2: Fleet Warning Broadcast ---");
    println!();

    let broadcast_time = Utc::now();
    let sent = broadcast_warning(&agents, &http_client, &args).await?;
    println!();
    println!("  Broadcast: {}/{} agents notified", sent, agents.len());
    println!();

    // ── Phase 3: Wait for readiness ──
    println!("--- Phase 3: Readiness Collection ---");

    let mut readiness = wait_for_readiness(&agents, &http_client, &args, broadcast_time).await;

    // ── Phase 4: Interactive signoff ──
    println!("--- Phase 4: Signoff ---");

    display_readiness_dashboard(&roster, &readiness);

    // Validate handoff schema for agents that submitted files
    validate_handoff_schemas(&agents, &args.state_dir, broadcast_time);

    if args.skip_restart || args.dry_run {
        println!("Handover collection complete.");
        return Ok(());
    }

    // Interactive loop: recheck / resend / quit
    loop {
        let choice = prompt_interactive();
        match choice {
            'r' => {
                // Recheck: re-poll for readiness (only core agents block)
                println!();
                println!("  Rechecking readiness...");
                readiness = wait_for_readiness(&agents, &http_client, &args, broadcast_time).await;
                display_readiness_dashboard(&roster, &readiness);
            }
            's' => {
                // Resend: broadcast only to missing core agents
                println!();
                let missing: Vec<String> = roster.core
                    .iter()
                    .filter(|a| !readiness.get(a.as_str()).unwrap_or(&false))
                    .cloned()
                    .collect();
                if missing.is_empty() {
                    println!("  All core agents ready — nothing to resend.");
                } else {
                    println!("  Resending to {} missing core agent(s)...", missing.len());
                    let _ = broadcast_warning(&missing, &http_client, &args).await;
                    println!("  Re-polling...");
                    readiness = wait_for_readiness(&agents, &http_client, &args, broadcast_time).await;
                    display_readiness_dashboard(&roster, &readiness);
                }
            }
            'c' => {
                // Continue: proceed with fleet restart
                let missing_core: Vec<String> = roster.core
                    .iter()
                    .filter(|a| !readiness.get(a.as_str()).unwrap_or(&false))
                    .cloned()
                    .collect();

                if !missing_core.is_empty() {
                    println!();
                    println!("  WARNING: {} core agent(s) still MISSING: {}",
                        missing_core.len(),
                        missing_core.join(", "));
                    print!("  Proceed anyway? [y/N]: ");
                    let _ = io::stdout().flush();
                    let mut confirm = String::new();
                    if io::stdin().read_line(&mut confirm).is_ok() {
                        if confirm.trim().to_lowercase() != "y" {
                            println!("  Aborted. Returning to menu.");
                            continue;
                        }
                    } else {
                        println!("  Aborted.");
                        continue;
                    }
                }

                println!();
                println!("  Executing fleet restart via am-fleet...");
                println!();

                let fleet_script = "/Users/proteus/astralmaris/astrallation/fleet/am-fleet.sh";

                // am-fleet down
                info!("Running: am-fleet down");
                let output = Command::new("bash")
                    .args([fleet_script, "down"])
                    .output()
                    .context("Failed to run am-fleet down")?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                print!("{}", stdout);

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("am-fleet down reported issues: {}", stderr.trim());
                    // Continue anyway — down may warn but fleet restart should proceed
                }

                // Brief pause
                std::thread::sleep(Duration::from_secs(3));

                // am-fleet up
                info!("Running: am-fleet up");
                let output = Command::new("bash")
                    .args([fleet_script, "up"])
                    .output()
                    .context("Failed to run am-fleet up")?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                print!("{}", stdout);

                if output.status.success() {
                    println!();
                    println!("  Fleet restart complete.");
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!();
                    eprintln!("  Fleet restart had errors: {}", stderr.trim());
                }
                break;
            }
            _ => {
                println!("  Exiting. Handover data collected — restart manually when ready.");
                println!("  Run: am-fleet down && am-fleet up");
                break;
            }
        }
    }

    Ok(())
}
