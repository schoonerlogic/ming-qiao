//! MCP Streamable HTTP transport via rmcp crate
//!
//! Tools are defined using rmcp's `#[tool]` macro and `ServerHandler` trait.
//! Push delivery: JetStream → PushBroker → Peer.notify_logging_message() → SSE.

use std::future::Future;
use std::sync::Arc;
use std::collections::HashMap;

use rmcp::{
    RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::{Parameters, Extension}},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig,
        tower::StreamableHttpService,
    },
    Peer,
};
use serde::Deserialize;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn, debug};

use crate::state::AppState;

// ============================================================================
// PushBroker — bridges JetStream messages to MCP SSE push
// ============================================================================

/// Lightweight wake signal — claim check pattern.
/// Agents call `check_messages` to pull actual content from SurrealDB.
#[derive(Clone, Debug)]
pub struct PushEvent;

/// Manages per-agent broadcast channels and peer handles for push delivery.
pub struct PushBroker {
    /// Per-agent broadcast senders (JetStream ingester writes here)
    senders: RwLock<HashMap<String, broadcast::Sender<PushEvent>>>,
    /// Per-agent Peer handles (captured on first tool call, used for push)
    peers: RwLock<HashMap<String, Peer<RoleServer>>>,
}

impl PushBroker {
    pub fn new() -> Self {
        Self {
            senders: RwLock::new(HashMap::new()),
            peers: RwLock::new(HashMap::new()),
        }
    }

    /// Subscribe to push events for an agent. Creates channel if needed.
    pub async fn subscribe(&self, agent_id: &str) -> broadcast::Receiver<PushEvent> {
        let mut senders = self.senders.write().await;
        let tx = senders.entry(agent_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0);
        tx.subscribe()
    }

    /// Publish a push event to an agent's channel.
    pub async fn publish(&self, agent_id: &str, event: PushEvent) {
        let senders = self.senders.read().await;
        if let Some(tx) = senders.get(agent_id) {
            match tx.send(event) {
                Ok(n) => debug!("PushBroker: delivered to {} ({} receivers)", agent_id, n),
                Err(_) => debug!("PushBroker: no receivers for {}", agent_id),
            }
        }
    }

    /// Register a peer handle for an agent. Starts the push listener task.
    /// Sends one immediate wake signal on connect so the agent polls for missed messages.
    pub async fn register_peer(&self, agent_id: &str, peer: Peer<RoleServer>) {
        // Store the peer
        self.peers.write().await.insert(agent_id.to_string(), peer.clone());

        // Subscribe to push events for this agent
        let mut rx = self.subscribe(agent_id).await;
        let agent = agent_id.to_string();

        // Spawn background task that forwards push events → peer notifications
        tokio::spawn(async move {
            info!("Push listener started for agent={}", agent);

            // Immediate wake on connect — agent polls check_messages for anything missed
            let resource_uri = format!("agent://{}/messages", agent);
            if let Err(e) = peer.notify_resource_updated(
                rmcp::model::ResourceUpdatedNotificationParam {
                    uri: resource_uri.clone(),
                }
            ).await {
                warn!("Initial wake notification failed for {}: {}", agent, e);
                return;
            }
            info!("Initial wake signal sent to {} (poll for missed messages)", agent);

            loop {
                match rx.recv().await {
                    Ok(_event) => {
                        // Lightweight wake signal — just notify resource changed
                        // Agent calls check_messages to pull actual content (claim check)
                        if let Err(e) = peer.notify_resource_updated(
                            rmcp::model::ResourceUpdatedNotificationParam {
                                uri: resource_uri.clone(),
                            }
                        ).await {
                            warn!("Wake notification failed for {}: {}", agent, e);
                            break; // Peer disconnected
                        }
                        info!("Wake signal pushed to {}", agent);
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Push listener lagged {} events for {}", n, agent);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Push channel closed for {}", agent);
                        break;
                    }
                }
            }
            info!("Push listener ended for agent={}", agent);
        });
    }

    /// Check if a peer is registered for an agent.
    pub async fn has_peer(&self, agent_id: &str) -> bool {
        self.peers.read().await.contains_key(agent_id)
    }
}

