//! Colloquium Daemon — Unified Rust rewrite of the Python colloquium framework
//!
//! Replaces 7 Python modules (1,459 lines):
//!   cast.py, adapter.py, pipeline.py, envelope.py,
//!   mingqiao.py, astrolabe_client.py, council-dispatch.py
//!
//! Architecture:
//!   - HTTP server (axum) exposing /cast and /health endpoints
//!   - Background dispatch loop watching notification files
//!   - Ed25519 signed envelopes for invocation/response provenance
//!   - ASTROLABE/Graphiti MCP client for knowledge graph context
//!   - Claude CLI subprocess invocation for voice generation
//!
//! Usage:
//!   colloquium-daemon                          # Start HTTP server + dispatch daemon
//!   colloquium-daemon cast --thread <id> --all # One-shot cast (CLI mode)
//!   colloquium-daemon --port 8200              # Custom port

use std::collections::{HashMap, HashSet};
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Json, State};
use axum::routing::{get, post};
use axum::{Router};
use chrono::Utc;
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{error, info, warn};

// ============================================================================
// CLI
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "colloquium-daemon", about = "Colloquium voice orchestration daemon")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// HTTP server port
    #[arg(long, default_value_t = 8200)]
    port: u16,

    /// Ming-qiao API URL
    #[arg(long, default_value = "http://localhost:7777")]
    mq_url: String,

    /// ASTROLABE MCP URL
    #[arg(long, default_value = "http://localhost:8001/mcp")]
    astrolabe_url: String,

    /// Colloquium root directory
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/colloquium")]
    colloquium_dir: PathBuf,

    /// Keys directory (Ed25519 seeds)
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/aleph/config/keys")]
    keys_dir: PathBuf,

    /// Council keyring path (public keys)
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/council-keyring.json")]
    keyring_path: PathBuf,

    /// Agent tokens file
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/agent-tokens.json")]
    tokens_path: PathBuf,

    /// Disable dispatch daemon (HTTP-only mode)
    #[arg(long)]
    no_dispatch: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// One-shot cast (CLI mode)
    Cast {
        /// Ming-qiao thread ID
        #[arg(long)]
        thread: Option<String>,
        /// Custom proposal text
        #[arg(long)]
        proposal: Option<String>,
        /// Context tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
        /// Specific agents to cast
        #[arg(long)]
        agent: Vec<String>,
        /// Cast all voices
        #[arg(long)]
        all: bool,
        /// Post results to ming-qiao
        #[arg(long)]
        post: bool,
        /// Claude model
        #[arg(long, default_value = "sonnet")]
        model: String,
        /// Include captain voices (merlin, thales)
        #[arg(long)]
        authored: bool,
    },
}

// ============================================================================
// Constants
// ============================================================================

const MAX_EVENT_AGE_SECS: u64 = 60;
const NONCE_TTL_SECS: u64 = 120;
const GRAPHITI_GROUP_ID: &str = "oracle_main";
const CAPTAIN_VOICES: &[&str] = &["merlin", "thales"];

const VOICE_CONFIGS: &[(&str, &str, &str)] = &[
    ("aleph", "Aleph", "Infrastructure architect — evaluate from systems reliability, deployment, and operational complexity perspective"),
    ("thales", "Thales", "Architecture lead — evaluate from structural integrity, pattern coherence, and long-term maintainability perspective"),
    ("luban", "Luban", "Inference engineer — evaluate from performance, latency, resource efficiency, and model serving perspective"),
    ("laozi-jung", "Laozi-Jung", "Witness and pattern observer — evaluate from emergent patterns, historical parallels, and systemic wisdom perspective"),
    ("mataya", "Mataya", "Design lead — evaluate from user experience, interface clarity, and aesthetic coherence perspective"),
    ("ogma", "Ogma", "Security architect — evaluate from threat surface, compliance, credential hygiene, and access control perspective"),
    ("merlin", "Merlin", "Project lead — evaluate from strategic alignment, resource allocation, and mission priorities perspective"),
];

