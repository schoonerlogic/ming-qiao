//! Fleet Orchestrator — Deterministic process supervisor for the AstralMaris Council
//!
//! NOT an AI agent. A state-machine daemon that manages task queue, agent health,
//! work dispatch, and escalation. Think launchd/supervisord, not another wizard.
//!
//! Phase 1: Core loop with NATS JetStream subscription, agent health polling,
//! task dispatch via HTTP API, dashboard state JSON, osascript escalation.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::{error, info, warn};

use ming_qiao::nats::NatsAgentClient;
use ming_qiao::nats::streams;
use ming_qiao::state::NatsConfig;

// ============================================================================
// Configuration
// ============================================================================

const POLL_INTERVAL: Duration = Duration::from_secs(15);
const IDLE_THRESHOLD: Duration = Duration::from_secs(300);    // 5 min
const STUCK_THRESHOLD: Duration = Duration::from_secs(900);   // 15 min
const WAITING_ESCALATE: Duration = Duration::from_secs(1800); // 30 min
const ESCALATION_COOLDOWN: Duration = Duration::from_secs(3600); // 1 hour between escalations per agent

const MINGQIAO_API: &str = "http://localhost:7777/api";

// ============================================================================
// Agent state machine
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum AgentState {
    Working,
    Idle,
    Waiting,
    Stuck,
    Dead,
    Skip,
    Unknown,
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Working => write!(f, "WORKING"),
            Self::Idle => write!(f, "IDLE"),
            Self::Waiting => write!(f, "WAITING"),
            Self::Stuck => write!(f, "STUCK"),
            Self::Dead => write!(f, "DEAD"),
            Self::Skip => write!(f, "SKIP"),
            Self::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

#[derive(Debug, Clone)]
struct AgentInfo {
    state: AgentState,
    task: Option<String>,
    last_activity: Option<chrono::DateTime<Utc>>,
    waiting_since: Option<chrono::DateTime<Utc>>,
    last_escalation: Option<Instant>,
}

impl AgentInfo {
    fn new() -> Self {
        Self {
            state: AgentState::Unknown,
            task: None,
            last_activity: None,
            waiting_since: None,
            last_escalation: None,
        }
    }
}

// ============================================================================
// Dashboard state (written to JSON for am-fleet status --watch)
// ============================================================================

#[derive(Serialize)]
struct DashboardState {
    timestamp: String,
    agents: HashMap<String, DashboardAgent>,
    poll_interval_secs: u64,
}

#[derive(Serialize)]
struct DashboardAgent {
    state: AgentState,
    task: Option<String>,
    last_activity: Option<String>,
}

// ============================================================================
// Fleet manifest (minimal parser for agent list)
// ============================================================================

#[derive(Debug, Deserialize)]
struct FleetManifest {
    agents: HashMap<String, ManifestAgent>,
}

#[derive(Debug, Deserialize)]
struct ManifestAgent {
    role: Option<String>,
    runtime: Option<String>,
    worktree: Option<String>,
    skip_session_launch: Option<bool>,
}

fn load_manifest(path: &str) -> Option<FleetManifest> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

// ============================================================================
// Agent health polling
// ============================================================================

async fn poll_agent_state(
    agent: &str,
    manifest_agent: &ManifestAgent,
    http_client: &reqwest::Client,
) -> AgentState {
    if manifest_agent.skip_session_launch.unwrap_or(false) {
        return AgentState::Skip;
    }

    let runtime = manifest_agent.runtime.as_deref().unwrap_or("claude");

    // Check if agent is alive via process table
    let alive = match runtime {
        "claude-desktop" | "manual" => true, // Can't check
        rt => {
            // Detect agent process via pgrep -f (pattern match on full command)
            // Process names vary: "claude", "Kimi Code" (with space), node paths for opencode
            let pgrep_pattern = match rt {
                "claude-code" | "claude" => "/opt/homebrew/bin/claude",
                "kimi" => "Kimi Code",
                "opencode" => "opencode-ai",
                "codex" => "codex",
                _ => "",
            };
            if pgrep_pattern.is_empty() {
                false
            } else {
                Command::new("pgrep")
                    .args(["-f", pgrep_pattern])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
        }
    };

    if !alive {
        return AgentState::Dead;
    }

    // Check last message timestamp from ming-qiao
    let inbox_url = format!(
        "{}/inbox/{}?unread_only=false&limit=1&peek=true",
        MINGQIAO_API, agent
    );

    let last_activity = match http_client.get(&inbox_url).send().await {
        Ok(resp) => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                body["messages"]
                    .as_array()
                    .and_then(|msgs| msgs.first())
                    .and_then(|m| m["timestamp"].as_str())
                    .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                    .map(|dt| dt.with_timezone(&Utc))
            } else {
                None
            }
        }
        Err(_) => None,
    };

    let age = last_activity
        .map(|ts| Utc::now().signed_duration_since(ts))
        .map(|d| Duration::from_secs(d.num_seconds().max(0) as u64))
        .unwrap_or(Duration::from_secs(u64::MAX));

    // Check if agent is waiting for Proteus (sent request to merlin)
    let merlin_url = format!(
        "{}/inbox/merlin?unread_only=true&limit=10&peek=true",
        MINGQIAO_API
    );
    let waiting_for_human = match http_client.get(&merlin_url).send().await {
        Ok(resp) => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                body["messages"]
                    .as_array()
                    .map(|msgs| {
                        msgs.iter().any(|m| {
                            m["from"].as_str() == Some(agent)
                                && m["intent"].as_str() == Some("request")
                        })
                    })
                    .unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    };

    if waiting_for_human {
        return AgentState::Waiting;
    }

    if age < IDLE_THRESHOLD {
        AgentState::Working
    } else if age < STUCK_THRESHOLD {
        AgentState::Idle
    } else {
        AgentState::Stuck
    }
}

