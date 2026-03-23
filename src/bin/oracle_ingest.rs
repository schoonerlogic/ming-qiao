//! ORACLE Ingestion Pipeline — Unified Rust binary
//!
//! Replaces 4 Python scripts (1,544 lines):
//!   oracle-ingest.py (244) — universal MCP ingestion entry
//!   oracle-reingest.py (292) — batch arXiv + seed docs
//!   oracle-refresh.py (418) — automated daemon (4 sources)
//!   chamber-voice.py (590) — autonomous agent polling + Ollama
//!
//! Subcommands:
//!   oracle-ingest ingest    — Ingest a single episode into Graphiti
//!   oracle-ingest reingest  — Batch ingest arXiv papers + seed docs
//!   oracle-ingest refresh   — Daemon watching 4 sources continuously
//!   oracle-ingest voice     — Autonomous colloquium voice responder

use std::collections::{HashMap, HashSet};
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use regex::Regex;
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

// ============================================================================
// CLI
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "oracle-ingest", about = "ORACLE ingestion pipeline for Graphiti knowledge graph")]
struct Args {
    #[command(subcommand)]
    command: Commands,

    /// Graphiti MCP URL
    #[arg(long, default_value = "http://localhost:8001/mcp", global = true)]
    mcp_url: String,

    /// Ming-qiao API URL
    #[arg(long, default_value = "http://localhost:7777", global = true)]
    mq_url: String,

    /// Graphiti group ID
    #[arg(long, default_value = "oracle_main", global = true)]
    group_id: String,

    /// Journal file for dedup tracking
    #[arg(long, default_value = "/Users/proteus/astralmaris/oracle/aleph/scripts/logs/ingest-journal.jsonl", global = true)]
    journal: PathBuf,

    /// Dry run — don't actually ingest
    #[arg(long, global = true)]
    dry_run: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Ingest a single episode into Graphiti
    Ingest {
        /// Episode name/title
        #[arg(long)]
        name: String,
        /// Episode content (or read from stdin if omitted)
        #[arg(long)]
        body: Option<String>,
        /// Source description (e.g., "arxiv:2401.12345")
        #[arg(long, default_value = "manual")]
        source: String,
        /// Content format
        #[arg(long, default_value = "text")]
        format: String,
        /// Force re-ingest even if already in journal
        #[arg(long)]
        force: bool,
    },
    /// Batch ingest arXiv papers + seed documents
    Reingest {
        /// Only ingest seed docs
        #[arg(long)]
        seed_only: bool,
        /// Use cached Gmail papers instead of re-fetching
        #[arg(long)]
        use_cache: bool,
        /// Force re-ingest all
        #[arg(long)]
        force: bool,
        /// Specific arXiv IDs to ingest
        #[arg(long)]
        arxiv_ids: Vec<String>,
    },
    /// Automated refresh daemon (watches 4 sources)
    Refresh {
        /// Run once then exit
        #[arg(long)]
        once: bool,
        /// Only refresh specific source
        #[arg(long)]
        source: Option<String>,
        /// Token file for ming-qiao auth
        #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/agent-tokens.json")]
        tokens_file: PathBuf,
    },
    /// Autonomous colloquium voice responder
    Voice {
        /// Run once then exit
        #[arg(long)]
        once: bool,
        /// Agents to respond as
        #[arg(long, default_value = "laozi-jung,mataya,ogma")]
        agents: String,
        /// Ollama URL
        #[arg(long, default_value = "http://localhost:11434")]
        ollama_url: String,
        /// Ollama model
        #[arg(long, default_value = "qwen3:8b")]
        model: String,
        /// Token file
        #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/agent-tokens.json")]
        tokens_file: PathBuf,
    },
}

// ============================================================================
// Shared: Graphiti MCP client
// ============================================================================

struct McpClient {
    client: reqwest::Client,
    url: String,
    group_id: String,
    session_id: Option<String>,
}

impl McpClient {
    fn new(url: &str, group_id: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("HTTP client"),
            url: url.to_string(),
            group_id: group_id.to_string(),
            session_id: None,
        }
    }

    async fn initialize(&mut self) -> Result<()> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "oracle-ingest", "version": "1.0.0"}
            }
        });

        let resp = self.client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("MCP initialize failed")?;

        self.session_id = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        Ok(())
    }

    async fn add_memory(&self, name: &str, body: &str, source: &str) -> Result<serde_json::Value> {
        let tool_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "add_memory",
                "arguments": {
                    "name": name,
                    "episode_body": body,
                    "source": "json",
                    "source_description": source,
                    "group_id": self.group_id
                }
            }
        });

        let mut req = self.client
            .post(&self.url)
            .header("Content-Type", "application/json");

        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid);
        }

        let resp = req.json(&tool_body).send().await.context("MCP add_memory failed")?;
        let text = resp.text().await?;

        // Parse SSE or plain JSON
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
            return Ok(data);
        }
        for line in text.lines() {
            if let Some(data_str) = line.strip_prefix("data: ") {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                    if data.get("result").is_some() {
                        return Ok(data);
                    }
                }
            }
        }
        anyhow::bail!("Failed to parse MCP response")
    }
}