const COMMITMENT_PATTERNS: &[&str] = &[
    r"\bI will\b",
    r"\bI'll\b(?!\s+(?:note|observe|watch|consider|think|flag|keep|check|look))",
    r"\bI commit\b",
    r"\bI volunteer\b",
    r"\blet me (?:handle|build|deploy|implement|fix|create)\b",
    r"\bI(?:'ll| will) (?:build|deploy|implement|fix|create|write|deliver|ship)\b",
    r"\bI take (?:ownership|responsibility)\b",
    r"\bI(?:'ll| will) have (?:it|this|that) (?:ready|done|finished)\b",
];

// ============================================================================
// Data types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignedEnvelope {
    event_id: String,
    from_agent: String,
    timestamp_utc: String,
    nonce: String,
    payload_hash: String,
    signature: String,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CastRequest {
    thread_id: Option<String>,
    proposal: Option<String>,
    tags: Option<Vec<String>>,
    agents: Option<Vec<String>>,
    all: Option<bool>,
    post: Option<bool>,
    model: Option<String>,
    authored: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct CastResult {
    agent_id: String,
    content: String,
    model: String,
    autonomous: bool,
    commitment_detected: bool,
    posted: bool,
    invocation_envelope: SignedEnvelope,
    response_envelope: SignedEnvelope,
    astrolabe_available: bool,
    response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CastResponse {
    results: Vec<CastResult>,
    thread_id: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AstrolabeBriefing {
    nodes: Vec<serde_json::Value>,
    facts: Vec<serde_json::Value>,
    raw_query: String,
    available: bool,
    error: String,
}

impl AstrolabeBriefing {
    fn to_text(&self, max_chars: usize) -> String {
        if !self.available {
            return String::new();
        }
        let mut lines = Vec::new();
        if !self.nodes.is_empty() {
            lines.push("### Relevant Entities".to_string());
            for node in &self.nodes {
                let name = node["name"].as_str().unwrap_or("?");
                let summary = node["summary"].as_str().unwrap_or("");
                lines.push(format!("- **{}**: {}", name, summary));
            }
        }
        if !self.facts.is_empty() {
            lines.push("### Relevant Facts".to_string());
            for fact in &self.facts {
                let text = fact["fact"].as_str().unwrap_or("");
                let invalid = fact["is_invalid"].as_bool().unwrap_or(false);
                if invalid {
                    lines.push(format!("- {} [SUPERSEDED]", text));
                } else {
                    lines.push(format!("- {}", text));
                }
            }
        }
        let full = lines.join("\n");
        if full.len() > max_chars {
            format!("{}...", &full[..max_chars - 3])
        } else {
            full
        }
    }
}

// ============================================================================
// Shared state
// ============================================================================

struct AppState {
    config: Args,
    http_client: reqwest::Client,
    keyring: HashMap<String, VerifyingKey>,
    tokens: HashMap<String, String>,
    nonce_registry: Mutex<NonceRegistry>,
}

// ============================================================================
// Nonce Registry (replay defense)
// ============================================================================

struct NonceRegistry {
    entries: HashMap<String, f64>,
    persist_path: PathBuf,
    ttl: u64,
}

impl NonceRegistry {
    fn new(persist_path: PathBuf, ttl: u64) -> Self {
        let mut reg = Self {
            entries: HashMap::new(),
            persist_path,
            ttl,
        };
        reg.load_persisted();
        reg
    }

    fn check_and_insert(&mut self, nonce: &str) -> bool {
        self.cleanup();
        if self.entries.contains_key(nonce) {
            return false; // replay
        }
        let now = now_epoch();
        self.entries.insert(nonce.to_string(), now);
        self.persist(nonce, now);
        true
    }

    fn cleanup(&mut self) {
        let cutoff = now_epoch() - self.ttl as f64;
        self.entries.retain(|_, ts| *ts > cutoff);
    }

    fn load_persisted(&mut self) {
        if let Ok(content) = std::fs::read_to_string(&self.persist_path) {
            let cutoff = now_epoch() - self.ttl as f64;
            for line in content.lines() {
                if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
                    if let (Some(nonce), Some(ts)) = (entry["nonce"].as_str(), entry["ts"].as_f64())
                    {
                        if ts > cutoff {
                            self.entries.insert(nonce.to_string(), ts);
                        }
                    }
                }
            }
        }
    }

    fn persist(&self, nonce: &str, ts: f64) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.persist_path)
        {
            let _ = writeln!(f, r#"{{"nonce":"{}","ts":{}}}"#, nonce, ts);
        }
    }
}

// ============================================================================
// Crypto helpers
// ============================================================================

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hex::encode(hasher.finalize())
}