// ============================================================================
// Task discovery
// ============================================================================

async fn get_pending_tasks(
    agent: &str,
    http_client: &reqwest::Client,
) -> Vec<(String, String, String)> {
    // (id, from, subject)
    let url = format!(
        "{}/inbox/{}?unread_only=true&limit=5&peek=true",
        MINGQIAO_API, agent
    );

    match http_client.get(&url).send().await {
        Ok(resp) => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                body["messages"]
                    .as_array()
                    .map(|msgs| {
                        msgs.iter()
                            .filter(|m| {
                                matches!(
                                    m["intent"].as_str(),
                                    Some("request") | Some("discuss")
                                )
                            })
                            .map(|m| {
                                (
                                    m["id"].as_str().unwrap_or("").to_string(),
                                    m["from"].as_str().unwrap_or("").to_string(),
                                    m["subject"].as_str().unwrap_or("").to_string(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }
        Err(_) => vec![],
    }
}

// ============================================================================
// Escalation via osascript
// ============================================================================

fn escalate(agent: &str, reason: &str) {
    let msg = reason.replace('\\', "\\\\").replace('"', "\\\"");
    // Use display notification (non-blocking banner) instead of display alert (modal dialog)
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "display notification \"{}\" with title \"Fleet: {}\"",
            agent, msg
        ))
        .spawn();
    let _ = Command::new("afplay")
        .arg("/System/Library/Sounds/Submarine.aiff")
        .spawn();
}

// ============================================================================
// Dashboard state writer
// ============================================================================

fn write_dashboard_state(
    agents: &HashMap<String, AgentInfo>,
    state_path: &str,
) {
    let dashboard = DashboardState {
        timestamp: Utc::now().to_rfc3339(),
        agents: agents
            .iter()
            .filter(|(name, _)| name.as_str() != "_server" && name.as_str() != "council-chamber")
            .map(|(name, info)| {
                (
                    name.clone(),
                    DashboardAgent {
                        state: info.state,
                        task: info.task.clone(),
                        last_activity: info.last_activity.map(|t| t.to_rfc3339()),
                    },
                )
            })
            .collect(),
        poll_interval_secs: POLL_INTERVAL.as_secs(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&dashboard) {
        let tmp = format!("{}.tmp", state_path);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, state_path);
        }
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let manifest_path = env::var("FLEET_MANIFEST")
        .unwrap_or_else(|_| "/Users/proteus/astralmaris/astrallation/fleet/fleet-manifest.toml".to_string());

    let state_path = env::var("ORCHESTRATOR_STATE")
        .unwrap_or_else(|_| "/tmp/am-fleet-orchestrator-state.json".to_string());

    let dry_run = env::args().any(|a| a == "--dry-run");

    let manifest = match load_manifest(&manifest_path) {
        Some(m) => m,
        None => {
            error!("Failed to load fleet manifest from {}", manifest_path);
            std::process::exit(1);
        }
    };

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build HTTP client");

    let mut agent_infos: HashMap<String, AgentInfo> = HashMap::new();
    for agent in manifest.agents.keys() {
        agent_infos.insert(agent.clone(), AgentInfo::new());
    }

    info!("════════════════════════════════════════");
    info!("Fleet Orchestrator Phase 1 (Rust)");
    info!("  Manifest: {}", manifest_path);
    info!("  State: {}", state_path);
    info!("  Dry run: {}", dry_run);
    info!("  Poll interval: {}s", POLL_INTERVAL.as_secs());
    info!("════════════════════════════════════════");

    loop {
        for (agent_name, manifest_agent) in &manifest.agents {
            if agent_name == "_server" || agent_name == "council-chamber" {
                continue;
            }

            let state = poll_agent_state(agent_name, manifest_agent, &http_client).await;
            let info = agent_infos.entry(agent_name.clone()).or_insert_with(AgentInfo::new);
            info.state = state;

            match state {
                AgentState::Skip => continue,

                AgentState::Dead => {
                    info!("DEAD: {} — escalating", agent_name);
                    if !dry_run {
                        escalate(agent_name, "Session dead — manual restart needed");
                    }
                }

                AgentState::Stuck => {
                    let should_escalate = info
                        .last_escalation
                        .map(|t| t.elapsed() > ESCALATION_COOLDOWN)
                        .unwrap_or(true);

                    if should_escalate {
                        info!("STUCK: {} — escalating", agent_name);
                        if !dry_run {
                            escalate(
                                agent_name,
                                &format!(
                                    "Unresponsive for >{}min",
                                    STUCK_THRESHOLD.as_secs() / 60
                                ),
                            );
                        }
                        info.last_escalation = Some(Instant::now());
                    }
                }

                AgentState::Waiting => {
                    if info.waiting_since.is_none() {
                        info.waiting_since = Some(Utc::now());
                    }
                    let wait_duration = info
                        .waiting_since
                        .map(|ws| Utc::now().signed_duration_since(ws))
                        .map(|d| Duration::from_secs(d.num_seconds().max(0) as u64))
                        .unwrap_or_default();

                    if wait_duration > WAITING_ESCALATE {
                        let should_escalate = info
                            .last_escalation
                            .map(|t| t.elapsed() > ESCALATION_COOLDOWN)
                            .unwrap_or(true);

                        if should_escalate {
                            info!("WAITING: {} — escalating ({}min)", agent_name, wait_duration.as_secs() / 60);
                            if !dry_run {
                                escalate(
                                    agent_name,
                                    &format!(
                                        "Waiting for Proteus input for {}+ minutes",
                                        wait_duration.as_secs() / 60
                                    ),
                                );
                            }
                            info.last_escalation = Some(Instant::now());
                        }
                    }
                }

                AgentState::Idle => {
                    info.waiting_since = None;

                    // Check for pending tasks
                    let tasks = get_pending_tasks(agent_name, &http_client).await;
                    if let Some((_, from, subject)) = tasks.first() {
                        info!("DISPATCH: {} ← {} re: {}", agent_name, from, subject);
                        info.task = Some(subject.clone());
                        // Task dispatch: the message is already in the agent's inbox.
                        // The agent will see it when they check_messages.
                        // For now, log the dispatch. Phase 2: write PENDING_MESSAGES.md
                        // or cmux send to wake the agent.
                    } else {
                        info.task = None;
                    }
                }

                AgentState::Working => {
                    info.waiting_since = None;
                    let tasks = get_pending_tasks(agent_name, &http_client).await;
                    if let Some((_, _, subject)) = tasks.first() {
                        info.task = Some(subject.clone());
                    }
                }

                AgentState::Unknown => {}
            }
        }

        // Write dashboard state
        write_dashboard_state(&agent_infos, &state_path);

        sleep(POLL_INTERVAL).await;
    }
}