// ============================================================================
// Tool parameter types
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SendMessageParams {
    /// Target agent ID
    pub to: String,
    /// Message subject
    pub subject: String,
    /// Message content
    pub content: String,
    /// Message intent: inform, request, discuss
    #[serde(default = "default_inform")]
    pub intent: String,
    /// Priority: low, normal, high, critical
    #[serde(default = "default_normal")]
    pub priority: String,
    /// Optional thread ID to reply to
    pub thread_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckMessagesParams {
    /// Only return unread messages
    #[serde(default = "default_true")]
    pub unread_only: bool,
    /// Filter by sender agent ID
    pub from_agent: Option<String>,
    /// Maximum messages to return
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AcknowledgeMessagesParams {
    /// ID of the newest message to acknowledge
    pub message_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadThreadParams {
    /// Thread ID (UUID)
    pub thread_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplyToThreadParams {
    /// Thread ID to reply to
    pub thread_id: String,
    /// Reply content
    pub content: String,
    /// Intent: inform, request, discuss
    #[serde(default = "default_inform")]
    pub intent: String,
    /// Priority: low, normal, high, critical
    #[serde(default = "default_normal")]
    pub priority: String,
}

fn default_inform() -> String { "inform".to_string() }
fn default_normal() -> String { "normal".to_string() }
fn default_true() -> bool { true }
fn default_limit() -> usize { 10 }

// ============================================================================
// Helpers
// ============================================================================

fn resolve_agent_from_parts(parts: &http::request::Parts) -> Option<String> {
    let auth = parts.headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    if token.starts_with("mq-") {
        let rest = &token[3..];
        if let Some(dash_pos) = rest.find('-') {
            return Some(rest[..dash_pos].to_string());
        }
    }
    None
}

async fn call_tool(state: &AppState, tool_name: &str, args: serde_json::Value, agent_id: &str) -> String {
    let registry = crate::mcp::tools::ToolRegistry::with_state(state.clone());
    match registry.call(tool_name, args, agent_id).await {
        Ok(result) => result.content.into_iter()
            .filter_map(|c| match c {
                crate::mcp::protocol::ToolContent::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("Error: {}", e),
    }
}

// ============================================================================
// MCP Server Handler
// ============================================================================

#[derive(Clone)]
pub struct MingQiaoMcpHandler {
    state: AppState,
    agent_id: String,
    /// Whether we've captured the peer for push delivery
    peer_registered: Arc<tokio::sync::Mutex<bool>>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for MingQiaoMcpHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MingQiaoMcpHandler")
            .field("agent_id", &self.agent_id)
            .finish()
    }
}

impl MingQiaoMcpHandler {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            agent_id: "unknown".to_string(),
            peer_registered: Arc::new(tokio::sync::Mutex::new(false)),
            tool_router: Self::tool_router(),
        }
    }

    fn resolve_agent(&self, parts: Option<&http::request::Parts>) -> String {
        if let Some(parts) = parts {
            if let Some(agent) = resolve_agent_from_parts(parts) {
                return agent;
            }
        }
        self.agent_id.clone()
    }

    /// Capture the Peer handle on first tool call for push delivery.
    async fn maybe_register_peer(&self, agent_id: &str, peer: &Peer<RoleServer>) {
        let mut registered = self.peer_registered.lock().await;
        if !*registered {
            let broker = self.state.push_broker();
            if !broker.has_peer(agent_id).await {
                broker.register_peer(agent_id, peer.clone()).await;
                info!("Captured peer for agent={} — push delivery active", agent_id);
            }
            *registered = true;
        }
    }
}

#[tool_router]
impl MingQiaoMcpHandler {
    #[tool(description = "Send a message to another agent")]
    async fn send_message(
        &self,
        peer: Peer<RoleServer>,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<SendMessageParams>,
    ) -> String {
        let agent = self.resolve_agent(Some(&parts));
        self.maybe_register_peer(&agent, &peer).await;
        call_tool(&self.state, "send_message", serde_json::json!({
            "to": p.to, "subject": p.subject, "content": p.content,
            "intent": p.intent, "priority": p.priority, "thread_id": p.thread_id,
        }), &agent).await
    }

    #[tool(description = "Check inbox for new messages")]
    async fn check_messages(
        &self,
        peer: Peer<RoleServer>,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<CheckMessagesParams>,
    ) -> String {
        let agent = self.resolve_agent(Some(&parts));
        self.maybe_register_peer(&agent, &peer).await;
        call_tool(&self.state, "check_messages", serde_json::json!({
            "unread_only": p.unread_only, "from_agent": p.from_agent, "limit": p.limit,
        }), &agent).await
    }

    #[tool(description = "Mark messages as read/acknowledged. Call after processing messages.")]
    async fn acknowledge_messages(
        &self,
        peer: Peer<RoleServer>,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<AcknowledgeMessagesParams>,
    ) -> String {
        let agent = self.resolve_agent(Some(&parts));
        self.maybe_register_peer(&agent, &peer).await;
        call_tool(&self.state, "acknowledge_messages", serde_json::json!({
            "message_id": p.message_id,
        }), &agent).await
    }

    #[tool(description = "Read all messages in a thread")]
    async fn read_thread(
        &self,
        peer: Peer<RoleServer>,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<ReadThreadParams>,
    ) -> String {
        let agent = self.resolve_agent(Some(&parts));
        self.maybe_register_peer(&agent, &peer).await;
        call_tool(&self.state, "read_thread", serde_json::json!({
            "thread_id": p.thread_id,
        }), &agent).await
    }

    #[tool(description = "Reply to an existing thread")]
    async fn reply_to_thread(
        &self,
        peer: Peer<RoleServer>,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(p): Parameters<ReplyToThreadParams>,
    ) -> String {
        let agent = self.resolve_agent(Some(&parts));
        self.maybe_register_peer(&agent, &peer).await;
        call_tool(&self.state, "reply_to_thread", serde_json::json!({
            "thread_id": p.thread_id, "content": p.content,
            "intent": p.intent, "priority": p.priority,
        }), &agent).await
    }

    #[tool(description = "List all threads")]
    async fn list_threads(
        &self,
        peer: Peer<RoleServer>,
        Extension(parts): Extension<http::request::Parts>,
    ) -> String {
        let agent = self.resolve_agent(Some(&parts));
        self.maybe_register_peer(&agent, &peer).await;
        call_tool(&self.state, "list_threads", serde_json::json!({}), &agent).await
    }
}

impl ServerHandler for MingQiaoMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Ming-Qiao: Communication bridge for the AstralMaris Council.".into()),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
            ..Default::default()
        }
    }

    fn list_prompts(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListPromptsResult, rmcp::Error>> + Send + '_ {
        use rmcp::model::{Prompt, ListPromptsResult};
        std::future::ready(Ok(ListPromptsResult {
            prompts: vec![
                Prompt::new("inbox_status", Some("Check for new messages — returns pending message notifications for this agent"), None::<Vec<rmcp::model::PromptArgument>>),
            ],
            next_cursor: None,
        }))
    }

    fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::GetPromptResult, rmcp::Error>> + Send + '_ {
        use rmcp::model::{GetPromptResult, PromptMessage, PromptMessageRole};
        let state = self.state.clone();
        let agent_id = self.agent_id.clone();
        async move {
            if request.name != "inbox_status" {
                return Err(rmcp::Error::invalid_params("Unknown prompt", None));
            }

            // Check for unread messages
            let agent = if agent_id != "unknown" { agent_id } else { "aleph".to_string() };
            let inbox_url = format!("http://localhost:7777/api/inbox/{}?unread_only=true&peek=true&limit=5", agent);
            let messages = match reqwest::Client::new()
                .get(&inbox_url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        body["messages"].as_array().cloned().unwrap_or_default()
                    } else {
                        vec![]
                    }
                }
                Err(_) => vec![],
            };

            if messages.is_empty() {
                return Ok(GetPromptResult {
                    description: Some("No pending messages".into()),
                    messages: vec![
                        PromptMessage::new_text(PromptMessageRole::User, "No new messages in inbox."),
                    ],
                });
            }

            let mut text = format!("You have {} unread message(s):\n\n", messages.len());
            for m in &messages {
                let from = m["from"].as_str().unwrap_or("?");
                let subject = m["subject"].as_str().unwrap_or("?");
                let intent = m["intent"].as_str().unwrap_or("inform");
                text.push_str(&format!("- From: {} | Subject: {} | Intent: {}\n", from, subject, intent));
            }
            text.push_str("\nCall check_messages to read the full content and respond.");

            Ok(GetPromptResult {
                description: Some(format!("{} unread message(s)", messages.len())),
                messages: vec![
                    PromptMessage::new_text(PromptMessageRole::User, text),
                ],
            })
        }
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListToolsResult, rmcp::Error>> + Send + '_ {
        let tools = self.tool_router.list_all();
        std::future::ready(Ok(rmcp::model::ListToolsResult {
            tools,
            next_cursor: None,
        }))
    }

    fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
        context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::CallToolResult, rmcp::Error>> + Send + '_ {
        let tool_context = rmcp::handler::server::tool::ToolCallContext {
            request_context: context,
            service: self,
            name: request.name,
            arguments: request.arguments,
        };
        async move {
            self.tool_router.call(tool_context).await.map_err(|e| {
                rmcp::Error::internal_error(e.to_string(), None)
            })
        }
    }

    fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListResourcesResult, rmcp::Error>> + Send + '_ {
        use rmcp::model::ListResourcesResult;
        let agent = if self.agent_id != "unknown" { self.agent_id.clone() } else { "aleph".to_string() };
        let resource = rmcp::model::RawResource {
            uri: format!("agent://{}/messages", agent),
            name: "Inbox Messages".to_string(),
            description: Some("Unread messages for this agent. Subscribe for real-time notifications.".into()),
            mime_type: Some("application/json".into()),
            size: None,
        };
        let self_resource = rmcp::model::RawResource {
            uri: "agent://self/messages".to_string(),
            name: "My Inbox (alias)".to_string(),
            description: Some("Alias for agent://{self}/messages — subscribe to this for push notifications.".into()),
            mime_type: Some("application/json".into()),
            size: None,
        };
        std::future::ready(Ok(ListResourcesResult {
            resources: vec![
                rmcp::model::Annotated { raw: resource, annotations: None },
                rmcp::model::Annotated { raw: self_resource, annotations: None },
            ],
            next_cursor: None,
        }))
    }

    fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ReadResourceResult, rmcp::Error>> + Send + '_ {
        use rmcp::model::{ReadResourceResult, ResourceContents};
        let state = self.state.clone();
        let agent = if self.agent_id != "unknown" { self.agent_id.clone() } else { "aleph".to_string() };
        async move {
            let expected_uri = format!("agent://{}/messages", agent);
            let self_uri = "agent://self/messages";
            if request.uri != expected_uri && request.uri != self_uri {
                return Err(rmcp::Error::invalid_params("Unknown resource URI", None));
            }

            let inbox_url = format!("http://localhost:7777/api/inbox/{}?unread_only=true&peek=true&limit=10", agent);
            let text = match reqwest::Client::new()
                .get(&inbox_url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let msgs = body["messages"].as_array().cloned().unwrap_or_default();
                        if msgs.is_empty() {
                            "[]".to_string()
                        } else {
                            // Return structured JSON for kimi's MoE routing
                            let structured: Vec<serde_json::Value> = msgs.iter().map(|m| {
                                serde_json::json!({
                                    "message_id": m["id"].as_str().unwrap_or(""),
                                    "from": m["from"].as_str().unwrap_or(""),
                                    "subject": m["subject"].as_str().unwrap_or(""),
                                    "body": m["content"].as_str().unwrap_or(""),
                                    "urgency": m["priority"].as_str().unwrap_or("normal"),
                                    "intent": m["intent"].as_str().unwrap_or("inform"),
                                    "timestamp": m["timestamp"].as_str().unwrap_or(""),
                                    "metadata": {}
                                })
                            }).collect();
                            serde_json::to_string_pretty(&structured).unwrap_or_else(|_| "[]".to_string())
                        }
                    } else {
                        "[]".to_string()
                    }
                }
                Err(e) => format!("[]"),
            };

            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(text, request.uri)],
            })
        }
    }

    fn subscribe(
        &self,
        request: rmcp::model::SubscribeRequestParam,
        context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), rmcp::Error>> + Send + '_ {
        let state = self.state.clone();
        let agent_id = self.agent_id.clone();

        async move {
            let uri = request.uri.to_string();

            // Resolve "self" in agent://self/messages to actual agent ID
            let resolved_agent = if uri.contains("self") {
                if agent_id != "unknown" { agent_id.clone() } else { "aleph".to_string() }
            } else {
                uri.strip_prefix("agent://")
                    .and_then(|s| s.split('/').next())
                    .unwrap_or(&agent_id)
                    .to_string()
            };

            info!("Resource subscription from {}: {} (resolved: {})", agent_id, uri, resolved_agent);

            // Spawn peer registration in background — return Ok immediately
            // so the subscribe response reaches the client before any async work
            let peer = context.peer.clone();
            let broker_state = state.clone();
            let agent_for_task = resolved_agent.clone();
            tokio::spawn(async move {
                let broker = broker_state.push_broker();
                if !broker.has_peer(&agent_for_task).await {
                    broker.register_peer(&agent_for_task, peer).await;
                    info!("Peer registered via subscribe for agent={}", agent_for_task);
                }
            });

            Ok(())
        }
    }
}

