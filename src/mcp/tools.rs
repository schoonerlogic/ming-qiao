//! MCP tool definitions and registry
//!
//! This module defines the tools exposed by the MCP server and provides
//! a registry for looking up and invoking tools.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

use crate::events::{
    EventEnvelope, EventPayload, EventType, MessageEvent, Priority,
};
use crate::mcp::protocol::{CallToolResult, McpError};
use crate::state::AppState;

/// Tool definition as exposed to MCP clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    /// Tool name (used in tools/call)
    pub name: String,

    /// Human-readable description
    pub description: String,

    /// JSON Schema for input parameters
    pub input_schema: Value,
}

/// Registry of available tools
pub struct ToolRegistry {
    /// Tool definitions by name
    definitions: HashMap<String, ToolDefinition>,
    /// App state (provides persistence, indexer, broadcasting)
    state: AppState,
}

impl ToolRegistry {
    /// Create a tool registry backed by AppState.
    ///
    /// All event writes go through `state.persistence()`, all reads through
    /// `state.indexer()`. Events are also fed to the Indexer after storing.
    pub fn with_state(state: AppState) -> Self {
        let mut definitions = HashMap::new();
        for tool in Self::all_tools() {
            definitions.insert(tool.name.clone(), tool);
        }
        Self { definitions, state }
    }

    /// List all available tools
    pub fn list(&self) -> Vec<&ToolDefinition> {
        self.definitions.values().collect()
    }

    /// Call a tool by name
    pub async fn call(
        &self,
        name: &str,
        arguments: Value,
        agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        if !self.definitions.contains_key(name) {
            return Err(McpError::NotFound(format!("Tool not found: {}", name)));
        }

        match name {
            "send_message" => self.tool_send_message(arguments, agent_id).await,
            "check_messages" => self.tool_check_messages(arguments, agent_id).await,
            "read_message" => self.tool_read_message(arguments, agent_id).await,
            "request_review" => self.tool_request_review(arguments, agent_id).await,
            "share_artifact" => self.tool_share_artifact(arguments, agent_id).await,
            "get_decision" => self.tool_get_decision(arguments, agent_id).await,
            "list_threads" => self.tool_list_threads(arguments, agent_id).await,
            "record_decision" => self.tool_record_decision(arguments, agent_id).await,
            _ => Err(McpError::NotFound(format!("Tool not found: {}", name))),
        }
    }

    /// All tool definitions
    fn all_tools() -> Vec<ToolDefinition> {
        vec![
            Self::def_send_message(),
            Self::def_check_messages(),
            Self::def_read_message(),
            Self::def_request_review(),
            Self::def_share_artifact(),
            Self::def_get_decision(),
            Self::def_list_threads(),
            Self::def_record_decision(),
        ]
    }

    // ========================================================================
    // Tool Definitions
    // ========================================================================

