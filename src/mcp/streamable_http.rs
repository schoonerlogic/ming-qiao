//! MCP Streamable HTTP transport via rmcp crate
//!
//! Replaces the hand-rolled transport with rmcp's `StreamableHttpService`.
//! Tools are defined using rmcp's `#[tool]` macro and `ServerHandler` trait.

use std::future::Future;
use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::{Parameters, Extension}},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig,
        tower::StreamableHttpService,
    },
};
use serde::Deserialize;

use crate::state::AppState;

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
// Helper: call existing tool registry and extract text result
// ============================================================================

/// Resolve agent ID from HTTP request parts injected by rmcp into extensions.
fn resolve_agent_from_parts(parts: &http::request::Parts, state: &AppState) -> Option<String> {
    let auth = parts.headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    // Parse agent name from token format: mq-{agent}-{hash}
    if token.starts_with("mq-") {
        let rest = &token[3..]; // Skip "mq-"
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
    pub fn new(state: AppState, agent_id: String) -> Self {
        Self {
            state,
            agent_id,
            tool_router: Self::tool_router(),
        }
    }

    /// Resolve agent ID from HTTP request parts or fall back to stored agent_id.
    fn resolve_agent(&self, parts: Option<&http::request::Parts>) -> String {
        if let Some(parts) = parts {
            if let Some(agent) = resolve_agent_from_parts(parts, &self.state) {
                return agent;
            }
        }
        self.agent_id.clone()
    }
}

#[tool_router]
impl MingQiaoMcpHandler {
    #[tool(description = "Send a message to another agent")]
    async fn send_message(&self, Extension(parts): Extension<http::request::Parts>, Parameters(p): Parameters<SendMessageParams>) -> String {
        let agent = self.resolve_agent(Some(&parts));
        call_tool(&self.state, "send_message", serde_json::json!({
            "to": p.to, "subject": p.subject, "content": p.content,
            "intent": p.intent, "priority": p.priority, "thread_id": p.thread_id,
        }), &agent).await
    }

    #[tool(description = "Check inbox for new messages")]
    async fn check_messages(&self, Extension(parts): Extension<http::request::Parts>, Parameters(p): Parameters<CheckMessagesParams>) -> String {
        let agent = self.resolve_agent(Some(&parts));
        call_tool(&self.state, "check_messages", serde_json::json!({
            "unread_only": p.unread_only, "from_agent": p.from_agent, "limit": p.limit,
        }), &agent).await
    }

    #[tool(description = "Mark messages as read/acknowledged. Call after processing messages.")]
    async fn acknowledge_messages(&self, Extension(parts): Extension<http::request::Parts>, Parameters(p): Parameters<AcknowledgeMessagesParams>) -> String {
        let agent = self.resolve_agent(Some(&parts));
        call_tool(&self.state, "acknowledge_messages", serde_json::json!({
            "message_id": p.message_id,
        }), &agent).await
    }

    #[tool(description = "Read all messages in a thread")]
    async fn read_thread(&self, Extension(parts): Extension<http::request::Parts>, Parameters(p): Parameters<ReadThreadParams>) -> String {
        let agent = self.resolve_agent(Some(&parts));
        call_tool(&self.state, "read_thread", serde_json::json!({
            "thread_id": p.thread_id,
        }), &agent).await
    }

    #[tool(description = "Reply to an existing thread")]
    async fn reply_to_thread(&self, Extension(parts): Extension<http::request::Parts>, Parameters(p): Parameters<ReplyToThreadParams>) -> String {
        let agent = self.resolve_agent(Some(&parts));
        call_tool(&self.state, "reply_to_thread", serde_json::json!({
            "thread_id": p.thread_id, "content": p.content,
            "intent": p.intent, "priority": p.priority,
        }), &agent).await
    }

    #[tool(description = "List all threads")]
    async fn list_threads(&self, Extension(parts): Extension<http::request::Parts>) -> String {
        let agent = self.resolve_agent(Some(&parts));
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
}

// ============================================================================
// Axum integration
// ============================================================================

/// Create the StreamableHttpService for nesting under `/mcp` in Axum.
pub fn create_mcp_service(state: AppState) -> StreamableHttpService<MingQiaoMcpHandler> {
    StreamableHttpService::new(
        move || Ok(MingQiaoMcpHandler::new(state.clone(), "unknown".to_string())),
        Default::default(),
        StreamableHttpServerConfig {
            stateful_mode: true,
            sse_keep_alive: Some(std::time::Duration::from_secs(15)),
        },
    )
}

// ============================================================================
// Backward compat stubs (for AppState and handlers.rs references)
// ============================================================================

use std::collections::HashMap;
use tokio::sync::RwLock;

pub type SessionStore = Arc<RwLock<HashMap<String, ()>>>;
pub fn new_session_store() -> SessionStore { Arc::new(RwLock::new(HashMap::new())) }
pub struct McpSession;
pub struct SseEvent;

pub async fn push_message_notification(
    _sessions: &SessionStore, _to: &str, _eid: &str, _from: &str, _subj: &str, _intent: &str,
) {}
