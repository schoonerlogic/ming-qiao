//! MCP server implementation
//!
//! Handles the stdio transport and JSON-RPC message routing for the MCP protocol.
//! The server reads JSON-RPC requests from stdin, dispatches them to the appropriate
//! handlers, and writes responses to stdout.

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

use crate::events::{EventEnvelope, EventPayload, EventType};
use crate::mcp::protocol::{
    CallToolParams, InitializeParams, InitializeResult, JsonRpcError, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, McpError, McpErrorCode, RequestId, ServerCapabilities,
    ServerInfo, ToolContent, ToolsCapability,
};
use crate::mcp::tools::ToolRegistry;
use crate::state::AppState;

/// MCP protocol version we support
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server name
pub const SERVER_NAME: &str = "ming-qiao";

/// Server version
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Tools that reset the inbox check timestamp (agent is already looking at messages)
const INBOX_TOOLS: &[&str] = &["check_messages", "read_inbox", "read_message"];

/// MCP server that handles stdio transport
pub struct McpServer {
    /// Tool registry
    tools: ToolRegistry,

    /// Agent ID (from environment)
    agent_id: String,

    /// Whether the server has been initialized
    initialized: bool,

    /// Timestamp of last inbox check — messages newer than this are "new"
    last_inbox_check: Mutex<DateTime<Utc>>,
}

impl McpServer {
    /// Create a new MCP server backed by AppState.
    ///
    /// All tool operations use `state.persistence()` for writes and
    /// `state.indexer()` for reads.
    pub fn with_state(agent_id: String, state: AppState) -> Self {
        Self {
            tools: ToolRegistry::with_state(state),
            agent_id,
            initialized: false,
            last_inbox_check: Mutex::new(Utc::now()),
        }
    }