// ============================================================================
// Shared: Dedup journal
// ============================================================================

fn content_hash(name: &str, body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}|{}", name, body).as_bytes());
    hex::encode(&hasher.finalize()[..8]) // 16-char hex prefix
}

fn load_journal(path: &PathBuf) -> HashSet<String> {
    let mut hashes = HashSet::new();
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(h) = entry["hash"].as_str() {
                    hashes.insert(h.to_string());
                }
            }
        }
    }
    hashes
}

fn append_journal(path: &PathBuf, hash: &str, name: &str, source: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let entry = serde_json::json!({
            "hash": hash,
            "name": name,
            "source": source,
            "timestamp": Utc::now().to_rfc3339()
        });
        let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }
}

// ============================================================================
// Subcommand: ingest
// ============================================================================

async fn cmd_ingest(args: &Args, name: &str, body: &str, source: &str, force: bool) -> Result<()> {
    let hash = content_hash(name, body);
    let journal = load_journal(&args.journal);

    if !force && journal.contains(&hash) {
        info!("Skipping (already ingested): {} [{}]", name, hash);
        return Ok(());
    }

    if args.dry_run {
        info!("[DRY RUN] Would ingest: {} ({} chars, source: {})", name, body.len(), source);
        return Ok(());
    }

    let mut mcp = McpClient::new(&args.mcp_url, &args.group_id);
    mcp.initialize().await?;

    info!("Ingesting: {} ({} chars)", name, body.len());
    let result = mcp.add_memory(name, body, source).await?;
    info!("Ingested: {} → {:?}", name, result.get("result").and_then(|r| r.get("message")));

    append_journal(&args.journal, &hash, name, source);
    Ok(())
}

// ============================================================================
// Subcommand: reingest (arXiv + seeds)
// ============================================================================

const SEED_DOCS: &[(&str, &str, &str)] = &[
    ("AstralMaris Council Architecture", "council:architecture",
     "The AstralMaris Council is a multi-agent system with agents: Aleph (infrastructure), Thales (architecture), Luban (inference), Laozi-Jung (witness/patterns), Mataya (design), Ogma (security). Communication via ming-qiao messaging bridge over NATS JetStream. Knowledge stored in ORACLE/Graphiti knowledge graph backed by FalkorDB."),
    ("AstralMaris Technology Stack", "council:technology",
     "Primary stack: Rust (systems/backend/CLI), TypeScript + Svelte (frontend). Inference: Ollama with qwen3:8b for extraction, nomic-embed-text for embeddings. Model modification: LoRA, distillation, model merging (TIES/DARE/SLERP) targeting 1-3B models. Base model: Qwen2.5-3B-Instruct."),
    ("ORACLE Knowledge Graph", "council:oracle",
     "ORACLE is the research intelligence knowledge graph. Architecture: Gmail/arXiv papers ingested via MCP into Graphiti server backed by FalkorDB. Graph stats: ~800 nodes, ~1800 relationships, ~80 episodes. Models: qwen3:8b for extraction, nomic-embed-text (768d) for embeddings via Ollama."),
    ("AstralMaris Research Foundations", "council:research",
     "Core research areas: model composition (LoRA adapter merging vs joint training), small language model optimization, mixture of experts, knowledge distillation. Key finding from ATLAS-01: adapter merging fails for factual knowledge across all configs (linear/TIES/DARE), but joint training on curated domain bundles preserves accuracy."),
];