fn canonical_json(value: &serde_json::Value) -> String {
    // Sort keys for canonical representation
    match value {
        serde_json::Value::Object(map) => {
            let mut pairs: Vec<_> = map.iter().collect();
            pairs.sort_by_key(|(k, _)| k.clone());
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", k, canonical_json(v)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        serde_json::Value::Array(arr) => {
            let inner: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", inner.join(","))
        }
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn hash_payload(payload: &serde_json::Value) -> String {
    sha256_hex(&canonical_json(payload))
}

fn generate_nonce() -> String {
    let mut bytes = [0u8; 32];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    hex::encode(bytes)
}

fn load_signing_key(keys_dir: &PathBuf, agent_id: &str) -> Option<SigningKey> {
    let path = keys_dir.join(format!("{}.seed", agent_id));
    let content = std::fs::read_to_string(&path).ok()?;
    let bytes = hex::decode(content.trim()).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    Some(SigningKey::from_bytes(&seed))
}

fn load_keyring(path: &PathBuf) -> HashMap<String, VerifyingKey> {
    let mut ring = HashMap::new();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return ring,
    };
    let data: serde_json::Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(_) => return ring,
    };
    if let Some(agents) = data["agents"].as_object() {
        for (agent_id, info) in agents {
            if let Some(pk_hex) = info["public_key"].as_str() {
                if let Ok(pk_bytes) = hex::decode(pk_hex) {
                    if pk_bytes.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&pk_bytes);
                        if let Ok(vk) = VerifyingKey::from_bytes(&arr) {
                            ring.insert(agent_id.clone(), vk);
                        }
                    }
                }
            }
        }
    }
    ring
}

fn load_tokens(path: &PathBuf) -> HashMap<String, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let data: serde_json::Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };
    let mut tokens = HashMap::new();
    if let Some(t) = data["tokens"].as_object() {
        for (k, v) in t {
            if let Some(s) = v.as_str() {
                tokens.insert(k.clone(), s.to_string());
            }
        }
    }
    tokens
}

// ============================================================================
// Envelope operations
// ============================================================================

impl SignedEnvelope {
    fn create(
        from_agent: &str,
        payload: serde_json::Value,
        signing_key: &SigningKey,
    ) -> Self {
        let event_id = uuid::Uuid::now_v7().to_string();
        let timestamp_utc = Utc::now().to_rfc3339();
        let nonce = generate_nonce();
        let payload_hash = hash_payload(&payload);

        let msg = format!("{}\n{}\n{}\n{}", event_id, timestamp_utc, nonce, payload_hash);
        let signature = signing_key.sign(msg.as_bytes());

        Self {
            event_id,
            from_agent: from_agent.to_string(),
            timestamp_utc,
            nonce,
            payload_hash,
            signature: hex::encode(signature.to_bytes()),
            payload,
        }
    }

    fn verify(
        &self,
        keyring: &HashMap<String, VerifyingKey>,
        nonce_registry: &mut NonceRegistry,
    ) -> Result<String, String> {
        // Check timestamp freshness
        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&self.timestamp_utc) {
            let age = Utc::now()
                .signed_duration_since(ts.with_timezone(&Utc))
                .num_seconds();
            if age < 0 {
                return Err("envelope timestamp is in the future".to_string());
            }
            if age as u64 > MAX_EVENT_AGE_SECS {
                return Err(format!("envelope too old ({}s)", age));
            }
        } else {
            return Err("invalid timestamp".to_string());
        }

