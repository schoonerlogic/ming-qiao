//! Fleet Metrics CLI — am-fleet-metrics
//!
//! Collects infrastructure metrics for the AstralMaris Council fleet.
//! Phase 1: Infrastructure metrics (no API keys needed)
//!
//! Data sources:
//! 1. ming-qiao message activity (SurrealDB query)
//! 2. Observer wake logs (observer.log parsing)
//! 3. Jikimi health data (JSONL parsing)
//! 4. Fleet manifest (fleet-manifest.toml)
//! 5. Handoff file freshness (last-handoff-{agent}.json)
//! 6. Process uptime (cmux/launchd)

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use surrealdb::engine::any::connect;
use surrealdb::opt::auth::Database;
use surrealdb::Surreal;

#[derive(Parser, Debug)]
#[command(name = "am-fleet-metrics")]
#[command(about = "Fleet Metrics CLI for AstralMaris Council")]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, default_value = "/Users/proteus/astralmaris/astrallation/fleet/fleet-manifest.toml")]
    manifest: PathBuf,

    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/logs/observer.log")]
    observer_log: PathBuf,

    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/jikimi/health")]
    jikimi_health_dir: PathBuf,

    #[arg(long, default_value = "/Users/proteus/astralmaris/astrallation/fleet/state")]
    fleet_state_dir: PathBuf,

    #[arg(long, default_value = "ws://localhost:8000")]
    surrealdb_url: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Collect and display fleet metrics")]
    Collect {
        #[arg(long, help = "Output as JSON instead of table")]
        json: bool,
        #[arg(long, help = "Store metrics in SurrealDB")]
        store: bool,
    },
    #[command(about = "Generate daily report and send via ming-qiao")]
    Report,
    #[command(about = "Query message activity from SurrealDB")]
    Messages {
        #[arg(long, default_value = "today")]
        period: String,
    },
    #[command(about = "Check handoff file freshness")]
    Handoffs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FleetManifest {
    version: u32,
    fleet: FleetConfig,
    agents: HashMap<String, AgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FleetConfig {
    base_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentConfig {
    config_path: String,
    worktree: Option<String>,
    role: String,
    runtime: String,
    role_description: Option<String>,
    startup_command: Option<String>,
    wake_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ObserverLogEntry {
    timestamp: DateTime<Utc>,
    agent: String,
    event: String,
    success: bool,
    tier: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JikimiHealthEntry {
    ts: DateTime<Utc>,
    check: String,
    tier: u8,
    status: String,
    message: String,
    agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HandoffFile {
    agent: String,
    session_date: String,
    ready_for_restart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentMetrics {
    agent_id: String,
    role: String,
    runtime: String,
    transport_type: String,
    connected: bool,
    last_active: Option<DateTime<Utc>>,
    uptime_seconds: Option<i64>,
    messages_today: u32,
    wake_attempts: u32,
    wake_successes: u32,
    handoff_ready: bool,
    handoff_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InfraHealth {
    service: String,
    status: String,
    last_check: Option<DateTime<Utc>>,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FleetMetricsSnapshot {
    recorded_at: DateTime<Utc>,
    agents: Vec<AgentMetrics>,
    infra_health: Vec<InfraHealth>,
    total_active: u32,
    total_dormant: u32,
    total_error: u32,
}

fn parse_fleet_manifest(path: &Path) -> Result<FleetManifest> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read fleet manifest: {:?}", path))?;
    let manifest: FleetManifest = toml::from_str(&content)
        .with_context(|| "Failed to parse fleet manifest TOML")?;
    Ok(manifest)
}

fn parse_observer_log(path: &Path) -> Result<Vec<ObserverLogEntry>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read observer log: {:?}", path))?;
    
    let mut entries = Vec::new();
    
    for line in content.lines() {
        if line.contains("observer") || line.contains("council_observer") {
            if let Some(entry) = parse_observer_line(line) {
                entries.push(entry);
            }
        }
    }
    
    Ok(entries)
}

fn parse_observer_line(line: &str) -> Option<ObserverLogEntry> {
    let line = strip_ansi_codes(line);
    
    if line.contains("woke successfully") {
        let timestamp = extract_iso_timestamp(&line)?;
        let agent = extract_field(&line, "agent")?;
        return Some(ObserverLogEntry {
            timestamp,
            agent,
            event: "wake".to_string(),
            success: true,
            tier: None,
        });
    }
    
    if line.contains("has unread messages") {
        let timestamp = extract_iso_timestamp(&line)?;
        let agent = extract_field(&line, "agent")?;
        let tier = extract_tier_from_label(&line);
        return Some(ObserverLogEntry {
            timestamp,
            agent,
            event: "pending_wake".to_string(),
            success: false,
            tier,
        });
    }
    
    if line.contains("no cmux workspace found") || line.contains("failed to wake") {
        let timestamp = extract_iso_timestamp(&line)?;
        let agent = extract_field(&line, "agent")?;
        return Some(ObserverLogEntry {
            timestamp,
            agent,
            event: "wake_failed".to_string(),
            success: false,
            tier: None,
        });
    }
    
    None
}

fn strip_ansi_codes(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

fn extract_iso_timestamp(line: &str) -> Option<DateTime<Utc>> {
    let re = regex::Regex::new(r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)").unwrap();
    let cap = re.captures(line)?;
    let ts_str = cap.get(1)?.as_str();
    DateTime::parse_from_rfc3339(ts_str)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn extract_field(line: &str, field: &str) -> Option<String> {
    let pattern = format!(r#"{}="([^"]+)""#, field);
    let re = regex::Regex::new(&pattern).unwrap();
    let cap = re.captures(line)?;
    Some(cap.get(1)?.as_str().to_string())
}

fn extract_tier_from_label(line: &str) -> Option<u8> {
    if line.contains("tier 1") {
        Some(1)
    } else if line.contains("tier 2") {
        Some(2)
    } else if line.contains("tier 3") {
        Some(3)
    } else {
        None
    }
}

fn parse_jikimi_health_dir(dir: &Path, date: NaiveDate) -> Result<Vec<JikimiHealthEntry>> {
    let filename = format!("{}.jsonl", date.format("%Y-%m-%d"));
    let path = dir.join(&filename);
    
    if !path.exists() {
        return Ok(Vec::new());
    }
    
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read jikimi health file: {:?}", path))?;
    
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<JikimiHealthEntry>(line) {
            entries.push(entry);
        }
    }
    
    Ok(entries)
}

fn parse_handoff_file(path: &Path) -> Result<HandoffFile> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read handoff file: {:?}", path))?;
    let handoff: HandoffFile = serde_json::from_str(&content)
        .with_context(|| "Failed to parse handoff JSON")?;
    Ok(handoff)
}

fn get_handoff_files(state_dir: &Path) -> Result<Vec<(String, HandoffFile)>> {
    let mut handoffs = Vec::new();
    
    if !state_dir.exists() {
        return Ok(handoffs);
    }
    
    for entry in fs::read_dir(state_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename.starts_with("last-handoff-") && filename.ends_with(".json") {
                let agent = filename
                    .trim_start_matches("last-handoff-")
                    .trim_end_matches(".json")
                    .to_string();
                
                if let Ok(handoff) = parse_handoff_file(&path) {
                    handoffs.push((agent, handoff));
                }
            }
        }
    }
    
    Ok(handoffs)
}

fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    
    if hours > 24 {
        let days = hours / 24;
        let remain_hours = hours % 24;
        format!("{}d {}h", days, remain_hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

fn format_timestamp(ts: Option<DateTime<Utc>>) -> String {
    match ts {
        Some(t) => {
            let local = t.with_timezone(&Local);
            let now = Local::now();
            let diff = now.signed_duration_since(local);
            
            if diff.num_minutes() < 1 {
                "just now".to_string()
            } else if diff.num_minutes() < 60 {
                format!("{}m ago", diff.num_minutes())
            } else if diff.num_hours() < 24 {
                format!("{}h ago", diff.num_hours())
            } else {
                format!("{}d ago", diff.num_days())
            }
        }
        None => "—".to_string(),
    }
}

fn status_emoji(connected: bool, messages_today: u32) -> &'static str {
    if connected {
        "✅"
    } else if messages_today > 0 {
        "⚠️"
    } else {
        "🔴"
    }
}

async fn query_message_counts(db: &Surreal<surrealdb::engine::any::Any>, today: NaiveDate) -> Result<HashMap<String, u32>> {
    let start_of_day: DateTime<Utc> = today.and_hms_opt(0, 0, 0)
        .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
        .unwrap();
    
    let mut result = db
        .query(
            "SELECT count() AS count, from AS agent \
             FROM message \
             WHERE created_at >= $start_of_day \
             GROUP BY from"
        )
        .bind(("start_of_day", start_of_day.to_rfc3339()))
        .await?;
    
    let rows: Vec<serde_json::Value> = result.take(0)?;
    let mut counts = HashMap::new();
    
    for row in rows {
        if let (Some(agent), Some(count)) = (
            row.get("agent").and_then(|v| v.as_str()),
            row.get("count").and_then(|v| v.as_i64()),
        ) {
            counts.insert(agent.to_string(), count as u32);
        }
    }
    
    Ok(counts)
}

async fn connect_surrealdb(url: &str) -> Result<Surreal<surrealdb::engine::any::Any>> {
    let db = connect(url).await
        .with_context(|| format!("Failed to connect to SurrealDB at {}", url))?;
    
    db.signin(Database {
        namespace: "astralmaris".to_string(),
        database: "mingqiao".to_string(),
        username: "root".to_string(),
        password: "root".to_string(),
    })
    .await
    .with_context(|| "Failed to authenticate to SurrealDB")?;
    
    db.use_ns("astralmaris")
        .use_db("mingqiao")
        .await
        .with_context(|| "Failed to use namespace/database")?;
    
    Ok(db)
}

fn print_metrics_table(metrics: &FleetMetricsSnapshot) {
    println!();
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  Fleet Metrics — {}", metrics.recorded_at.format("%Y-%m-%d %H:%M"));
    println!("═══════════════════════════════════════════════════════════════════");
    println!();
    println!("  {:<14} {:^6} {:>10} {:>14} {:>12}", 
        "Agent", "Status", "Msgs Today", "Last Active", "Uptime");
    println!("  {:<14} {:^6} {:>10} {:>14} {:>12}",
        "------", "------", "---------", "-----------", "------");
    
    for agent in &metrics.agents {
        let status = status_emoji(agent.connected, agent.messages_today);
        let last_active = format_timestamp(agent.last_active);
        let uptime = agent.uptime_seconds
            .map(format_duration)
            .unwrap_or_else(|| "—".to_string());
        
        println!("  {:<14} {:^6} {:>10} {:>14} {:>12}",
            agent.agent_id, status, agent.messages_today, last_active, uptime);
    }
    
    println!();
    println!("  Infrastructure:");
    for infra in &metrics.infra_health {
        let status = if infra.status == "ok" { "✅" } else { "🔴" };
        println!("    {} {}: {}", status, infra.service, infra.message);
    }
    
    println!();
    println!("  Summary:");
    println!("    Active agents: {}", metrics.total_active);
    println!("    Dormant agents: {}", metrics.total_dormant);
    println!("    Error state: {}", metrics.total_error);
    
    let handoff_ready = metrics.agents.iter().filter(|a| a.handoff_ready).count();
    let handoff_total = metrics.agents.len();
    println!("    Handover: {}/{} ready", handoff_ready, handoff_total);
    
    println!();
    println!("═══════════════════════════════════════════════════════════════════");
}

async fn store_snapshot(db: &Surreal<surrealdb::engine::any::Any>, snapshot: &FleetMetricsSnapshot) -> Result<()> {
    let schema = r#"
        DEFINE TABLE IF NOT EXISTS agent_status SCHEMALESS;
        DEFINE INDEX IF NOT EXISTS agent_id_idx ON agent_status COLUMNS agent_id;
        DEFINE INDEX IF NOT EXISTS recorded_at_idx ON agent_status COLUMNS recorded_at;
        
        DEFINE TABLE IF NOT EXISTS infra_health SCHEMALESS;
        DEFINE INDEX IF NOT EXISTS service_idx ON infra_health COLUMNS service;
        DEFINE INDEX IF NOT EXISTS recorded_at_idx ON infra_health COLUMNS recorded_at;
        
        DEFINE TABLE IF NOT EXISTS fleet_metrics_snapshot SCHEMALESS;
        DEFINE INDEX IF NOT EXISTS recorded_at_idx ON fleet_metrics_snapshot COLUMNS recorded_at;
    "#;
    db.query(schema).await?;
    
    for agent in &snapshot.agents {
        let mut val = serde_json::to_value(agent)?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert("recorded_at".to_string(), serde_json::to_value(&snapshot.recorded_at)?);
        }
        db.query("CREATE agent_status CONTENT $data")
            .bind(("data", val))
            .await?;
    }
    
    for infra in &snapshot.infra_health {
        let mut val = serde_json::to_value(infra)?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert("recorded_at".to_string(), serde_json::to_value(&snapshot.recorded_at)?);
        }
        db.query("CREATE infra_health CONTENT $data")
            .bind(("data", val))
            .await?;
    }
    
    let snapshot_val = serde_json::to_value(snapshot)?;
    db.query("CREATE fleet_metrics_snapshot CONTENT $data")
        .bind(("data", snapshot_val))
        .await?;
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    match args.command {
        Commands::Collect { json, store } => {
            let manifest = parse_fleet_manifest(&args.manifest)?;
            let observer_entries = parse_observer_log(&args.observer_log)?;
            let today = Local::now().date_naive();
            let jikimi_entries = parse_jikimi_health_dir(&args.jikimi_health_dir, today)?;
            let handoffs = get_handoff_files(&args.fleet_state_dir)?;
            
            let handoff_map: HashMap<String, HandoffFile> = handoffs
                .into_iter()
                .map(|(k, v)| (k, v))
                .collect();
            
            let mut agent_wake_stats: HashMap<String, (u32, u32)> = HashMap::new();
            for entry in &observer_entries {
                if entry.event == "wake" || entry.event == "wake_failed" {
                    let stats = agent_wake_stats.entry(entry.agent.clone()).or_insert((0, 0));
                    if entry.success {
                        stats.0 += 1;
                    }
                    stats.1 += 1;
                }
            }
            
            let mut infra_health = Vec::new();
            let mut last_checks: HashMap<String, DateTime<Utc>> = HashMap::new();
            
            for entry in &jikimi_entries {
                let last = last_checks.get(&entry.check).copied();
                if last.is_none() || entry.ts > last.unwrap() {
                    last_checks.insert(entry.check.clone(), entry.ts);
                }
            }
            
            let services = [("ming-qiao", "mingqiao_http"), ("nats", "nats"), ("surrealdb", "surrealdb"), ("falkordb", "falkordb")];
            for (service_name, check_name) in services {
                let mut service_status = InfraHealth {
                    service: service_name.to_string(),
                    status: "unknown".to_string(),
                    last_check: None,
                    message: "No health data".to_string(),
                };
                
                for entry in jikimi_entries.iter().rev() {
                    if entry.check == check_name {
                        let status = match entry.status.as_str() {
                            "ok" => "healthy",
                            "degraded" => "degraded",
                            "error" => "down",
                            s => s,
                        };
                        service_status.status = status.to_string();
                        service_status.last_check = Some(entry.ts);
                        service_status.message = entry.message.clone();
                        break;
                    }
                }
                
                infra_health.push(service_status);
            }
            
            let mut agents = Vec::new();
            
            let db = connect_surrealdb(&args.surrealdb_url).await.ok();
            let message_counts = if let Some(ref db) = db {
                query_message_counts(db, today).await.unwrap_or_default()
            } else {
                HashMap::new()
            };
            
            for (agent_id, config) in &manifest.agents {
                if agent_id.starts_with('_') || agent_id == "council-chamber" {
                    continue;
                }
                
                let (wake_successes, wake_attempts) = agent_wake_stats.get(agent_id)
                    .copied()
                    .unwrap_or((0, 0));
                
                let handoff = handoff_map.get(agent_id);
                
                let connected = wake_successes > 0 || 
                    observer_entries.iter().rev().any(|e| 
                        e.agent == *agent_id && e.success
                    );
                
                let last_active = observer_entries.iter()
                    .rev()
                    .find(|e| e.agent == *agent_id)
                    .map(|e| e.timestamp);
                
                let messages_today = message_counts.get(agent_id).copied().unwrap_or(0);
                
                let transport_type = match config.runtime.as_str() {
                    "claude-code" | "opencode" | "kimi" | "codex" => "stdio",
                    "gemini" => "stdio",
                    _ => "streamable-http",
                };
                
                let agent_metrics = AgentMetrics {
                    agent_id: agent_id.clone(),
                    role: config.role.clone(),
                    runtime: config.runtime.clone(),
                    transport_type: transport_type.to_string(),
                    connected,
                    last_active,
                    uptime_seconds: None,
                    messages_today,
                    wake_attempts,
                    wake_successes,
                    handoff_ready: handoff.map(|h| h.ready_for_restart).unwrap_or(false),
                    handoff_date: handoff.map(|h| h.session_date.clone()),
                };
                
                agents.push(agent_metrics);
            }
            
            let total_active = agents.iter().filter(|a| a.connected).count() as u32;
            let total_dormant = agents.iter().filter(|a| !a.connected && a.wake_attempts == 0).count() as u32;
            let total_error = agents.iter().filter(|a| !a.connected && a.wake_attempts > 0).count() as u32;
            
            let snapshot = FleetMetricsSnapshot {
                recorded_at: Utc::now(),
                agents,
                infra_health,
                total_active,
                total_dormant,
                total_error,
            };
            
            if store {
                if let Some(ref db) = db {
                    store_snapshot(db, &snapshot).await?;
                    eprintln!("Metrics stored to SurrealDB");
                } else {
                    eprintln!("Warning: Could not connect to SurrealDB, metrics not stored");
                }
            }
            
            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            } else {
                print_metrics_table(&snapshot);
            }
        }
        Commands::Report => {
            println!("Daily report generation not yet implemented.");
            println!("Use 'am-fleet-metrics collect' to gather metrics.");
        }
        Commands::Messages { period } => {
            println!("Message activity for: {}", period);
            println!("SurrealDB query not yet connected. Use --store flag with collect.");
        }
        Commands::Handoffs => {
            let handoffs = get_handoff_files(&args.fleet_state_dir)?;
            
            println!();
            println!("Handoff File Status:");
            println!("{:<14} {:<12} {:>8}", "Agent", "Date", "Ready");
            println!("{:<14} {:<12} {:>8}", "------", "----", "-----");
            
            for (agent, handoff) in handoffs {
                let ready = if handoff.ready_for_restart { "✅" } else { "🔴" };
                println!("{:<14} {:<12} {:>8}", agent, handoff.session_date, ready);
            }
        }
    }
    
    Ok(())
}