async fn cmd_reingest(args: &Args, seed_only: bool, use_cache: bool, force: bool, arxiv_ids: &[String]) -> Result<()> {
    // Ingest seed documents
    info!("Ingesting {} seed documents...", SEED_DOCS.len());
    for (name, source, body) in SEED_DOCS {
        cmd_ingest(args, name, body, source, force).await?;
    }

    if seed_only {
        info!("Seed-only mode — done.");
        return Ok(());
    }

    // arXiv papers
    let papers = if !arxiv_ids.is_empty() {
        fetch_arxiv_metadata(&arxiv_ids).await?
    } else if use_cache {
        load_arxiv_cache()?
    } else {
        info!("No arXiv IDs specified and --use-cache not set. Skipping arXiv.");
        vec![]
    };

    info!("Ingesting {} arXiv papers...", papers.len());
    for paper in &papers {
        let name = format!("[arXiv:{}] {}", paper.arxiv_id, paper.title);
        let body = format!(
            "Title: {}\nAuthors: {}\nArXiv ID: {}\n\nAbstract:\n{}",
            paper.title, paper.authors, paper.arxiv_id, paper.abstract_text
        );
        cmd_ingest(args, &name, &body, &format!("arxiv:{}", paper.arxiv_id), force).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    info!("Reingest complete: {} seeds + {} papers", SEED_DOCS.len(), papers.len());
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct ArxivPaper {
    arxiv_id: String,
    title: String,
    authors: String,
    #[serde(alias = "abstract")]
    abstract_text: String,
}

fn load_arxiv_cache() -> Result<Vec<ArxivPaper>> {
    let cache_path = PathBuf::from("/Users/proteus/astralmaris/oracle/aleph/scripts/logs/gmail-papers.json");
    let content = std::fs::read_to_string(&cache_path)
        .context("Failed to read Gmail papers cache")?;
    let papers: Vec<ArxivPaper> = serde_json::from_str(&content)
        .context("Failed to parse Gmail papers cache")?;
    Ok(papers)
}

async fn fetch_arxiv_metadata(ids: &[String]) -> Result<Vec<ArxivPaper>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let mut papers = Vec::new();

    // Batch in groups of 20
    for chunk in ids.chunks(20) {
        let id_list = chunk.join(",");
        let url = format!(
            "https://export.arxiv.org/api/query?id_list={}&max_results={}",
            urlencoding::encode(&id_list),
            chunk.len()
        );

        let text = client.get(&url)
            .header("User-Agent", "oracle-ingest/1.0")
            .send()
            .await?
            .text()
            .await?;

        // Parse Atom XML
        let mut reader = quick_xml::Reader::from_str(&text);
        reader.config_mut().trim_text(true);

        let mut in_entry = false;
        let mut current_tag = String::new();
        let mut title = String::new();
        let mut summary = String::new();
        let mut arxiv_id = String::new();
        let mut authors: Vec<String> = Vec::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(e)) => {
                    let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if tag == "entry" {
                        in_entry = true;
                        title.clear();
                        summary.clear();
                        arxiv_id.clear();
                        authors.clear();
                    }
                    if in_entry {
                        current_tag = tag;
                    }
                }
                Ok(quick_xml::events::Event::Text(e)) => {
                    if in_entry {
                        let text = e.unescape().unwrap_or_default().to_string();
                        match current_tag.as_str() {
                            "title" => title.push_str(&text),
                            "summary" => summary.push_str(&text),
                            "name" => authors.push(text.trim().to_string()),
                            "id" => {
                                // Extract arXiv ID from URL like http://arxiv.org/abs/2401.12345v1
                                let re = Regex::new(r"\d{4}\.\d{4,5}(?:v\d+)?").unwrap();
                                if let Some(m) = re.find(&text) {
                                    arxiv_id = m.as_str().to_string();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if tag == "entry" && in_entry {
                        in_entry = false;
                        if !title.is_empty() && !arxiv_id.is_empty() {
                            let author_str = if authors.len() > 5 {
                                format!("{} et al. ({} authors)", authors[..5].join(", "), authors.len())
                            } else {
                                authors.join(", ")
                            };
                            papers.push(ArxivPaper {
                                arxiv_id: arxiv_id.trim().to_string(),
                                title: title.split_whitespace().collect::<Vec<_>>().join(" "),
                                authors: author_str,
                                abstract_text: summary.split_whitespace().collect::<Vec<_>>().join(" "),
                            });
                        }
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        // Rate limit: 3s between batches
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    Ok(papers)
}

// ============================================================================
// Subcommand: refresh (daemon)
// ============================================================================

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct RefreshState {
    #[serde(default)]
    captains_log_seen: HashMap<String, i64>,
    #[serde(default)]
    colloquium_last_line: usize,
    #[serde(default)]
    threads_ingested: Vec<String>,
    #[serde(default)]
    gmail_last_check: Option<String>,
}

const REFRESH_STATE_PATH: &str = "/Users/proteus/astralmaris/oracle/aleph/scripts/logs/refresh-state.json";
const CAPTAINS_LOG_DIR: &str = "/Users/proteus/astralmaris/ming-qiao/main/docs/captains-log";
const COLLOQUIUM_AUDIT_LOG: &str = "/Users/proteus/astralmaris/ming-qiao/main/colloquium/logs/audit-log.jsonl";
const DECISION_KEYWORDS: &[&str] = &[
    "directive", "decision", "architecture", "gate", "security",
    "phase", "colloquium", "founding", "magna carta", "design brief",
];

fn load_refresh_state() -> RefreshState {
    std::fs::read_to_string(REFRESH_STATE_PATH)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn save_refresh_state(state: &RefreshState) {
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(REFRESH_STATE_PATH, json);
    }
}

async fn refresh_captains_log(args: &Args, state: &mut RefreshState) -> Result<u32> {
    let dir = PathBuf::from(CAPTAINS_LOG_DIR);
    if !dir.exists() {
        return Ok(0);
    }

    let mut ingested = 0u32;
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let metadata = std::fs::metadata(&path)?;
            let mtime = metadata
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            let prev_mtime = state.captains_log_seen.get(&filename).copied().unwrap_or(0);
            if mtime > prev_mtime {
                let content = std::fs::read_to_string(&path)?;
                let name = format!("Captain's Log: {}", filename.trim_end_matches(".md"));
                cmd_ingest(args, &name, &content, &format!("captains-log:{}", filename), false).await?;
                state.captains_log_seen.insert(filename, mtime);
                ingested += 1;
            }
        }
    }
    Ok(ingested)
}

async fn refresh_threads(
    args: &Args,
    state: &mut RefreshState,
    mq_token: &str,
) -> Result<u32> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let resp = client
        .get(format!("{}/api/threads?limit=50", args.mq_url))
        .header("Authorization", format!("Bearer {}", mq_token))
        .send()
        .await?;

    let threads: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
    let mut ingested = 0u32;

    for thread in &threads {
        let tid = thread["thread_id"].as_str().unwrap_or("");
        let subject = thread["subject"].as_str().unwrap_or("").to_lowercase();

        if tid.is_empty() || state.threads_ingested.contains(&tid.to_string()) {
            continue;
        }

        // Filter: only ingest threads with decision keywords
        let is_decision = DECISION_KEYWORDS.iter().any(|kw| subject.contains(kw));
        if !is_decision {
            continue;
        }

        // Fetch full thread
        let thread_resp = client
            .get(format!("{}/api/thread/{}", args.mq_url, tid))
            .header("Authorization", format!("Bearer {}", mq_token))
            .send()
            .await;

        if let Ok(resp) = thread_resp {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                let messages = data["messages"].as_array();
                if let Some(msgs) = messages {
                    let mut transcript = format!("# Thread: {}\n\n", subject);
                    for msg in msgs {
                        let from = msg["from"].as_str().unwrap_or("?");
                        let content = msg["content"].as_str().unwrap_or("");
                        transcript.push_str(&format!("## {}\n{}\n\n", from, content));
                    }
                    let name = format!("Thread: {}", thread["subject"].as_str().unwrap_or("?"));
                    cmd_ingest(args, &name, &transcript, &format!("thread:{}", tid), false).await?;
                    state.threads_ingested.push(tid.to_string());
                    ingested += 1;
                }
            }
        }
    }
    Ok(ingested)
}

async fn cmd_refresh(args: &Args, once: bool, source: Option<&str>, tokens_file: &PathBuf) -> Result<()> {
    let mq_token = load_token(tokens_file, "aleph");

    loop {
        let mut state = load_refresh_state();
        let mut total = 0u32;

        if source.is_none() || source == Some("captains-log") {
            match refresh_captains_log(args, &mut state).await {
                Ok(n) => { total += n; if n > 0 { info!("Captain's Log: ingested {} entries", n); } }
                Err(e) => warn!("Captain's Log refresh failed: {}", e),
            }
        }

        if source.is_none() || source == Some("threads") {
            match refresh_threads(args, &mut state, &mq_token).await {
                Ok(n) => { total += n; if n > 0 { info!("Threads: ingested {} decision threads", n); } }
                Err(e) => warn!("Threads refresh failed: {}", e),
            }
        }

        save_refresh_state(&state);

        if total > 0 {
            info!("Refresh cycle: {} items ingested", total);
        }

        if once {
            break;
        }

        tokio::time::sleep(Duration::from_secs(60)).await;
    }

    Ok(())
}

// ============================================================================
// Subcommand: voice (autonomous colloquium responder)
// ============================================================================

async fn cmd_voice(
    args: &Args,
    once: bool,
    agent_list: &str,
    ollama_url: &str,
    model: &str,
    tokens_file: &PathBuf,
) -> Result<()> {
    let agents: Vec<&str> = agent_list.split(',').map(|s| s.trim()).collect();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let commitment_re: Vec<Regex> = vec![
        Regex::new(r"(?i)\bI will\b")?,
        Regex::new(r"(?i)\bI'll\b(?!\s+(?:note|observe|watch|consider|think|flag|keep|check|look))")?,
        Regex::new(r"(?i)\bI commit\b")?,
        Regex::new(r"(?i)\bI volunteer\b")?,
        Regex::new(r"(?i)\blet me (?:handle|build|deploy|implement|fix|create)\b")?,
    ];

    info!("Voice daemon: agents={:?}, model={}", agents, model);

    loop {
        for agent in &agents {
            let token = load_token(tokens_file, agent);
            if token.is_empty() {
                continue;
            }

            // Check inbox for colloquium messages
            let inbox_url = format!("{}/api/inbox/{}?unread_only=true&limit=5", args.mq_url, agent);
            let resp = client
                .get(&inbox_url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await;

            if let Ok(resp) = resp {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(msgs) = data["messages"].as_array() {
                        for msg in msgs {
                            let subject = msg["subject"].as_str().unwrap_or("").to_lowercase();
                            let from = msg["from"].as_str().unwrap_or("");
                            let thread_id = msg["thread_id"].as_str().unwrap_or("");

                            // Skip non-colloquium, self-messages, or missing thread
                            if from == *agent || thread_id.is_empty() {
                                continue;
                            }
                            let is_colloquium = subject.contains("colloquium");
                            if !is_colloquium {
                                continue;
                            }

                            let content = msg["content"].as_str().unwrap_or("");

                            // Generate response via Ollama
                            info!("Generating response for {} on thread {}", agent, &thread_id[..8.min(thread_id.len())]);

                            let ollama_resp = client
                                .post(format!("{}/v1/chat/completions", ollama_url))
                                .json(&serde_json::json!({
                                    "model": model,
                                    "messages": [
                                        {"role": "system", "content": format!("You are {}, a member of the AstralMaris Council. Respond thoughtfully and concisely (200-400 words).", agent)},
                                        {"role": "user", "content": content}
                                    ],
                                    "temperature": 0.7
                                }))
                                .send()
                                .await;

                            if let Ok(resp) = ollama_resp {
                                if let Ok(data) = resp.json::<serde_json::Value>().await {
                                    let response_text = data["choices"][0]["message"]["content"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();

                                    if response_text.is_empty() || response_text == "[PASS]" {
                                        continue;
                                    }

                                    // Commitment detection
                                    let has_commitment = commitment_re.iter().any(|re| re.is_match(&response_text));
                                    if has_commitment {
                                        warn!("Commitment detected in {} response — HELD (not posting)", agent);
                                        continue;
                                    }

                                    // Post reply
                                    let tagged = format!(
                                        "{}\n\n--- *{} [autonomous voice | {}]* ---",
                                        response_text, agent, model
                                    );

                                    if !args.dry_run {
                                        let _ = client
                                            .post(format!("{}/api/thread/{}/reply", args.mq_url, thread_id))
                                            .header("Authorization", format!("Bearer {}", token))
                                            .header("Content-Type", "application/json")
                                            .json(&serde_json::json!({
                                                "from_agent": agent,
                                                "content": tagged,
                                                "intent": "discuss"
                                            }))
                                            .send()
                                            .await;
                                        info!("Posted response for {} to thread {}", agent, &thread_id[..8.min(thread_id.len())]);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        if once {
            break;
        }
        tokio::time::sleep(Duration::from_secs(45)).await;
    }

    Ok(())
}

// ============================================================================
// Token loading
// ============================================================================

fn load_token(path: &PathBuf, agent: &str) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|d| d["tokens"][agent].as_str().map(String::from))
        .unwrap_or_default()
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

    let args = Args::parse();

    match &args.command {
        Commands::Ingest { name, body, source, format: _, force } => {
            let content = match body {
                Some(b) => b.clone(),
                None => {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                    buf
                }
            };
            cmd_ingest(&args, name, &content, source, *force).await?;
        }
        Commands::Reingest { seed_only, use_cache, force, arxiv_ids } => {
            cmd_reingest(&args, *seed_only, *use_cache, *force, arxiv_ids).await?;
        }
        Commands::Refresh { once, source, tokens_file } => {
            cmd_refresh(&args, *once, source.as_deref(), tokens_file).await?;
        }
        Commands::Voice { once, agents, ollama_url, model, tokens_file } => {
            cmd_voice(&args, *once, agents, ollama_url, model, tokens_file).await?;
        }
    }

    Ok(())
}