        // Check nonce
        if !nonce_registry.check_and_insert(&self.nonce) {
            return Err("nonce replay detected".to_string());
        }

        // Check payload hash
        let expected_hash = hash_payload(&self.payload);
        if expected_hash != self.payload_hash {
            return Err("payload hash mismatch".to_string());
        }

        // Lookup verifying key
        let vk = keyring
            .get(&self.from_agent)
            .ok_or_else(|| format!("unknown agent: {}", self.from_agent))?;

        // Verify signature
        let msg = format!(
            "{}\n{}\n{}\n{}",
            self.event_id, self.timestamp_utc, self.nonce, self.payload_hash
        );
        let sig_bytes = hex::decode(&self.signature).map_err(|e| format!("bad signature hex: {}", e))?;
        if sig_bytes.len() != 64 {
            return Err("signature wrong length".to_string());
        }
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);

        vk.verify(msg.as_bytes(), &sig)
            .map_err(|e| format!("signature verification failed: {}", e))?;

        Ok(self.from_agent.clone())
    }
}

// ============================================================================
// Commitment detection
// ============================================================================

fn detect_commitment(text: &str) -> bool {
    for pattern in COMMITMENT_PATTERNS {
        if let Ok(re) = Regex::new(&format!("(?i){}", pattern)) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
}

// ============================================================================
// Ming-qiao client
// ============================================================================

async fn mq_read_thread(
    client: &reqwest::Client,
    mq_url: &str,
    thread_id: &str,
    token: &str,
) -> Result<serde_json::Value, String> {
    let url = format!("{}/api/thread/{}", mq_url, thread_id);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("thread read failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("thread read HTTP {}", resp.status()));
    }
    resp.json()
        .await
        .map_err(|e| format!("thread parse failed: {}", e))
}

async fn mq_post_reply(
    client: &reqwest::Client,
    mq_url: &str,
    thread_id: &str,
    agent_id: &str,
    token: &str,
    content: &str,
    intent: &str,
) -> Result<(), String> {
    let url = format!("{}/api/thread/{}/reply", mq_url, thread_id);
    let body = serde_json::json!({
        "from": agent_id,
        "to": "council",
        "content": content,
        "intent": intent,
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("reply post failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("reply post HTTP {}", resp.status()));
    }
    Ok(())
}

// ============================================================================
// ASTROLABE client
// ============================================================================

async fn query_astrolabe(
    client: &reqwest::Client,
    astrolabe_url: &str,
    tags: &[String],
) -> AstrolabeBriefing {
    let query = tags.join(" ");

    // Initialize MCP session
    let init_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "colloquium-daemon", "version": "1.0.0"}
        }
    });

    let init_resp = match client
        .post(astrolabe_url)
        .header("Content-Type", "application/json")
        .json(&init_body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return AstrolabeBriefing {
                nodes: vec![],
                facts: vec![],
                raw_query: query,
                available: false,
                error: format!("MCP init failed: {}", e),
            };
        }
    };

    let session_id = init_resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    // Search nodes
    let nodes = mcp_tool_call(client, astrolabe_url, &session_id, "search_nodes", &serde_json::json!({
        "query": query,
        "group_ids": [GRAPHITI_GROUP_ID],
        "max_nodes": 10
    })).await.unwrap_or_default();

    // Search facts
    let facts = mcp_tool_call(client, astrolabe_url, &session_id, "search_memory_facts", &serde_json::json!({
        "query": query,
        "group_ids": [GRAPHITI_GROUP_ID],
        "max_facts": 10
    })).await.unwrap_or_default();

    AstrolabeBriefing {
        nodes: extract_mcp_content(&nodes, "nodes"),
        facts: extract_mcp_content(&facts, "facts"),
        raw_query: query,
        available: true,
        error: String::new(),
    }
}