    fn def_send_message() -> ToolDefinition {
        ToolDefinition {
            name: "send_message".to_string(),
            description: "Send a message to another agent".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Recipient agent ID (e.g., 'thales', 'merlin')"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Message subject line"
                    },
                    "content": {
                        "type": "string",
                        "description": "Message body (markdown supported)"
                    },
                    "thread_id": {
                        "type": "string",
                        "description": "Existing thread ID to reply to (optional)"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["low", "normal", "high", "critical"],
                        "default": "normal",
                        "description": "Message priority"
                    }
                },
                "required": ["to", "subject", "content"]
            }),
        }
    }

    fn def_check_messages() -> ToolDefinition {
        ToolDefinition {
            name: "check_messages".to_string(),
            description: "Check inbox for new messages".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "unread_only": {
                        "type": "boolean",
                        "default": true,
                        "description": "Only return unread messages"
                    },
                    "from_agent": {
                        "type": "string",
                        "description": "Filter by sender agent ID"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 10,
                        "description": "Maximum messages to return"
                    }
                }
            }),
        }
    }

    fn def_read_message() -> ToolDefinition {
        ToolDefinition {
            name: "read_message".to_string(),
            description: "Read full content of a specific message".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_id": {
                        "type": "string",
                        "description": "Message ID to read"
                    },
                    "mark_read": {
                        "type": "boolean",
                        "default": true,
                        "description": "Mark message as read"
                    }
                },
                "required": ["message_id"]
            }),
        }
    }

    fn def_request_review() -> ToolDefinition {
        ToolDefinition {
            name: "request_review".to_string(),
            description: "Ask Thales to review an artifact".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "artifact_path": {
                        "type": "string",
                        "description": "Path to artifact to review"
                    },
                    "question": {
                        "type": "string",
                        "description": "Specific question or focus area for review"
                    },
                    "context": {
                        "type": "string",
                        "description": "Additional context for the reviewer"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["low", "normal", "high", "critical"],
                        "default": "normal"
                    }
                },
                "required": ["artifact_path", "question"]
            }),
        }
    }

    fn def_share_artifact() -> ToolDefinition {
        ToolDefinition {
            name: "share_artifact".to_string(),
            description: "Share a file for other agents to access".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source_path": {
                        "type": "string",
                        "description": "Local path to file to share"
                    },
                    "description": {
                        "type": "string",
                        "description": "Brief description of the artifact"
                    },
                    "target_name": {
                        "type": "string",
                        "description": "Name in artifacts directory (optional)"
                    }
                },
                "required": ["source_path"]
            }),
        }
    }

    fn def_get_decision() -> ToolDefinition {
        ToolDefinition {
            name: "get_decision".to_string(),
            description: "Retrieve a past decision by ID or query".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "decision_id": {
                        "type": "string",
                        "description": "Specific decision ID"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query (if no decision_id)"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 5,
                        "description": "Max results for query"
                    }
                }
            }),
        }
    }

    fn def_list_threads() -> ToolDefinition {
        ToolDefinition {
            name: "list_threads".to_string(),
            description: "List active and recent threads".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["active", "paused", "blocked", "resolved", "archived", "all"],
                        "default": "active"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 10
                    },
                    "participant": {
                        "type": "string",
                        "description": "Filter by participant agent"
                    }
                }
            }),
        }
    }

    fn def_record_decision() -> ToolDefinition {
        ToolDefinition {
            name: "record_decision".to_string(),
            description: "Record a decision from the current conversation".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "thread_id": {
                        "type": "string",
                        "description": "Thread where decision was made"
                    },
                    "question": {
                        "type": "string",
                        "description": "What was being decided"
                    },
                    "resolution": {
                        "type": "string",
                        "description": "What was decided"
                    },
                    "rationale": {
                        "type": "string",
                        "description": "Why this option was chosen"
                    },
                    "options_considered": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Alternatives that were evaluated"
                    }
                },
                "required": ["thread_id", "question", "resolution", "rationale"]
            }),
        }
    }

    // ========================================================================
    // Tool Implementations
    // ========================================================================

    /// Write an event: persist to SurrealDB, feed to Indexer, broadcast, notify.
    async fn write_event(&self, event: &EventEnvelope) -> Result<String, McpError> {
        // 1. Persist to SurrealDB
        let event_id = self
            .state
            .persistence()
            .store_event(event)
            .await
            .map_err(|e| McpError::Internal(format!("Failed to store event: {}", e)))?;

        // 2. Feed to Indexer (materialized views)
        {
            let mut indexer = self.state.indexer_mut().await;
            if let Err(e) = indexer.process_event(event) {
                tracing::warn!("Indexer failed to process event {}: {}", event_id, e);
            }
        }

        // 3. Broadcast to WebSocket clients + Merlin notifications
        self.state.broadcast_event(event.clone());
        self.state.merlin_notifier().notify(event.clone(), &self.state);

        Ok(event_id)
    }

    /// Helper to parse priority from string
    fn parse_priority(s: Option<&str>) -> Priority {
        match s {
            Some("low") => Priority::Low,
            Some("high") => Priority::High,
            Some("critical") => Priority::Critical,
            _ => Priority::Normal,
        }
    }

    async fn tool_send_message(
        &self,
        args: Value,
        agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let to = args
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'to' is required".to_string()))?;

        let subject = args
            .get("subject")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'subject' is required".to_string()))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'content' is required".to_string()))?;

        let thread_id = args
            .get("thread_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let priority = Self::parse_priority(args.get("priority").and_then(|v| v.as_str()));

        let event = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: agent_id.to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: agent_id.to_string(),
                to: to.to_string(),
                subject: subject.to_string(),
                content: content.to_string(),
                thread_id,
                priority,
            }),
        };

        let event_id = self.write_event(&event).await?;

        Ok(CallToolResult::text(format!(
            "Message sent successfully.\n\n**Event ID:** {}\n**From:** {}\n**To:** {}\n**Subject:** {}",
            event_id, agent_id, to, subject
        )))
    }

    async fn tool_check_messages(
        &self,
        args: Value,
        agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let _unread_only = args
            .get("unread_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let from_agent = args.get("from_agent").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        // Use Indexer for O(1) lookups — clone messages to release the lock
        let mut messages: Vec<_> = {
            let indexer = self.state.indexer().await;
            indexer
                .get_messages_to_agent(agent_id)
                .into_iter()
                .filter(|msg| {
                    if let Some(from) = from_agent {
                        msg.from == from
                    } else {
                        true
                    }
                })
                .take(limit)
                .cloned()
                .collect()
        };

        // Sort by timestamp (most recent first)
        messages.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        if messages.is_empty() {
            return Ok(CallToolResult::text(format!(
                "## Inbox for {}\n\nNo messages.",
                agent_id
            )));
        }

        let mut output = format!("## Inbox for {}\n\n", agent_id);
        for msg in messages {
            output.push_str(&format!(
                "### {} [{}]\n**From:** {} | **Priority:** {:?}\n**ID:** {}\n\n---\n\n",
                msg.subject,
                msg.created_at.format("%Y-%m-%d %H:%M"),
                msg.from,
                msg.priority,
                msg.id
            ));
        }

        Ok(CallToolResult::text(output))
    }

    async fn tool_read_message(
        &self,
        args: Value,
        _agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let message_id = args
            .get("message_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("message_id required".to_string()))?;

        // Use Indexer for O(1) lookup
        let indexer = self.state.indexer().await;
        let msg = indexer.get_message(message_id);

        match msg {
            Some(msg) => Ok(CallToolResult::text(format!(
                "## {}\n\n**From:** {}\n**To:** {}\n**Date:** {}\n**Priority:** {:?}\n\n---\n\n{}",
                msg.subject,
                msg.from,
                msg.to,
                msg.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
                msg.priority,
                msg.content
            ))),
            None => Ok(CallToolResult::text(format!(
                "Message not found: {}",
                message_id
            ))),
        }
    }

    async fn tool_request_review(
        &self,
        args: Value,
        agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let artifact_path = args
            .get("artifact_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("artifact_path required".to_string()))?;

        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("question required".to_string()))?;

        let context = args.get("context").and_then(|v| v.as_str()).unwrap_or("");
        let priority = Self::parse_priority(args.get("priority").and_then(|v| v.as_str()));

        let content = format!(
            "**Review Request**\n\n**Artifact:** {}\n\n**Question:** {}\n\n**Context:** {}",
            artifact_path, question, context
        );

        let event = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: agent_id.to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: agent_id.to_string(),
                to: "thales".to_string(),
                subject: format!("Review Request: {}", artifact_path),
                content,
                thread_id: None,
                priority,
            }),
        };

        let event_id = self.write_event(&event).await?;

        Ok(CallToolResult::text(format!(
            "Review requested from Thales.\n\n**Event ID:** {}\n**Artifact:** {}\n**Question:** {}",
            event_id, artifact_path, question
        )))
    }

    async fn tool_share_artifact(
        &self,
        args: Value,
        agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        use crate::events::ArtifactEvent;

        let source_path = args
            .get("source_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("source_path required".to_string()))?;

        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Shared artifact");

        let checksum = format!("sha256:{}", Uuid::now_v7());

        let event = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::ArtifactShared,
            agent_id: agent_id.to_string(),
            payload: EventPayload::Artifact(ArtifactEvent {
                path: source_path.to_string(),
                description: description.to_string(),
                checksum,
            }),
        };

        let event_id = self.write_event(&event).await?;

        Ok(CallToolResult::text(format!(
            "Artifact shared successfully.\n\n**Event ID:** {}\n**Path:** {}\n**Description:** {}",
            event_id, source_path, description
        )))
    }

    async fn tool_get_decision(
        &self,
        args: Value,
        _agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let decision_id = args.get("decision_id").and_then(|v| v.as_str());
        let query = args.get("query").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

        let indexer = self.state.indexer().await;

        // Search by specific decision ID
        if let Some(id) = decision_id {
            if let Some(decision) = indexer.get_decision(id) {
                let chosen_opt = decision
                    .options
                    .get(decision.chosen)
                    .map(|o| o.description.as_str())
                    .unwrap_or("(unknown)");

                return Ok(CallToolResult::text(format!(
                    "## Decision: {}\n\n**Date:** {}\n**Context:** {}\n\n**Chosen:** {}\n\n**Rationale:** {}",
                    decision.title,
                    decision.created_at.format("%Y-%m-%d %H:%M"),
                    decision.context,
                    chosen_opt,
                    decision.rationale
                )));
            }
            return Ok(CallToolResult::text(format!("Decision not found: {}", id)));
        }

        // Search by query string
        if let Some(q) = query {
            let q_lower = q.to_lowercase();
            let results: Vec<_> = indexer
                .get_decisions()
                .into_iter()
                .filter(|d| {
                    d.title.to_lowercase().contains(&q_lower)
                        || d.context.to_lowercase().contains(&q_lower)
                })
                .take(limit)
                .collect();

            if results.is_empty() {
                return Ok(CallToolResult::text(format!(
                    "No decisions found matching: {}",
                    q
                )));
            }

            let mut output = format!("## Decisions matching: {}\n\n", q);
            for decision in results {
                output.push_str(&format!(
                    "### {}\n**ID:** {} | **Date:** {}\n\n---\n\n",
                    decision.title,
                    decision.id,
                    decision.created_at.format("%Y-%m-%d %H:%M")
                ));
            }

            return Ok(CallToolResult::text(output));
        }

        Err(McpError::InvalidInput(
            "decision_id or query required".to_string(),
        ))
    }

    async fn tool_list_threads(
        &self,
        args: Value,
        _agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
        let participant = args.get("participant").and_then(|v| v.as_str());

        let indexer = self.state.indexer().await;
        let all_threads = indexer.get_all_threads();

        let mut threads: Vec<_> = all_threads
            .into_iter()
            .filter(|t| {
                if let Some(p) = participant {
                    t.participants.contains(&p.to_string())
                } else {
                    true
                }
            })
            .collect();

        // Sort by updated_at (most recent first)
        threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        threads.truncate(limit);

        if threads.is_empty() {
            return Ok(CallToolResult::text(
                "## Threads\n\nNo threads yet.".to_string(),
            ));
        }

        let mut output = "## Threads\n\n".to_string();
        for thread in threads {
            output.push_str(&format!(
                "- **{}** (started by {}, {})\n  ID: `{}`\n\n",
                thread.subject,
                thread.participants.first().map(|s| s.as_str()).unwrap_or("unknown"),
                thread.created_at.format("%Y-%m-%d %H:%M"),
                thread.id
            ));
        }

        Ok(CallToolResult::text(output))
    }

    async fn tool_record_decision(
        &self,
        args: Value,
        agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        use crate::events::{DecisionEvent, DecisionOption};

        let thread_id = args
            .get("thread_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("thread_id required".to_string()))?;

        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("question required".to_string()))?;

        let resolution = args
            .get("resolution")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("resolution required".to_string()))?;

        let rationale = args
            .get("rationale")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("rationale required".to_string()))?;

        let options: Vec<DecisionOption> = args
            .get("options_considered")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| DecisionOption {
                        description: s.to_string(),
                        pros: vec![],
                        cons: vec![],
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                vec![DecisionOption {
                    description: resolution.to_string(),
                    pros: vec![],
                    cons: vec![],
                }]
            });

        let event = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::DecisionRecorded,
            agent_id: agent_id.to_string(),
            payload: EventPayload::Decision(DecisionEvent {
                title: question.to_string(),
                context: format!("Thread: {}", thread_id),
                options,
                chosen: 0,
                rationale: rationale.to_string(),
            }),
        };

        let event_id = self.write_event(&event).await?;

        Ok(CallToolResult::text(format!(
            "Decision recorded successfully.\n\n**Event ID:** {}\n**Question:** {}\n**Resolution:** {}\n**Rationale:** {}",
            event_id, question, resolution, rationale
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_creation() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let tools = registry.list();

        assert_eq!(tools.len(), 8);

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"send_message"));
        assert!(names.contains(&"check_messages"));
        assert!(names.contains(&"read_message"));
        assert!(names.contains(&"list_threads"));
    }

    #[test]
    fn test_tool_definition_serialization() {
        let tool = ToolRegistry::def_send_message();
        let json = serde_json::to_string(&tool).unwrap();

        assert!(json.contains("inputSchema"));
        assert!(json.contains("\"name\":\"send_message\""));
    }

    #[tokio::test]
    async fn test_call_unknown_tool() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let result = registry
            .call("unknown_tool", serde_json::json!({}), "test")
            .await;

        assert!(result.is_err());
        match result {
            Err(McpError::NotFound(_)) => {}
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_call_send_message() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let result = registry
            .call(
                "send_message",
                serde_json::json!({
                    "to": "thales",
                    "subject": "Test",
                    "content": "Hello"
                }),
                "aleph",
            )
            .await
            .unwrap();

        if let Some(content) = result.content.first() {
            match content {
                crate::mcp::protocol::ToolContent::Text { text } => {
                    assert!(text.contains("Message sent"));
                    assert!(text.contains("aleph"));
                    assert!(text.contains("thales"));
                }
                _ => panic!("Expected text content"),
            }
        }
    }
}