// ============================================================================
// Axum integration
// ============================================================================

pub fn create_mcp_service(state: AppState) -> StreamableHttpService<MingQiaoMcpHandler> {
    StreamableHttpService::new(
        move || Ok(MingQiaoMcpHandler::new(state.clone())),
        Default::default(),
        StreamableHttpServerConfig {
            stateful_mode: true,
            sse_keep_alive: Some(std::time::Duration::from_secs(15)),
        },
    )
}

// ============================================================================
// Backward compat stubs (for AppState references)
// ============================================================================

pub type SessionStore = Arc<RwLock<HashMap<String, ()>>>;
pub fn new_session_store() -> SessionStore { Arc::new(RwLock::new(HashMap::new())) }
pub struct McpSession;
pub struct SseEvent;

/// Push a message notification via the PushBroker.
/// Called from HTTP handlers when a message is created.
pub async fn push_message_notification(
    _sessions: &SessionStore,
    to_agent: &str,
    message_id: &str,
    from: &str,
    subject: &str,
    intent: &str,
) {
    // This stub is called from handlers.rs — the real push goes through PushBroker.
    // We can't access AppState here (it's not passed), so we'll wire PushBroker
    // directly in the handler. For now, log that the old path was called.
    debug!("push_message_notification stub called for {} (use PushBroker instead)", to_agent);
}
