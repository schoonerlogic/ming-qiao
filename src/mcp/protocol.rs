//! JSON-RPC 2.0 protocol types for MCP
//!
//! MCP uses JSON-RPC 2.0 over stdio. This module defines the request/response
//! types and error handling.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol version, always "2.0"
    pub jsonrpc: String,

    /// Request ID (can be string or number)
    pub id: RequestId,

    /// Method name
    pub method: String,

    /// Method parameters (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Protocol version, always "2.0"
    pub jsonrpc: String,

    /// Request ID this is responding to
    pub id: RequestId,

    /// Result on success
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Error on failure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 notification (no response expected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// Protocol version, always "2.0"
    pub jsonrpc: String,

    /// Method name
    pub method: String,

    /// Method parameters (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// Request ID can be string, number, or null
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
    Null,
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,

    /// Human-readable message
    pub message: String,

    /// Additional error data (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcNotification {
    /// Create a new outbound notification (no `id` field — no response expected).
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

impl JsonRpcRequest {
    /// Create a new request
    pub fn new(id: impl Into<RequestId>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: RequestId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        RequestId::String(s)
    }
}

impl From<&str> for RequestId {
    fn from(s: &str) -> Self {
        RequestId::String(s.to_string())
    }
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        RequestId::Number(n)
    }
}

impl From<i32> for RequestId {
    fn from(n: i32) -> Self {
        RequestId::Number(n as i64)
    }
}

/// MCP-specific error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpErrorCode {
    // JSON-RPC standard errors
    ParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    InternalError,

    // MCP-specific errors
    NotFound,
    PermissionDenied,
    Conflict,
}

impl McpErrorCode {
    /// Get the numeric code
    pub fn code(&self) -> i32 {
        match self {
            McpErrorCode::ParseError => -32700,
            McpErrorCode::InvalidRequest => -32600,
            McpErrorCode::MethodNotFound => -32601,
            McpErrorCode::InvalidParams => -32602,
            McpErrorCode::InternalError => -32603,
            McpErrorCode::NotFound => -32001,
            McpErrorCode::PermissionDenied => -32002,
            McpErrorCode::Conflict => -32003,
        }
    }

    /// Get the standard message
    pub fn message(&self) -> &'static str {
        match self {
            McpErrorCode::ParseError => "Parse error",
            McpErrorCode::InvalidRequest => "Invalid request",
            McpErrorCode::MethodNotFound => "Method not found",
            McpErrorCode::InvalidParams => "Invalid params",
            McpErrorCode::InternalError => "Internal error",
            McpErrorCode::NotFound => "Not found",
            McpErrorCode::PermissionDenied => "Permission denied",
            McpErrorCode::Conflict => "Conflict",
        }
    }
}

impl From<McpErrorCode> for JsonRpcError {
    fn from(code: McpErrorCode) -> Self {
        Self {
            code: code.code(),
            message: code.message().to_string(),
            data: None,
        }
    }
}

/// MCP error type for use in tool handlers
#[derive(Debug, Error)]
pub enum McpError {
    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl McpError {
    /// Convert to JSON-RPC error
    pub fn to_rpc_error(&self) -> JsonRpcError {
        let (code, message) = match self {
            McpError::NotFound(msg) => (McpErrorCode::NotFound, msg.clone()),
            McpError::InvalidInput(msg) => (McpErrorCode::InvalidParams, msg.clone()),
            McpError::PermissionDenied(msg) => (McpErrorCode::PermissionDenied, msg.clone()),
            McpError::Conflict(msg) => (McpErrorCode::Conflict, msg.clone()),
            McpError::Internal(msg) => (McpErrorCode::InternalError, msg.clone()),
            McpError::Json(e) => (McpErrorCode::ParseError, e.to_string()),
            McpError::Io(e) => (McpErrorCode::InternalError, e.to_string()),
        };

        JsonRpcError {
            code: code.code(),
            message,
            data: None,
        }
    }
}

// ============================================================================
// MCP Protocol Messages
// ============================================================================

/// MCP initialize request params
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Protocol version
    pub protocol_version: String,

    /// Client capabilities
    pub capabilities: ClientCapabilities,

    /// Client info
    pub client_info: ClientInfo,
}

/// Client capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Experimental capabilities
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,

    /// Roots capability
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,

    /// Sampling capability
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
}

/// Roots capability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsCapability {
    pub list_changed: bool,
}

/// Client info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// MCP initialize response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Protocol version
    pub protocol_version: String,

    /// Server capabilities
    pub capabilities: ServerCapabilities,

    /// Server info
    pub server_info: ServerInfo,
}

/// Server capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Tools capability
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,

    /// Resources capability
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<Value>,

    /// Prompts capability
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Value>,

    /// Logging capability
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
}

/// Tools capability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    pub list_changed: bool,
}

/// Server info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Tool call request params
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// Tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Tool content item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text {
        text: String,
    },
    Image {
        data: String,
        mime_type: String,
    },
    Resource {
        uri: String,
        mime_type: Option<String>,
        text: Option<String>,
    },
}

impl CallToolResult {
    /// Create a text result
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: None,
        }
    }

    /// Create an error result
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: Some(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"tools/list\""));
    }

    #[test]
    fn test_response_success() {
        let resp = JsonRpcResponse::success(RequestId::Number(1), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_response_error() {
        let resp = JsonRpcResponse::error(RequestId::Number(1), McpErrorCode::NotFound.into());
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn test_request_id_variants() {
        let id_str: RequestId = "abc".into();
        let id_num: RequestId = 123i32.into();

        assert_eq!(id_str, RequestId::String("abc".to_string()));
        assert_eq!(id_num, RequestId::Number(123));
    }

    #[test]
    fn test_tool_content_text() {
        let content = ToolContent::Text {
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_call_tool_result() {
        let result = CallToolResult::text("Success!");
        assert!(result.is_error.is_none());
        assert_eq!(result.content.len(), 1);

        let error = CallToolResult::error("Failed!");
        assert_eq!(error.is_error, Some(true));
    }

    #[test]
    fn test_notification_new() {
        let n = JsonRpcNotification::new(
            "notifications/message",
            Some(serde_json::json!({"level": "info", "logger": "ming-qiao"})),
        );
        assert_eq!(n.jsonrpc, "2.0");
        assert_eq!(n.method, "notifications/message");
        assert!(n.params.is_some());

        // Must have no "id" field when serialized
        let json = serde_json::to_string(&n).unwrap();
        assert!(!json.contains("\"id\""));
        assert!(json.contains("\"method\":\"notifications/message\""));
    }

    #[test]
    fn test_notification_without_params() {
        let n = JsonRpcNotification::new("notifications/cancelled", None);
        let json = serde_json::to_string(&n).unwrap();
        assert!(!json.contains("\"params\""));
    }
}
