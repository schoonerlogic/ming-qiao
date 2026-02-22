//! MCP server implementation
//!
//! Handles the stdio transport and JSON-RPC message routing for the MCP protocol.
//! The server reads JSON-RPC requests from stdin, dispatches them to the appropriate
//! handlers, and writes responses to stdout.

use std::io::{self, BufRead, Write};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{debug, error, info, warn};

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

    /// Run the server, reading from stdin and writing to stdout
    pub async fn run(&mut self) -> Result<(), McpError> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        eprintln!("[ming-qiao] MCP server ready for agent: {}", self.agent_id);

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
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
                writeln!(stdout, "{}", json)?;
                stdout.flush()?;
            }
        }

        eprintln!("[ming-qiao] MCP server shutting down (stdin closed)");
        Ok(())
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
                logging: None,
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
    /// Returns `None` if there are no new messages since the last inbox check.
    async fn build_message_hint(&self) -> Option<String> {
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

        // Group by (thread_id, from) → collect subjects
        let mut groups: std::collections::BTreeMap<(&str, &str), &str> =
            std::collections::BTreeMap::new();
        for msg in &new_messages {
            groups
                .entry((&msg.thread_id, &msg.from))
                .or_insert(&msg.subject);
        }

        let summaries: Vec<String> = groups
            .iter()
            .map(|((_tid, from), subject)| format!("\"{}\" from {}", subject, from))
            .collect();

        Some(format!(
            "\n---\n[New: {} message{} — {}]",
            new_messages.len(),
            if new_messages.len() == 1 { "" } else { "s" },
            summaries.join(", ")
        ))
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
}
