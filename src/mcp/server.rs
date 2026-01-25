//! MCP server implementation
//!
//! Handles the stdio transport and JSON-RPC message routing for the MCP protocol.
//! The server reads JSON-RPC requests from stdin, dispatches them to the appropriate
//! handlers, and writes responses to stdout.

use std::io::{self, BufRead, Write};

use serde_json::Value;
use tracing::{debug, error, info, warn};

use crate::mcp::protocol::{
    CallToolParams, InitializeParams, InitializeResult, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse, McpError, McpErrorCode, RequestId, ServerCapabilities, ServerInfo,
    ToolsCapability,
};
use crate::mcp::tools::ToolRegistry;

/// MCP protocol version we support
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server name
pub const SERVER_NAME: &str = "ming-qiao";

/// Server version
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP server that handles stdio transport
pub struct McpServer {
    /// Tool registry
    tools: ToolRegistry,

    /// Agent ID (from environment)
    agent_id: String,

    /// Whether the server has been initialized
    initialized: bool,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(agent_id: String) -> Self {
        Self {
            tools: ToolRegistry::new(),
            agent_id,
            initialized: false,
        }
    }

    /// Run the server, reading from stdin and writing to stdout
    pub async fn run(&mut self) -> Result<(), McpError> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        info!("MCP server starting for agent: {}", self.agent_id);

        for line in stdin.lock().lines() {
            let line = line?;

            if line.is_empty() {
                continue;
            }

            debug!("Received: {}", line);

            let response = self.handle_message(&line).await;

            if let Some(resp) = response {
                let json = serde_json::to_string(&resp)?;
                debug!("Sending: {}", json);
                writeln!(stdout, "{}", json)?;
                stdout.flush()?;
            }
        }

        info!("MCP server shutting down");
        Ok(())
    }

    /// Handle a single JSON-RPC message
    async fn handle_message(&mut self, message: &str) -> Option<JsonRpcResponse> {
        // Parse the JSON
        let request: JsonRpcRequest = match serde_json::from_str(message) {
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

    /// Dispatch a request to the appropriate handler
    async fn dispatch(&mut self, request: &JsonRpcRequest) -> Result<Value, McpError> {
        match request.method.as_str() {
            // Lifecycle methods
            "initialize" => self.handle_initialize(request.params.clone()),
            "initialized" => {
                // Notification, no response needed but we return empty
                debug!("Client sent initialized notification");
                Ok(serde_json::json!({}))
            }
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

        let result = self
            .tools
            .call(&params.name, params.arguments, &self.agent_id)
            .await?;

        Ok(serde_json::to_value(result)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = McpServer::new("test-agent".to_string());
        assert_eq!(server.agent_id, "test-agent");
        assert!(!server.initialized);
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let mut server = McpServer::new("test".to_string());

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

    #[test]
    fn test_handle_tools_list() {
        let server = McpServer::new("test".to_string());
        let result = server.handle_tools_list().unwrap();

        // Should have a "tools" array
        assert!(result.get("tools").is_some());
    }
}
