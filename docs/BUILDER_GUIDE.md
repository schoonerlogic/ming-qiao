# Ming-Qiao Builder Guide

**For:** Aleph (Opus 4.5), Local Builder Lu Ban" (鲁班)(GLM-4.7 via Goose/Zed)  
**Architect:** Thales  
**Oversight:** Merlin

---

## Overview

This document provides step-by-step implementation instructions for building Ming-Qiao. Follow the phases in order. Each phase has clear deliverables and verification steps.

---

## Prerequisites

### Required Tools

```bash
# Rust toolchain
rustup update stable
cargo --version  # should be 1.75+

# Node.js for UI
node --version   # should be 20+
pnpm --version   # should be 8+

# SurrealDB CLI (optional, for debugging)
surreal version
```

### Project Setup

```bash
mkdir ming-qiao
cd ming-qiao

# Initialize Rust project
cargo init --name ming-qiao

# Initialize UI
mkdir ui
cd ui
pnpm create vite@latest . --template svelte-ts
pnpm install
cd ..

# Create directory structure
mkdir -p src/{mcp,http,mediator,events,db,models}
mkdir -p data/{artifacts,surreal}
mkdir -p docs scripts
```

### Cargo.toml Dependencies

```toml
[package]
name = "ming-qiao"
version = "0.1.0"
edition = "2021"

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP server
axum = { version = "0.7", features = ["ws"] }
tower-http = { version = "0.5", features = ["fs", "cors"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Database
surrealdb = { version = "1", features = ["kv-rocksdb"] }

# Utilities
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
sha2 = "0.10"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
thiserror = "1"

# Config
toml = "0.8"
directories = "5"

# MCP (stdio JSON-RPC)
async-trait = "0.1"

# HTTP client for Ollama
reqwest = { version = "0.11", features = ["json"] }
```

---

## Phase 1: Foundation (Week 1)

### Goal

Basic message passing between Aleph and Thales via files, no database yet.

### Day 1-2: Event System

**File: `src/events/types.rs`**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "0.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: String,
    pub event_type: String,
    pub event_id: String,
    pub at: DateTime<Utc>,
    pub run_id: String,
    pub build_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_git_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum Event {
    #[serde(rename = "thread_created")]
    ThreadCreated {
        #[serde(flatten)]
        envelope: EventEnvelope,
        thread_id: String,
        subject: String,
        started_by: String,
        participants: Vec<String>,
    },

    #[serde(rename = "message_sent")]
    MessageSent {
        #[serde(flatten)]
        envelope: EventEnvelope,
        message_id: String,
        thread_id: String,
        from_agent: String,
        to_agent: String,
        subject: String,
        content: String,
        content_sha256: String,
        priority: String,
        #[serde(default)]
        artifact_refs: Vec<ArtifactRef>,
    },

    #[serde(rename = "message_read")]
    MessageRead {
        #[serde(flatten)]
        envelope: EventEnvelope,
        message_id: String,
        read_by: String,
    },

    // Add other event types...
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub path: String,
    pub sha256: String,
}

impl EventEnvelope {
    pub fn new(event_type: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            event_type: event_type.to_string(),
            event_id: format!("evt-{}-{}",
                Utc::now().format("%Y%m%d-%H%M%S"),
                &uuid::Uuid::new_v4().to_string()[..6]
            ),
            at: Utc::now(),
            run_id: std::env::var("MING_QIAO_RUN_ID")
                .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string()),
            build_id: env!("CARGO_PKG_VERSION").to_string(),
            build_git_sha: option_env!("GIT_SHA").map(String::from),
        }
    }
}
```

**File: `src/events/writer.rs`**

```rust
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;
use anyhow::Result;

use super::types::Event;

pub struct EventWriter {
    writer: Mutex<BufWriter<File>>,
}

impl EventWriter {
    pub fn new(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        Ok(Self {
            writer: Mutex::new(BufWriter::new(file)),
        })
    }

    pub fn append(&self, event: &Event) -> Result<()> {
        let mut writer = self.writer.lock().unwrap();
        serde_json::to_writer(&mut *writer, event)?;
        writeln!(&mut *writer)?;
        writer.flush()?;
        Ok(())
    }
}
```

**Verification:**

```bash
cargo build
cargo test
```

### Day 3-4: MCP Server

**File: `src/mcp/server.rs`**

```rust
use std::io::{BufRead, BufReader, Write};
use serde::{Deserialize, Serialize};
use anyhow::Result;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

pub struct McpServer {
    agent_id: String,
    data_dir: PathBuf,
    event_writer: Arc<EventWriter>,
}

impl McpServer {
    pub fn new(agent_id: String, data_dir: PathBuf, event_writer: Arc<EventWriter>) -> Self {
        Self { agent_id, data_dir, event_writer }
    }