    /// Run the server, reading from async stdin and pushing notifications from the event channel.
    ///
    /// Uses `tokio::select!` to multiplex:
    /// 1. JSON-RPC requests from stdin
    /// 2. Event notifications from the broadcast channel (e.g. incoming messages)
    pub async fn run(&mut self, state: &AppState) -> Result<(), McpError> {
        let stdin = BufReader::new(tokio::io::stdin());
        let mut stdout = tokio::io::stdout();
        let mut lines = stdin.lines();
        let mut event_rx = state.subscribe_events();

        eprintln!("[ming-qiao] MCP server ready for agent: {}", self.agent_id);

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let line = match line {
                        Ok(Some(l)) => l,
                        Ok(None) => {
                            // stdin closed
                            break;
                        }
                        Err(e) => {
                            eprintln!("[ming-qiao] stdin read error: {}", e);
                            return Err(McpError::Io(e));
                        }
                    };

                    if line.is_empty() {
                        continue;
                    }

                    let response = self.handle_message(&line).await;

                    if let Some(resp) = response {
                        let json = serde_json::to_string(&resp)?;
                        stdout.write_all(json.as_bytes()).await?;
                        stdout.write_all(b"\n").await?;
                        stdout.flush().await?;
                    }
                }
                event = event_rx.recv() => {
                    match event {
                        Ok(envelope) => {
                            if let Some(notification) = self.maybe_notify(&envelope) {
                                let json = serde_json::to_string(&notification)
                                    .expect("notification serialization");
                                // Best-effort write — don't break the loop on pipe errors
                                if let Err(e) = stdout.write_all(json.as_bytes()).await {
                                    eprintln!("[ming-qiao] notification write error: {}", e);
                                    continue;
                                }
                                let _ = stdout.write_all(b"\n").await;
                                let _ = stdout.flush().await;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            eprintln!("[ming-qiao] event channel lagged by {} events", n);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            eprintln!("[ming-qiao] event channel closed");
                            break;
                        }
                    }
                }
            }
        }

        eprintln!("[ming-qiao] MCP server shutting down");
        Ok(())
    }

    /// Build an MCP `notifications/message` if this event is a message for our agent.
    ///
    /// Returns `None` (no notification) when:
    /// - Server is not yet initialized
    /// - Event is not `MessageSent`
    /// - Message is not addressed to our agent
    /// - Message is from our own agent (echo suppression)
    /// Check if a message recipient matches this agent (direct, "all", or "council").
    fn is_addressed_to_me(&self, to: &str) -> bool {
        to == self.agent_id || to == "all" || to == "council"
    }

    fn maybe_notify(&self, envelope: &EventEnvelope) -> Option<JsonRpcNotification> {
        if !self.initialized {
            return None;
        }
        if envelope.event_type != EventType::MessageSent {
            return None;
        }
        let msg = match &envelope.payload {
            EventPayload::Message(m) => m,
            _ => return None,
        };
        if !self.is_addressed_to_me(&msg.to) {
            return None;
        }
        if msg.from == self.agent_id {
            return None;
        }

        let level = match msg.priority {
            crate::events::Priority::Low | crate::events::Priority::Normal => "info",
            crate::events::Priority::High => "warning",
            crate::events::Priority::Critical => "error",
        };

        Some(JsonRpcNotification::new(
            "notifications/message",
            Some(serde_json::json!({
                "level": level,
                "logger": "ming-qiao",
                "data": {
                    "type": "message_received",
                    "message_id": envelope.id.to_string(),
                    "thread_id": msg.thread_id,
                    "from": msg.from,
                    "subject": msg.subject,
                    "priority": msg.priority,
                    "intent": msg.intent,
                    "timestamp": envelope.timestamp.to_rfc3339(),
                }
            })),
        ))
    }

    /// Handle a single JSON-RPC message
    async fn handle_message(&mut self, message: &str) -> Option<JsonRpcResponse> {
        // Parse as raw JSON first to distinguish requests from notifications.
        // JSON-RPC notifications have no "id" field and MUST NOT receive a response.
        let raw: Value = match serde_json::from_str(message) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to parse JSON: {}", e);
                return Some(JsonRpcResponse::error(
                    RequestId::Null,
                    JsonRpcError {
                        code: McpErrorCode::ParseError.code(),
                        message: format!("Parse error: {}", e),
                        data: None,
                    },
                ));
            }
        };

        // No "id" field → JSON-RPC notification → no response allowed
        if raw.get("id").is_none() {
            match serde_json::from_value::<JsonRpcNotification>(raw) {
                Ok(notification) => self.handle_notification(&notification),
                Err(e) => eprintln!("[ming-qiao] Failed to parse notification: {}", e),
            }
            return None;
        }

        // Has "id" → JSON-RPC request → must respond
        let request: JsonRpcRequest = match serde_json::from_value(raw) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse request: {}", e);
                return Some(JsonRpcResponse::error(
                    RequestId::Null,
                    JsonRpcError {
                        code: McpErrorCode::ParseError.code(),
                        message: format!("Parse error: {}", e),
                        data: None,
                    },
                ));
            }
        };

        // Validate JSON-RPC version
        if request.jsonrpc != "2.0" {
            return Some(JsonRpcResponse::error(
                request.id,
                McpErrorCode::InvalidRequest.into(),
            ));
        }

        // Route to handler
        let result = self.dispatch(&request).await;

        match result {
            Ok(value) => Some(JsonRpcResponse::success(request.id, value)),
            Err(e) => Some(JsonRpcResponse::error(request.id, e.to_rpc_error())),
        }
    }

    /// Handle a JSON-RPC notification (no response allowed)
    fn handle_notification(&mut self, notification: &JsonRpcNotification) {
        match notification.method.as_str() {
            "initialized" => {
                debug!("Client acknowledged initialization");
            }
            "notifications/cancelled" => {
                debug!("Client cancelled a request");
            }
            _ => {
                warn!("Unknown notification: {}", notification.method);
            }
        }
    }

    /// Dispatch a request to the appropriate handler
    async fn dispatch(&mut self, request: &JsonRpcRequest) -> Result<Value, McpError> {
        match request.method.as_str() {
            // Lifecycle methods
            "initialize" => self.handle_initialize(request.params.clone()),
            "shutdown" => {
                info!("Shutdown requested");
                Ok(serde_json::json!(null))
            }

            // Tool methods
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(request.params.clone()).await,

            // Unknown method
            _ => {
                warn!("Unknown method: {}", request.method);
                Err(McpError::InvalidInput(format!(
                    "Unknown method: {}",
                    request.method
                )))
            }
        }
    }

    /// Handle initialize request
    fn handle_initialize(&mut self, params: Option<Value>) -> Result<Value, McpError> {
        let params: InitializeParams = match params {
            Some(p) => serde_json::from_value(p)?,
            None => {
                return Err(McpError::InvalidInput(
                    "Missing initialize params".to_string(),
                ))
            }
        };

        info!(
            "Initialize request from {} v{}",
            params.client_info.name, params.client_info.version
        );

        self.initialized = true;

        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: false,
                }),
                resources: None,
                prompts: None,
                logging: Some(serde_json::json!({})),
            },
            server_info: ServerInfo {
                name: SERVER_NAME.to_string(),
                version: SERVER_VERSION.to_string(),
            },
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Handle tools/list request
    fn handle_tools_list(&self) -> Result<Value, McpError> {
        let tools = self.tools.list();
        Ok(serde_json::json!({ "tools": tools }))
    }

    /// Handle tools/call request
    async fn handle_tools_call(&self, params: Option<Value>) -> Result<Value, McpError> {
        let params: CallToolParams = match params {
            Some(p) => serde_json::from_value(p)?,
            None => return Err(McpError::InvalidInput("Missing call params".to_string())),
        };

        info!("Tool call: {}", params.name);
        debug!("Arguments: {:?}", params.arguments);

        let mut result = self
            .tools
            .call(&params.name, params.arguments, &self.agent_id)
            .await?;

        if INBOX_TOOLS.contains(&params.name.as_str()) {
            // Agent is reading messages — reset the timestamp
            *self.last_inbox_check.lock().expect("poisoned") = Utc::now();
        } else if let Some(hint) = self.build_message_hint().await {
            result.content.push(ToolContent::Text { text: hint });
        }

        Ok(serde_json::to_value(result)?)
    }

    /// Build a hint string summarizing unread messages for this agent.
    ///
    /// Groups messages by intent (Request → Discuss → Inform) so the LLM
    /// sees actionable items first. Returns `None` if no new messages.
    async fn build_message_hint(&self) -> Option<String> {
        use crate::events::MessageIntent;

        let cutoff = *self.last_inbox_check.lock().expect("poisoned");

        let indexer = self.tools.state().indexer().await;
        let new_messages: Vec<_> = indexer
            .get_messages_to_agent(&self.agent_id)
            .into_iter()
            .filter(|m| m.created_at > cutoff)
            .collect();

        if new_messages.is_empty() {
            return None;
        }

        // Bucket messages by intent
        let mut requests: Vec<String> = Vec::new();
        let mut discussions: Vec<String> = Vec::new();
        let mut fyi: Vec<String> = Vec::new();

        for msg in &new_messages {
            let summary = format!("  - \"{}\" from {}", msg.subject, msg.from);
            match msg.intent {
                MessageIntent::Request => requests.push(summary),
                MessageIntent::Discuss => discussions.push(summary),
                MessageIntent::Inform => fyi.push(summary),
            }
        }

        let mut hint = format!(
            "\n---\n[Inbox: {} new message{}]",
            new_messages.len(),
            if new_messages.len() == 1 { "" } else { "s" }
        );

        if !requests.is_empty() {
            hint.push_str(&format!(
                "\n\u{26a0}\u{fe0f} INTERRUPT — {} request-intent message{} waiting:\n{}\nAction: Use check_messages to read and respond BEFORE continuing.",
                requests.len(),
                if requests.len() == 1 { "" } else { "s" },
                requests.join("\n")
            ));
        }
        if !discussions.is_empty() {
            hint.push_str(&format!(
                "\nDiscussion ({}):\n{}",
                discussions.len(),
                discussions.join("\n")
            ));
        }
        if !fyi.is_empty() {
            hint.push_str(&format!(
                "\nFYI ({}):\n{}",
                fyi.len(),
                fyi.join("\n")
            ));
        }

        Some(hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_creation() {
        let state = AppState::new().await;
        let server = McpServer::with_state("test-agent".to_string(), state);
        assert_eq!(server.agent_id, "test-agent");
        assert!(!server.initialized);
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("test".to_string(), state);

        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        });

        let result = server.handle_initialize(Some(params)).unwrap();
        assert!(server.initialized);

        let result: InitializeResult = serde_json::from_value(result).unwrap();
        assert_eq!(result.protocol_version, PROTOCOL_VERSION);
        assert_eq!(result.server_info.name, SERVER_NAME);
    }

    #[tokio::test]
    async fn test_handle_tools_list() {
        let state = AppState::new().await;
        let server = McpServer::with_state("test".to_string(), state);
        let result = server.handle_tools_list().unwrap();

        assert!(result.get("tools").is_some());
    }

    #[tokio::test]
    async fn test_initialize_enables_logging_capability() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("test".to_string(), state);

        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1.0.0" }
        });

        let result = server.handle_initialize(Some(params)).unwrap();
        let result: InitializeResult = serde_json::from_value(result).unwrap();
        assert!(result.capabilities.logging.is_some(), "logging capability must be declared");
    }

    // ========================================================================
    // maybe_notify tests
    // ========================================================================

    fn make_message_event(from: &str, to: &str, subject: &str, priority: crate::events::Priority) -> EventEnvelope {
        make_message_event_with_intent(from, to, subject, priority, crate::events::MessageIntent::Inform)
    }

    fn make_message_event_with_intent(from: &str, to: &str, subject: &str, priority: crate::events::Priority, intent: crate::events::MessageIntent) -> EventEnvelope {
        EventEnvelope {
            id: uuid::Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: from.to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: from.to_string(),
                to: to.to_string(),
                subject: subject.to_string(),
                content: "test content".to_string(),
                thread_id: None,
                priority,
                intent,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
                claimed_source_model: None,
                claimed_source_runtime: None,
                claimed_source_mode: None,
                verified_source_model: None,
                verified_source_runtime: None,
                verified_source_mode: None,
                source_worktree: None,
                source_session_id: None,
                provenance_level: crate::events::ProvenanceLevel::default(),
                provenance_issuer: None,
            }),
        }
    }

    #[tokio::test]
    async fn test_maybe_notify_sends_for_matching_message() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let event = make_message_event("thales", "aleph", "Review request", crate::events::Priority::Normal);
        let notification = server.maybe_notify(&event);

        assert!(notification.is_some());
        let n = notification.unwrap();
        assert_eq!(n.method, "notifications/message");
        assert_eq!(n.jsonrpc, "2.0");

        let params = n.params.unwrap();
        assert_eq!(params["level"], "info");
        assert_eq!(params["logger"], "ming-qiao");
        assert_eq!(params["data"]["type"], "message_received");
        assert_eq!(params["data"]["from"], "thales");
        assert_eq!(params["data"]["subject"], "Review request");
    }

    #[tokio::test]
    async fn test_maybe_notify_suppresses_echo() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let event = make_message_event("aleph", "aleph", "Self-note", crate::events::Priority::Normal);
        assert!(server.maybe_notify(&event).is_none(), "should suppress self-messages");
    }

    #[tokio::test]
    async fn test_maybe_notify_skips_non_message_events() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let event = EventEnvelope {
            id: uuid::Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::DecisionRecorded,
            agent_id: "thales".to_string(),
            payload: EventPayload::Decision(crate::events::DecisionEvent {
                title: "test".to_string(),
                context: "ctx".to_string(),
                options: vec![],
                chosen: 0,
                rationale: "because".to_string(),
            }),
        };
        assert!(server.maybe_notify(&event).is_none(), "should skip non-message events");
    }

    #[tokio::test]
    async fn test_maybe_notify_skips_when_not_initialized() {
        let state = AppState::new().await;
        let server = McpServer::with_state("aleph".to_string(), state);
        // server.initialized is false

        let event = make_message_event("thales", "aleph", "Hello", crate::events::Priority::Normal);
        assert!(server.maybe_notify(&event).is_none(), "should skip when not initialized");
    }

    #[tokio::test]
    async fn test_maybe_notify_skips_messages_for_other_agents() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let event = make_message_event("thales", "luban", "Not for aleph", crate::events::Priority::Normal);
        assert!(server.maybe_notify(&event).is_none(), "should skip messages not addressed to us");
    }

    #[tokio::test]
    async fn test_maybe_notify_priority_mapping() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let cases = vec![
            (crate::events::Priority::Low, "info"),
            (crate::events::Priority::Normal, "info"),
            (crate::events::Priority::High, "warning"),
            (crate::events::Priority::Critical, "error"),
        ];

        for (priority, expected_level) in cases {
            let event = make_message_event("thales", "aleph", "test", priority.clone());
            let n = server.maybe_notify(&event).unwrap();
            let level = n.params.as_ref().unwrap()["level"].as_str().unwrap();
            assert_eq!(level, expected_level, "priority {:?} should map to level '{}'", priority, expected_level);
        }
    }

    #[tokio::test]
    async fn test_notification_includes_thread_id() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let mut event = make_message_event("thales", "aleph", "Threaded", crate::events::Priority::Normal);
        if let EventPayload::Message(ref mut m) = event.payload {
            m.thread_id = Some("thread-abc-123".to_string());
        }

        let n = server.maybe_notify(&event).unwrap();
        let data = &n.params.as_ref().unwrap()["data"];
        assert_eq!(data["thread_id"], "thread-abc-123");
    }

    // ========================================================================
    // Broadcast notification tests (to: "all" and "council")
    // ========================================================================

    #[tokio::test]
    async fn test_maybe_notify_fires_for_all_broadcast() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let event = make_message_event("thales", "all", "Broadcast", crate::events::Priority::Normal);
        assert!(server.maybe_notify(&event).is_some(), "should notify for to='all'");
    }

    #[tokio::test]
    async fn test_maybe_notify_fires_for_council_broadcast() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let event = make_message_event("laozi-jung", "council", "Observation", crate::events::Priority::Normal);
        assert!(server.maybe_notify(&event).is_some(), "should notify for to='council'");
    }

    #[tokio::test]
    async fn test_maybe_notify_includes_intent() {
        let state = AppState::new().await;
        let mut server = McpServer::with_state("aleph".to_string(), state);
        server.initialized = true;

        let event = make_message_event_with_intent(
            "thales", "aleph", "Review PR",
            crate::events::Priority::High,
            crate::events::MessageIntent::Request,
        );
        let n = server.maybe_notify(&event).unwrap();
        let data = &n.params.as_ref().unwrap()["data"];
        assert_eq!(data["intent"], "request");
    }

    // ========================================================================
    // build_message_hint tests
    // ========================================================================

    #[tokio::test]
    async fn test_build_message_hint_groups_by_intent() {
        let state = AppState::new().await;
        let server = McpServer::with_state("aleph".to_string(), state.clone());

        // Reset inbox check to the past so all messages count as "new"
        *server.last_inbox_check.lock().unwrap() = chrono::DateTime::<Utc>::MIN_UTC;

        // Store messages with different intents
        let tools = &server.tools;
        for (from, subject, intent) in [
            ("luban", "Review my PR", crate::events::MessageIntent::Request),
            ("thales", "Architecture question", crate::events::MessageIntent::Request),
            ("thales", "Proposal: new schema", crate::events::MessageIntent::Discuss),
            ("luban", "Session started", crate::events::MessageIntent::Inform),
            ("laozi-jung", "[observe:scan] Weekly scan", crate::events::MessageIntent::Inform),
        ] {
            let event = EventEnvelope {
                id: uuid::Uuid::now_v7(),
                timestamp: Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: from.to_string(),
                payload: EventPayload::Message(crate::events::MessageEvent {
                    from: from.to_string(),
                    to: "aleph".to_string(),
                    subject: subject.to_string(),
                    content: "test".to_string(),
                    thread_id: None,
                    priority: crate::events::Priority::Normal,
                    intent,
                    expected_response: crate::events::ExpectedResponse::None,
                    require_ack: false,
                    claimed_source_model: None,
                    claimed_source_runtime: None,
                    claimed_source_mode: None,
                    verified_source_model: None,
                    verified_source_runtime: None,
                    verified_source_mode: None,
                    source_worktree: None,
                    source_session_id: None,
                    provenance_level: crate::events::ProvenanceLevel::default(),
                    provenance_issuer: None,
                }),
            };
            let mut indexer = tools.state().indexer_mut().await;
            indexer.process_event(&event).unwrap();
        }

        let hint = server.build_message_hint().await.unwrap();

        // Verify structure
        assert!(hint.contains("[Inbox: 5 new messages]"), "hint: {}", hint);
        assert!(hint.contains("INTERRUPT"), "hint: {}", hint);
        assert!(hint.contains("2 request-intent messages waiting"), "hint: {}", hint);
        assert!(hint.contains("Use check_messages to read and respond BEFORE continuing"), "hint: {}", hint);
        assert!(hint.contains("Discussion (1):"), "hint: {}", hint);
        assert!(hint.contains("FYI (2):"), "hint: {}", hint);

        // Verify INTERRUPT appears before Discussion, which appears before FYI
        let interrupt_pos = hint.find("INTERRUPT").unwrap();
        let discuss_pos = hint.find("Discussion").unwrap();
        let fyi_pos = hint.find("FYI").unwrap();
        assert!(interrupt_pos < discuss_pos, "INTERRUPT should come before Discussion");
        assert!(discuss_pos < fyi_pos, "Discussion should come before FYI");
    }
}