async fn mcp_tool_call(
    client: &reqwest::Client,
    url: &str,
    session_id: &Option<String>,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Option<serde_json::Value> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        }
    });

    let mut req = client
        .post(url)
        .header("Content-Type", "application/json");

    if let Some(sid) = session_id {
        req = req.header("Mcp-Session-Id", sid);
    }

    let resp = req.json(&body).send().await.ok()?;
    let text = resp.text().await.ok()?;

    // Try plain JSON first
    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
        if data.get("result").is_some() {
            return Some(data["result"].clone());
        }
    }

    // Fallback: SSE format
    for line in text.lines() {
        if let Some(data_str) = line.strip_prefix("data: ") {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                if data.get("result").is_some() {
                    return Some(data["result"].clone());
                }
            }
        }
    }
    None
}

fn extract_mcp_content(result: &serde_json::Value, key: &str) -> Vec<serde_json::Value> {
    if let Some(content) = result["content"].as_array() {
        for block in content {
            if block["type"].as_str() == Some("text") {
                if let Some(text) = block["text"].as_str() {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
                        if let Some(arr) = parsed[key].as_array() {
                            return arr.clone();
                        }
                    }
                }
            }
        }
    }
    vec![]
}

// ============================================================================
// Voice invocation (Claude CLI)
// ============================================================================

async fn invoke_claude_cli(
    system_prompt: &str,
    user_message: &str,
    model: &str,
) -> Result<(String, u64), String> {
    let start = Instant::now();

    let mut child = tokio::process::Command::new("claude")
        .args([
            "--print",
            "--model", model,
            "--system-prompt", system_prompt,
            "--no-session-persistence",
            "--disallowed-tools", "Bash,Edit,Write,Read,Glob,Grep,Agent,WebFetch,WebSearch",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("claude spawn failed: {}", e))?;

    // Write user message to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(user_message.as_bytes())
            .await
            .map_err(|e| format!("stdin write failed: {}", e))?;
    }

    let output = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| "claude invocation timed out (120s)".to_string())?
        .map_err(|e| format!("claude process error: {}", e))?;

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let content = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if content.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("claude returned empty response. stderr: {}", stderr));
    }

    Ok((content, elapsed_ms))
}

// ============================================================================
// Cast pipeline
// ============================================================================