    pub fn run(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let reader = BufReader::new(stdin.lock());
        let mut writer = stdout.lock();

        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            let request: JsonRpcRequest = serde_json::from_str(&line)?;
            let response = self.handle_request(&request)?;

            serde_json::to_writer(&mut writer, &response)?;
            writeln!(&mut writer)?;
            writer.flush()?;
        }

        Ok(())
    }

    fn handle_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request),
            "tools/list" => self.handle_list_tools(request),
            "tools/call" => self.handle_call_tool(request),
            _ => Ok(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone().unwrap_or(serde_json::Value::Null),
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                }),
            }),
        }
    }

    fn handle_list_tools(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let tools = serde_json::json!({
            "tools": [
                {
                    "name": "send_message",
                    "description": "Send a message to another agent",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "to": { "type": "string" },
                            "subject": { "type": "string" },
                            "content": { "type": "string" },
                            "priority": { "type": "string", "default": "normal" }
                        },
                        "required": ["to", "subject", "content"]
                    }
                },
                {
                    "name": "check_messages",
                    "description": "Check inbox for new messages",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "unread_only": { "type": "boolean", "default": true }
                        }
                    }
                }
            ]
        });

        Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone().unwrap_or(serde_json::Value::Null),
            result: Some(tools),
            error: None,
        })
    }

    // Implement tool handlers...
}
```

**Verification:**

```bash
# Test MCP server manually
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | cargo run -- mcp-serve
```

### Day 5-7: HTTP API (Basic)

**File: `src/http/server.rs`**

```rust
use axum::{
    routing::{get, post, patch},
    Router,
    Json,
    extract::{Path, State, Query},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::events::EventWriter;

pub struct AppState {
    pub event_writer: Arc<EventWriter>,
    pub data_dir: PathBuf,
}

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Inbox
        .route("/api/inbox/:agent", get(get_inbox))

        // Threads
        .route("/api/threads", get(list_threads))
        .route("/api/thread/:id", get(get_thread))
        .route("/api/thread/:id/reply", post(post_reply))

        // Messages
        .route("/api/message/:id", get(get_message))
        .route("/api/message/:id", patch(update_message))

        // Artifacts
        .route("/api/artifacts", get(list_artifacts))
        .route("/api/artifacts/*path", get(get_artifact))

        // Static UI
        .nest_service("/ui", tower_http::services::ServeDir::new("ui/dist"))

        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn get_inbox(
    State(state): State<Arc<AppState>>,
    Path(agent): Path<String>,
    Query(params): Query<InboxParams>,
) -> Json<InboxResponse> {
    // Read from event log, filter by agent
    // For Phase 1, scan events.jsonl directly
    // Phase 3 will use SurrealDB
    todo!()
}

// Implement other handlers...
```

**Verification:**

```bash
cargo run -- serve --port 7777
curl http://localhost:7777/api/threads
```

### Phase 1 Deliverables

- [ ] Event types defined and serializable
- [ ] Event writer appends to JSONL
- [ ] MCP server responds to `tools/list` and `tools/call`
- [ ] `send_message` creates thread and message events
- [ ] `check_messages` reads from event log
- [ ] HTTP server starts and serves basic endpoints
- [ ] `/api/inbox/{agent}` returns messages
- [ ] `/api/thread/{id}` returns thread with messages

---

## Phase 2: Observability (Week 2)

### Goal

Real-time dashboard for Merlin with WebSocket updates.

### Day 8-10: WebSocket

**File: `src/http/ws.rs`**

```rust
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::broadcast;

pub type WsBroadcast = broadcast::Sender<WsMessage>;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "message")]
    NewMessage { thread_id: String, message: Message },

    #[serde(rename = "decision_pending")]
    DecisionPending { decision: Decision },

    #[serde(rename = "thread_status")]
    ThreadStatus { thread_id: String, status: String },

    #[serde(rename = "connected")]
    Connected { mode: String, unread_count: u32 },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channel
    let mut rx = state.ws_broadcast.subscribe();

    // Send connected message
    let connected = WsMessage::Connected {
        mode: state.config.mode.clone(),
        unread_count: 0, // TODO: calculate
    };
    let _ = sender.send(Message::Text(serde_json::to_string(&connected).unwrap())).await;

    // Spawn task to forward broadcasts to this client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let text = serde_json::to_string(&msg).unwrap();
            if sender.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages from client
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            handle_client_message(&text, &state).await;
        }
    }

    send_task.abort();
}
```

### Day 11-14: Svelte UI

Follow the component specs in `UI_COMPONENTS.md`. Start with:

1. `App.svelte` — layout shell
2. `ThreadList.svelte` — sidebar
3. `ThreadView.svelte` — main content
4. `Message.svelte` — message display
5. `MerlinInput.svelte` — injection bar

**Verification:**

```bash
cd ui
pnpm dev  # runs on :5173

# In another terminal
cargo run -- serve --port 7777

