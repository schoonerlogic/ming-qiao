//! MCP client for ASTROLABE ingestion.
//!
//! Replaces astrolabe-ingest.py — connects to the Graphiti MCP server
//! and submits content to the knowledge graph.
//!
//! Agent: luban

use serde::{Deserialize, Serialize};

const DEFAULT_MCP_URL: &str = "http://localhost:8001/mcp";
const DEFAULT_GROUP: &str = "astrolabe_main";

/// MCP client for ASTROLABE operations.
pub struct AstrolabeClient {
    mcp_url: String,
    session_id: Option<String>,
    client: reqwest::blocking::Client,
}

/// Result from an MCP tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub data: serde_json::Value,
    pub is_error: bool,
}

impl AstrolabeClient {
    /// Create a new client with the given MCP URL.
    pub fn new(mcp_url: Option<&str>) -> Self {
        Self {
            mcp_url: mcp_url.unwrap_or(DEFAULT_MCP_URL).to_string(),
            session_id: None,
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Initialize an MCP session.
    pub fn connect(&mut self) -> Result<(), IngestError> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "astrolabe-ingest-rs", "version": "1.0" },
            },
            "id": 1,
        });

        let resp = self
            .client
            .post(&self.mcp_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&payload)
            .send()
            .map_err(|e| IngestError::Connection(e.to_string()))?;

        let session_id = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(String::from)
            .ok_or_else(|| IngestError::Connection("No MCP session ID".to_string()))?;

        // Send initialized notification
        let notify = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });

        self.client
            .post(&self.mcp_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("Mcp-Session-Id", &session_id)
            .json(&notify)
            .send()
            .map_err(|e| IngestError::Connection(e.to_string()))?;

        self.session_id = Some(session_id);
        Ok(())
    }

    /// Call an MCP tool.
    pub fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolResult, IngestError> {
        let session_id = self
            .session_id
            .as_ref()
            .ok_or(IngestError::NotConnected)?;

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": tool_name, "arguments": arguments },
            "id": 2,
        });

        let resp = self
            .client
            .post(&self.mcp_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("Mcp-Session-Id", session_id)
            .json(&payload)
            .send()
            .map_err(|e| IngestError::Connection(e.to_string()))?;

        let body = resp
            .text()
            .map_err(|e| IngestError::Connection(e.to_string()))?;

        // Parse SSE response
        for line in body.lines() {
            if let Some(data_str) = line.strip_prefix("data: ") {
                let data: serde_json::Value = serde_json::from_str(data_str)
                    .map_err(|e| IngestError::Parse(e.to_string()))?;

                let is_error = data
                    .get("result")
                    .and_then(|r| r.get("isError"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let content = data
                    .get("result")
                    .and_then(|r| r.get("content"))
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|item| item.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("{}");

                let parsed: serde_json::Value = serde_json::from_str(content)
                    .unwrap_or_else(|_| serde_json::json!({ "raw": content }));

                return Ok(McpToolResult {
                    data: parsed,
                    is_error,
                });
            }
        }

        Err(IngestError::Parse("No data in MCP response".to_string()))
    }

    /// Ingest content into the ASTROLABE knowledge graph.
    pub fn ingest(
        &self,
        name: &str,
        episode_body: &str,
        source: &str,
        source_description: &str,
        group_id: Option<&str>,
    ) -> Result<McpToolResult, IngestError> {
        self.call_tool(
            "add_memory",
            serde_json::json!({
                "name": name,
                "episode_body": episode_body,
                "group_id": group_id.unwrap_or(DEFAULT_GROUP),
                "source": source,
                "source_description": source_description,
            }),
        )
    }

    /// Search nodes in the knowledge graph.
    pub fn search_nodes(
        &self,
        query: &str,
        group_id: Option<&str>,
    ) -> Result<McpToolResult, IngestError> {
        self.call_tool(
            "search_nodes",
            serde_json::json!({
                "query": query,
                "group_ids": [group_id.unwrap_or(DEFAULT_GROUP)],
            }),
        )
    }

    /// Search facts in the knowledge graph.
    pub fn search_facts(
        &self,
        query: &str,
        group_id: Option<&str>,
    ) -> Result<McpToolResult, IngestError> {
        self.call_tool(
            "search_memory_facts",
            serde_json::json!({
                "query": query,
                "group_ids": [group_id.unwrap_or(DEFAULT_GROUP)],
            }),
        )
    }
}

/// Errors from the ingest client.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("not connected — call connect() first")]
    NotConnected,

    #[error("parse error: {0}")]
    Parse(String),

    #[error("MCP error: {0}")]
    McpError(String),
}