async fn cast_voice(
    state: &AppState,
    agent_id: &str,
    thread_id: Option<&str>,
    proposal_text: Option<&str>,
    context_tags: &[String],
    do_post: bool,
    model: &str,
    autonomous: bool,
    prior_responses: &[(String, String)],
) -> Result<CastResult, String> {
    // 1. ASTROLABE briefing
    let tags = if context_tags.is_empty() {
        if let Some(p) = proposal_text {
            extract_tags(p)
        } else {
            vec![]
        }
    } else {
        context_tags.to_vec()
    };

    let briefing = query_astrolabe(&state.http_client, &state.config.astrolabe_url, &tags).await;

    // 2. Thread retrieval
    let (proposal, thread_messages) = if let Some(tid) = thread_id {
        let token = state.tokens.values().next().cloned().unwrap_or_default();
        match mq_read_thread(&state.http_client, &state.config.mq_url, tid, &token).await {
            Ok(data) => {
                let msgs = data["messages"].as_array().cloned().unwrap_or_default();
                let prop = msgs
                    .first()
                    .and_then(|m| m["content"].as_str())
                    .unwrap_or("")
                    .to_string();
                (
                    proposal_text.map(String::from).unwrap_or(prop),
                    msgs,
                )
            }
            Err(e) => {
                warn!("Thread read failed: {}", e);
                (proposal_text.unwrap_or("").to_string(), vec![])
            }
        }
    } else {
        (proposal_text.unwrap_or("").to_string(), vec![])
    };

    // 3. Load charter
    let charter_path = state.config.colloquium_dir.join("charters").join(format!("{}.md", agent_id));
    let charter = std::fs::read_to_string(&charter_path).unwrap_or_else(|_| {
        format!("You are {}, a member of the AstralMaris Council.", agent_id)
    });

    // 4. Sign invocation envelope
    let signing_key = load_signing_key(&state.config.keys_dir, agent_id)
        .ok_or_else(|| format!("no signing key for {}", agent_id))?;

    let invocation_payload = serde_json::json!({
        "type": "colloquium_invocation",
        "agent_id": agent_id,
        "thread_id": thread_id.unwrap_or(""),
        "proposal_hash": sha256_hex(&proposal),
        "context_tags": tags,
        "astrolabe_available": briefing.available,
        "prior_response_count": prior_responses.len(),
    });

    let invocation_envelope = SignedEnvelope::create(agent_id, invocation_payload, &signing_key);

    // Self-verify (Ogma control 1-3)
    {
        let mut nonce_reg = state.nonce_registry.lock().await;
        if let Err(e) = invocation_envelope.verify(&state.keyring, &mut nonce_reg) {
            return Err(format!("invocation self-verify failed: {}", e));
        }
    }

    // 5. Build user message and invoke
    let perspective = VOICE_CONFIGS
        .iter()
        .find(|(id, _, _)| *id == agent_id)
        .map(|(_, _, p)| *p)
        .unwrap_or("Provide your expert perspective");

    let briefing_text = briefing.to_text(8000);
    let mut sections = Vec::new();
    if !briefing_text.is_empty() {
        sections.push(format!("## Field Briefing (ASTROLABE)\n\n{}", briefing_text));
    }
    sections.push(format!("## Proposal\n\n{}", proposal));
    if !prior_responses.is_empty() {
        let mut prior_text = "## Prior Responses\n\n".to_string();
        for (from, content) in prior_responses {
            prior_text.push_str(&format!("### {}\n{}\n\n", from, content));
        }
        sections.push(prior_text);
    }
    sections.push(format!(
        "## Your Task\n\n{}\n\nProvide a focused, substantive response (300-600 words). Be direct and specific.",
        perspective
    ));
    let user_message = sections.join("\n\n---\n\n");

    let (content, response_time_ms) = invoke_claude_cli(&charter, &user_message, model).await?;

    // 6. Commitment detection
    let commitment_detected = detect_commitment(&content);

    // 7. Sign response envelope
    let response_payload = serde_json::json!({
        "type": "colloquium_response",
        "agent_id": agent_id,
        "thread_id": thread_id.unwrap_or(""),
        "invocation_id": invocation_envelope.event_id,
        "model": model,
        "response_hash": sha256_hex(&content),
        "commitment_detected": commitment_detected,
        "autonomous": autonomous,
    });

    let response_envelope = SignedEnvelope::create(agent_id, response_payload, &signing_key);

    // 8. Post to ming-qiao
    let mut posted = false;
    if do_post && thread_id.is_some() && !commitment_detected {
        let tid = thread_id.unwrap();
        let token = state.tokens.get(agent_id)
            .or_else(|| state.tokens.values().next())
            .cloned()
            .unwrap_or_default();

        let provenance = if autonomous { "autonomous" } else { "authored" };
        let tagged_content = format!(
            "{}\n\n--- *{} [colloquium voice | {} | {}]* ---\n<!-- colloquium-meta: {} -->",
            content,
            agent_id,
            model,
            provenance,
            serde_json::json!({
                "autonomous": autonomous,
                "model": model,
                "adapter": "colloquium-daemon",
                "invocation_id": invocation_envelope.event_id,
            })
        );

        match mq_post_reply(
            &state.http_client,
            &state.config.mq_url,
            tid,
            agent_id,
            &token,
            &tagged_content,
            "inform",
        )
        .await
        {
            Ok(_) => posted = true,
            Err(e) => warn!("Post reply failed for {}: {}", agent_id, e),
        }
    }

    // 9. Audit log
    let log_dir = state.config.colloquium_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let audit_path = log_dir.join("audit-log.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&audit_path)
    {
        let entry = serde_json::json!({
            "timestamp": Utc::now().to_rfc3339(),
            "agent": agent_id,
            "model": model,
            "thread_id": thread_id.unwrap_or(""),
            "invocation_envelope": invocation_envelope,
            "response_envelope": response_envelope,
            "astrolabe_available": briefing.available,
            "astrolabe_query": briefing.raw_query,
            "astrolabe_node_count": briefing.nodes.len(),
            "astrolabe_fact_count": briefing.facts.len(),
            "commitment_detected": commitment_detected,
            "autonomous": autonomous,
            "posted": posted,
        });
        let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }

    Ok(CastResult {
        agent_id: agent_id.to_string(),
        content,
        model: model.to_string(),
        autonomous,
        commitment_detected,
        posted,
        invocation_envelope,
        response_envelope,
        astrolabe_available: briefing.available,
        response_time_ms,
    })
}

fn extract_tags(text: &str) -> Vec<String> {
    let stopwords: HashSet<&str> = ["the", "and", "for", "with", "that", "this", "from", "into", "have", "been"]
        .iter()
        .copied()
        .collect();
    let mut tags: Vec<String> = text
        .split_whitespace()
        .filter(|w| w.len() >= 5 && !stopwords.contains(w.to_lowercase().as_str()))
        .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>();
    tags.dedup();
    tags.truncate(8);
    tags
}

// ============================================================================
// HTTP handlers
// ============================================================================

async fn handle_cast(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CastRequest>,
) -> Json<serde_json::Value> {
    let model = req.model.as_deref().unwrap_or("sonnet");
    let do_post = req.post.unwrap_or(false);
    let authored = req.authored.unwrap_or(false);
    let tags: Vec<String> = req.tags.unwrap_or_default();

    // Determine voices
    let voices: Vec<(&str, bool)> = if req.all.unwrap_or(false) {
        VOICE_CONFIGS
            .iter()
            .filter(|(id, _, _)| authored || !CAPTAIN_VOICES.contains(id))
            .map(|(id, _, _)| (*id, !CAPTAIN_VOICES.contains(id)))
            .collect()
    } else if let Some(agents) = &req.agents {
        agents
            .iter()
            .filter_map(|a| {
                VOICE_CONFIGS.iter().find(|(id, _, _)| *id == a.as_str())
                    .map(|(id, _, _)| (*id, !CAPTAIN_VOICES.contains(id)))
            })
            .collect()
    } else {
        return Json(serde_json::json!({"error": "specify --all or --agent"}));
    };

    let mut results = Vec::new();
    let mut prior_responses: Vec<(String, String)> = Vec::new();

    for (agent_id, autonomous) in &voices {
        info!("Casting voice: {}", agent_id);
        match cast_voice(
            &state,
            agent_id,
            req.thread_id.as_deref(),
            req.proposal.as_deref(),
            &tags,
            do_post,
            model,
            *autonomous,
            &prior_responses,
        )
        .await
        {
            Ok(result) => {
                prior_responses.push((result.agent_id.clone(), result.content.clone()));
                results.push(result);
            }
            Err(e) => {
                error!("Cast failed for {}: {}", agent_id, e);
                // Continue with next voice
            }
        }
    }

    Json(serde_json::json!(CastResponse {
        results,
        thread_id: req.thread_id,
    }))
}

async fn handle_health() -> &'static str {
    "ok"
}