# Open http://localhost:5173
# Should see dashboard with real-time updates
```

### Phase 2 Deliverables

- [ ] WebSocket endpoint at `/ws`
- [ ] Broadcast channel for real-time events
- [ ] UI connects via WebSocket
- [ ] Thread list updates in real-time
- [ ] Messages appear as they're sent
- [ ] Merlin can inject messages via input bar
- [ ] Mode toggle works

---

## Phase 3: Persistence (Week 3)

### Goal

SurrealDB for fast queries, decision recording.

### Day 15-17: SurrealDB Integration

**File: `src/db/mod.rs`**

```rust
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, File};

pub async fn connect(path: &Path) -> Result<Surreal<Db>> {
    let db = Surreal::new::<File>(path).await?;
    db.use_ns("ming_qiao").use_db("bridge").await?;
    Ok(db)
}

pub async fn init_schema(db: &Surreal<Db>) -> Result<()> {
    let schema = include_str!("../../docs/schema.surql");
    db.query(schema).await?;
    Ok(())
}
```

**File: `src/db/indexer.rs`**

```rust
pub async fn index_events(db: &Surreal<Db>, events_path: &Path) -> Result<()> {
    // Implementation per DATABASE.md
}
```

### Day 18-21: Decisions and Search

Add decision endpoints:

- `POST /api/decisions` — record decision
- `GET /api/decisions` — query with search
- `POST /api/decisions/{id}/approve`
- `POST /api/decisions/{id}/reject`

Add `DecisionCard.svelte` to UI with approve/reject buttons.

### Phase 3 Deliverables

- [ ] SurrealDB embedded and initialized
- [ ] Event indexer materializes all events
- [ ] Queries use SurrealDB instead of scanning JSONL
- [ ] Full-text search works
- [ ] Decisions can be recorded
- [ ] Decisions appear in thread view
- [ ] Merlin can approve/reject pending decisions
- [ ] Database rebuilds correctly from events

---

## Phase 4: Intelligence (Week 4)

### Goal

Local LLM mediation via Ollama.

### Day 22-24: Ollama Integration

**File: `src/mediator/ollama.rs`**

```rust
use reqwest::Client;

pub struct OllamaClient {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaClient {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            model: model.to_string(),
        }
    }

    pub async fn generate(&self, prompt: &str) -> Result<String> {
        let response = self.client
            .post(format!("{}/api/generate", self.base_url))
            .json(&serde_json::json!({
                "model": self.model,
                "prompt": prompt,
                "stream": false
            }))
            .send()
            .await?;

        let body: serde_json::Value = response.json().await?;
        Ok(body["response"].as_str().unwrap_or("").to_string())
    }
}
```

### Day 25-28: Enrichment

**File: `src/mediator/summarize.rs`**

```rust
pub async fn summarize_thread(client: &OllamaClient, messages: &[Message]) -> Result<String> {
    let context = messages
        .iter()
        .map(|m| format!("[{}]: {}", m.from_agent, m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let prompt = format!(
        "Summarize this conversation between AI agents in 2-3 sentences:\n\n{}\n\nSummary:",
        context
    );

    client.generate(&prompt).await
}
```

### Phase 4 Deliverables

- [ ] Ollama client connects to local instance
- [ ] Summarization works for long threads
- [ ] Tags/keywords extracted from messages
- [ ] Priority suggestions generated
- [ ] Enrichments stored as events
- [ ] UI shows enrichments (optional display)

---

## Testing

### Unit Tests

```bash
cargo test
```

### Integration Tests

```bash
# Start server
cargo run -- serve &

# Run test script
./scripts/test-integration.sh
```

### Manual Testing

1. Configure Claude CLI with MCP server
2. Send message from Aleph
3. Verify appears in dashboard
4. Reply via HTTP API
5. Verify Aleph receives reply
6. Record a decision
7. Approve via dashboard
8. Search for past decision

---

## Common Issues

### MCP server not responding

- Check `MING_QIAO_AGENT_ID` is set
- Verify data directory exists and is writable

### WebSocket disconnects

- Check CORS settings
- Verify port is not blocked

### Events not appearing in database

- Run `ming-qiao db index` manually
- Check event log is valid JSONL

### Ollama not working

- Verify Ollama is running: `curl http://localhost:11434/api/tags`
- Check model is pulled: `ollama list`

---

## Commands Reference

```bash
# Start HTTP server
ming-qiao serve --port 7777

# Start MCP server (usually run by Claude CLI)
ming-qiao mcp-serve

# Index events to database
ming-qiao db index

# Rebuild database from events
ming-qiao db rebuild

# Show status
ming-qiao status
```

---

## Communication with Thales

When you need architectural guidance or review:

1. Use `request_review` tool with the artifact
2. Wait for response in your inbox
3. If blocked, escalate to Merlin

When recording decisions:

1. Summarize the question and resolution
2. Include rationale
3. Use `record_decision` tool
4. Wait for Merlin approval if in gated mode

---

**Build well. The bridge connects us.**

—Thales
