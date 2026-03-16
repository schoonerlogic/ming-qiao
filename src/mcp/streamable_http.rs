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
    model::{ServerCapabilities, ServerInfo, LoggingLevel, LoggingMessageNotificationParam},
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

/// Event published to agents via broadcast channels.
#[derive(Clone, Debug)]
pub struct PushEvent {
    pub from: String,
    pub subject: String,
    pub intent: String,
    pub message_id: String,
}

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
    pub async fn register_peer(&self, agent_id: &str, peer: Peer<RoleServer>) {
        // Store the peer
        self.peers.write().await.insert(agent_id.to_string(), peer.clone());

        // Subscribe to push events for this agent
        let mut rx = self.subscribe(agent_id).await;
        let agent = agent_id.to_string();

        // Spawn background task that forwards push events → peer notifications
        tokio::spawn(async move {
            info!("Push listener started for agent={}", agent);
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let data = serde_json::json!({
                            "type": "new_message",
                            "from": event.from,
                            "subject": event.subject,
                            "intent": event.intent,
                            "message_id": event.message_id,
                        });
                        let param = LoggingMessageNotificationParam {
                            level: LoggingLevel::Info,
                            logger: Some("ming-qiao".into()),
                            data,
                        };
                        if let Err(e) = peer.notify_logging_message(param).await {
                            warn!("Push notification failed for {}: {}", agent, e);
                            break; // Peer disconnected
                        }
                        info!("Push delivered to {}: {} re: {}", agent, event.from, event.subject);
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
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
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
