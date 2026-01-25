//! MCP (Model Context Protocol) server for ming-qiao
//!
//! This module implements the MCP server that Aleph uses to communicate
//! with other agents. It exposes tools for sending/receiving messages,
//! sharing artifacts, and querying decisions.
//!
//! Transport: stdio (JSON-RPC over stdin/stdout)

pub mod protocol;
pub mod server;
pub mod tools;

pub use protocol::{JsonRpcRequest, JsonRpcResponse, McpError, McpErrorCode};
pub use server::McpServer;
pub use tools::{ToolDefinition, ToolRegistry};