// ============================================================================
// Dispatch daemon (replaces council-dispatch.py)
// ============================================================================

async fn dispatch_loop(state: Arc<AppState>) {
    let notifications_path = PathBuf::from(
        std::env::var("HOME").unwrap_or("/tmp".to_string())
    )
    .join("astralmaris/ming-qiao/notifications/council-chamber.jsonl");

    let state_path = state.config.colloquium_dir.join("logs").join("dispatch-state.json");
    let mut last_line: usize = load_dispatch_state(&state_path);

    info!("Dispatch daemon started, watching: {}", notifications_path.display());

    loop {
        sleep(Duration::from_secs(10)).await;

        let content = match std::fs::read_to_string(&notifications_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        if lines.len() <= last_line {
            continue;
        }

        for line in &lines[last_line..] {
            let msg: serde_json::Value = match serde_json::from_str(line) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let from = msg["from"].as_str().unwrap_or("");
            if from == "council-dispatch" || from == "colloquium-daemon" {
                continue;
            }

            let subject = msg["subject"].as_str().unwrap_or("").to_lowercase();
            let _content = msg["content"].as_str().unwrap_or("").to_lowercase();
            let to = msg["to"].as_str().unwrap_or("");
            let intent = msg["intent"].as_str().unwrap_or("");
            let thread_id = msg["thread_id"].as_str().unwrap_or("");

            let is_colloquium = (to == "council" && is_colloquium_subject(&subject))
                || intent == "colloquium";

            if is_colloquium && !thread_id.is_empty() {
                info!("Auto-casting colloquium for thread {}", thread_id);
                let tags = extract_tags_from_subject(&subject);
                let req = CastRequest {
                    thread_id: Some(thread_id.to_string()),
                    proposal: None,
                    tags: Some(tags),
                    agents: None,
                    all: Some(true),
                    post: Some(true),
                    model: Some("sonnet".to_string()),
                    authored: Some(false),
                };
                let _ = handle_cast(
                    State(state.clone()),
                    Json(req),
                )
                .await;
            }
        }

        last_line = lines.len();
        save_dispatch_state(&state_path, last_line);
    }
}

fn is_colloquium_subject(subject: &str) -> bool {
    let markers = ["colloquium —", "colloquium —", "colloquium -", "inaugural colloquium"];
    markers.iter().any(|m| subject.contains(m))
}

fn extract_tags_from_subject(subject: &str) -> Vec<String> {
    subject
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(6)
        .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn load_dispatch_state(path: &PathBuf) -> usize {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| v["last_line"].as_u64())
        .unwrap_or(0) as usize
}

fn save_dispatch_state(path: &PathBuf, last_line: usize) {
    let state = serde_json::json!({"last_line": last_line});
    let _ = std::fs::write(path, serde_json::to_string_pretty(&state).unwrap_or_default());
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

    let keyring = load_keyring(&args.keyring_path);
    let tokens = load_tokens(&args.tokens_path);

    let nonce_path = args.colloquium_dir.join("logs").join("nonce-registry.jsonl");
    let _ = std::fs::create_dir_all(args.colloquium_dir.join("logs"));

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let state = Arc::new(AppState {
        config: args,
        http_client,
        keyring,
        tokens,
        nonce_registry: Mutex::new(NonceRegistry::new(nonce_path, NONCE_TTL_SECS)),
    });

    // CLI mode: one-shot cast
    if let Some(Commands::Cast {
        thread,
        proposal,
        tags,
        agent,
        all,
        post,
        model,
        authored,
    }) = &state.config.command
    {
        let req = CastRequest {
            thread_id: thread.clone(),
            proposal: proposal.clone(),
            tags: tags.as_ref().map(|t| t.split(',').map(String::from).collect()),
            agents: if agent.is_empty() { None } else { Some(agent.clone()) },
            all: Some(*all),
            post: Some(*post),
            model: Some(model.clone()),
            authored: Some(*authored),
        };
        let result = handle_cast(State(state.clone()), Json(req)).await;
        println!("{}", serde_json::to_string_pretty(&result.0)?);
        return Ok(());
    }

    // Daemon mode: HTTP server + dispatch loop
    let port = state.config.port;
    let no_dispatch = state.config.no_dispatch;

    info!("════════════════════════════════════════");
    info!("Colloquium Daemon (Rust)");
    info!("  Port: {}", port);
    info!("  Dispatch: {}", if no_dispatch { "disabled" } else { "enabled" });
    info!("  Keyring: {} agents", state.keyring.len());
    info!("════════════════════════════════════════");

    // Start dispatch loop in background
    if !no_dispatch {
        let dispatch_state = state.clone();
        tokio::spawn(async move {
            dispatch_loop(dispatch_state).await;
        });
    }

    // HTTP server
    let app = Router::new()
        .route("/cast", post(handle_cast))
        .route("/health", get(handle_health))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    info!("Listening on 127.0.0.1:{}", port);
    axum::serve(listener, app).await?;

    Ok(())
}
