//! MCP tool definitions and registry
//!
//! This module defines the tools exposed by the MCP server and provides
//! a registry for looking up and invoking tools.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use crate::events::{
    EventEnvelope, EventPayload, EventType, ExpectedResponse, MessageEvent, MessageIntent,
    Priority,
};
use crate::mcp::protocol::{CallToolResult, McpError};
use crate::nats::messages::MessageNotification;
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

    /// List tool definitions as JSON (no state needed — for Streamable HTTP tools/list)
    pub fn list_definitions() -> Value {
        let tools: Vec<_> = Self::all_tools()
            .into_iter()
            .map(|t| serde_json::json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            }))
            .collect();
        serde_json::json!(tools)
    }

    /// Get a reference to the shared application state
    pub fn state(&self) -> &AppState {
        &self.state
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
            "create_thread" => self.tool_create_thread(arguments, agent_id).await,
            "read_inbox" => self.tool_read_inbox(arguments, agent_id).await,
            "reply_to_thread" => self.tool_reply_to_thread(arguments, agent_id).await,
            "read_thread" => self.tool_read_thread(arguments, agent_id).await,
            "acknowledge_messages" => self.tool_acknowledge_messages(arguments, agent_id).await,
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
            // Council tools (explicit agent identity)
            Self::def_create_thread(),
            Self::def_read_inbox(),
            Self::def_reply_to_thread(),
            Self::def_read_thread(),
            Self::def_acknowledge_messages(),
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
                    },
                    "intent": {
                        "type": "string",
                        "enum": ["discuss", "request", "inform"],
                        "default": "inform",
                        "description": "Message intent: discuss (respond when ready), request (action needed), inform (FYI)"
                    },
                    "expected_response": {
                        "type": "string",
                        "enum": ["reply", "ack", "comply", "none", "standby"],
                        "default": "none",
                        "description": "What you expect the receiver to do: reply (OVER — send a response), ack (ROGER — confirm receipt), comply (WILCO — do this and confirm), none (OUT — FYI only), standby (STANDBY — hold for my follow-up)"
                    },
                    "require_ack": {
                        "type": "boolean",
                        "default": false,
                        "description": "Whether to track receipt acknowledgment for this message"
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
            description: "List conversation threads, optionally filtered by participant agent.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": {
                        "type": "string",
                        "description": "Filter threads by participant agent (e.g., 'thales', 'aleph')"
                    },
                    "participant": {
                        "type": "string",
                        "description": "Alias for 'agent' — filter by participant"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["active", "paused", "blocked", "resolved", "archived", "all"],
                        "default": "active"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 10
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
    // Council Tool Definitions (explicit agent identity for multi-agent use)
    // ========================================================================

    fn def_create_thread() -> ToolDefinition {
        ToolDefinition {
            name: "create_thread".to_string(),
            description: "Create a new conversation thread between agents. Persists to SurrealDB, updates the indexer, and broadcasts to WebSocket listeners.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "from_agent": {
                        "type": "string",
                        "description": "Sending agent identifier (e.g., 'thales', 'aleph', 'luban')"
                    },
                    "to_agent": {
                        "type": "string",
                        "description": "Receiving agent identifier"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Thread subject (convention: am.agent.council.*)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Message body (markdown supported)"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["low", "normal", "high", "critical"],
                        "default": "normal",
                        "description": "Message priority"
                    },
                    "intent": {
                        "type": "string",
                        "enum": ["discuss", "request", "inform"],
                        "default": "inform",
                        "description": "Message intent: discuss (respond when ready), request (action needed), inform (FYI)"
                    }
                },
                "required": ["from_agent", "to_agent", "subject", "content"]
            }),
        }
    }

    fn def_read_inbox() -> ToolDefinition {
        ToolDefinition {
            name: "read_inbox".to_string(),
            description: "Read pending messages for an agent. Returns all messages addressed to the specified agent.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": {
                        "type": "string",
                        "description": "Agent whose inbox to read (e.g., 'thales', 'aleph', 'luban')"
                    }
                },
                "required": ["agent"]
            }),
        }
    }

    fn def_reply_to_thread() -> ToolDefinition {
        ToolDefinition {
            name: "reply_to_thread".to_string(),
            description: "Reply within an existing thread. The recipient and subject are inferred from the thread's participants.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "thread_id": {
                        "type": "string",
                        "description": "Thread to reply in (UUID)"
                    },
                    "from_agent": {
                        "type": "string",
                        "description": "Sending agent identifier"
                    },
                    "content": {
                        "type": "string",
                        "description": "Reply body (markdown supported)"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["low", "normal", "high", "critical"],
                        "default": "normal",
                        "description": "Message priority"
                    },
                    "intent": {
                        "type": "string",
                        "enum": ["discuss", "request", "inform"],
                        "default": "inform",
                        "description": "Message intent: discuss (respond when ready), request (action needed), inform (FYI)"
                    }
                },
                "required": ["thread_id", "from_agent", "content"]
            }),
        }
    }

    fn def_read_thread() -> ToolDefinition {
        ToolDefinition {
            name: "read_thread".to_string(),
            description: "Read all messages in a specific thread, ordered chronologically.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "thread_id": {
                        "type": "string",
                        "description": "Thread to read (UUID)"
                    }
                },
                "required": ["thread_id"]
            }),
        }
    }

    fn def_acknowledge_messages() -> ToolDefinition {
        ToolDefinition {
            name: "acknowledge_messages".to_string(),
            description: "Mark messages as read/acknowledged. Advances the read cursor to the specified message ID. Call this AFTER you have processed messages from check_messages.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message_id": {
                        "type": "string",
                        "description": "ID of the newest message to acknowledge (all messages up to and including this ID are marked read)"
                    }
                },
                "required": ["message_id"]
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

        // 4. Publish to JetStream + NATS notification for MessageSent events
        if event.event_type == EventType::MessageSent {
            if let EventPayload::Message(ref msg) = event.payload {
                let nats_guard = self.state.nats_client_mut().await;
                if let Some(ref client) = *nats_guard {
                    // JetStream durable delivery (Phase 2)
                    if let Err(e) = client.publish_message_event(&msg.to, event).await {
                        eprintln!("[ming-qiao] JetStream publish failed: {} (SurrealDB is authoritative)", e);
                        tracing::warn!("JetStream message publish failed: {}", e);
                    }

                    // Ephemeral notification hint
                    let notification = MessageNotification {
                        event_id: event_id.clone(),
                        from: msg.from.clone(),
                        subject: msg.subject.clone(),
                        intent: msg.intent.clone(),
                        timestamp: event.timestamp,
                    };
                    if let Err(e) = client.publish_message_notification(&msg.to, &notification).await {
                        eprintln!("[ming-qiao] NATS message notification to '{}' failed: {}", msg.to, e);
                        tracing::warn!("NATS message notification failed: {}", e);
                    } else {
                        eprintln!("[ming-qiao] NATS notification sent to '{}': {}", msg.to, msg.subject);
                    }
                } else {
                    eprintln!("[ming-qiao] No NATS client — notification to '{}' skipped", msg.to);
                }
            }
        }

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

    /// Helper to parse message intent from string
    fn parse_intent(s: Option<&str>) -> MessageIntent {
        match s {
            Some("discuss") => MessageIntent::Discuss,
            Some("request") => MessageIntent::Request,
            _ => MessageIntent::Inform,
        }
    }

    /// Helper to parse expected response from string
    fn parse_expected_response(s: Option<&str>) -> ExpectedResponse {
        match s {
            Some("reply") => ExpectedResponse::Reply,
            Some("ack") => ExpectedResponse::Ack,
            Some("comply") => ExpectedResponse::Comply,
            Some("standby") => ExpectedResponse::Standby,
            _ => ExpectedResponse::None,
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
        let intent = Self::parse_intent(args.get("intent").and_then(|v| v.as_str()));
        let expected_response = Self::parse_expected_response(args.get("expected_response").and_then(|v| v.as_str()));
        let require_ack = args.get("require_ack").and_then(|v| v.as_bool()).unwrap_or(false);

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
                intent,
                expected_response,
                require_ack,
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
        let unread_only = args
            .get("unread_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let from_agent = args.get("from_agent").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        // Get server-side read cursor for unread filtering
        let read_cursor = if unread_only {
            self.state.persistence().get_read_cursor(agent_id).await.unwrap_or(None)
        } else {
            None
        };

        // Use Indexer for O(1) lookups — clone messages to release the lock
        let mut messages: Vec<_> = {
            let indexer = self.state.indexer().await;
            indexer
                .get_messages_to_agent(agent_id)
                .into_iter()
                .filter(|msg| {
                    if let Some(from) = from_agent {
                        if msg.from != from {
                            return false;
                        }
                    }
                    // Filter by read cursor (unread_only): only show messages newer than cursor
                    if let Some(ref cursor_id) = read_cursor {
                        if msg.id.as_str() <= cursor_id.as_str() {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect()
        };

        // Sort by timestamp (most recent first), then apply limit
        messages.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let total_count = messages.len();
        messages.truncate(limit);

        // Read cursor is NOT auto-advanced by check_messages.
        // Agents must explicitly call acknowledge_messages after processing.
        // This prevents the cursor race condition where reading advances past
        // unprocessed messages.

        if messages.is_empty() {
            return Ok(CallToolResult::text(format!(
                "## Inbox for {}\n\nNo {} messages.",
                agent_id,
                if unread_only { "unread" } else { "" }
            )));
        }

        let mut output = format!("## Inbox for {} ({} unread)\n\n", agent_id, messages.len());
        for msg in messages {
            output.push_str(&format!(
                "### {} [{}]\n**From:** {} | **Priority:** {:?} | **Intent:** {:?} | **Expected:** {:?}\n**ID:** {} | **Thread:** {}\n\n---\n\n",
                msg.subject,
                msg.created_at.format("%Y-%m-%d %H:%M"),
                msg.from,
                msg.priority,
                msg.intent,
                msg.expected_response,
                msg.id,
                msg.thread_id
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

        // Try Indexer first (O(1) in-memory lookup)
        {
            let indexer = self.state.indexer().await;
            if let Some(msg) = indexer.get_message(message_id) {
                return Ok(CallToolResult::text(format!(
                    "## {}\n\n**From:** {}\n**To:** {}\n**Date:** {}\n**Priority:** {:?}\n**Intent:** {:?}\n**Expected Response:** {:?}\n**Require ACK:** {}\n\n---\n\n{}",
                    msg.subject, msg.from, msg.to,
                    msg.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
                    msg.priority, msg.intent, msg.expected_response,
                    msg.require_ack, msg.content
                )));
            }
        }

        // Fallback: query SurrealDB directly (handles cross-instance race)
        let envelope = self
            .state
            .persistence()
            .get_event(message_id)
            .await
            .map_err(|e| McpError::Internal(format!("DB lookup failed: {}", e)))?;

        match envelope {
            Some(ref env) => {
                // Self-heal: feed into Indexer so subsequent reads are O(1)
                {
                    let mut indexer = self.state.indexer_mut().await;
                    if let Err(e) = indexer.process_event(env) {
                        tracing::warn!("Indexer failed to process event {}: {}", message_id, e);
                    }
                }

                if let EventPayload::Message(ref m) = env.payload {
                    Ok(CallToolResult::text(format!(
                        "## {}\n\n**From:** {}\n**To:** {}\n**Date:** {}\n**Priority:** {:?}\n**Intent:** {:?}\n**Expected Response:** {:?}\n**Require ACK:** {}\n\n---\n\n{}",
                        m.subject, m.from, m.to,
                        env.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                        m.priority, m.intent, m.expected_response,
                        m.require_ack, m.content
                    )))
                } else {
                    Ok(CallToolResult::text(format!(
                        "Event {} exists but is not a message (type: {})",
                        message_id, env.event_type
                    )))
                }
            }
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
                intent: MessageIntent::Request,
                expected_response: ExpectedResponse::Reply,
                require_ack: false,
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
        // Accept both "agent" and "participant" for filtering
        let participant = args
            .get("agent")
            .or_else(|| args.get("participant"))
            .and_then(|v| v.as_str());

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
            .cloned()
            .collect();
        drop(indexer);

        // Sort by updated_at (most recent first)
        threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        threads.truncate(limit);

        let json_threads: Vec<_> = threads
            .iter()
            .map(|t| {
                serde_json::json!({
                    "thread_id": t.id,
                    "subject": t.subject,
                    "participants": t.participants,
                    "message_count": t.message_count,
                    "last_message_at": t.updated_at.to_rfc3339()
                })
            })
            .collect();

        Ok(CallToolResult::text(
            serde_json::to_string_pretty(&serde_json::json!({
                "threads": json_threads
            }))
            .unwrap(),
        ))
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

    // ========================================================================
    // Council Tool Implementations
    // ========================================================================

    async fn tool_create_thread(
        &self,
        args: Value,
        _agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let from_agent = args
            .get("from_agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'from_agent' is required".to_string()))?;
        let to_agent = args
            .get("to_agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'to_agent' is required".to_string()))?;
        let subject = args
            .get("subject")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'subject' is required".to_string()))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'content' is required".to_string()))?;

        let priority = Self::parse_priority(args.get("priority").and_then(|v| v.as_str()));
        let intent = Self::parse_intent(args.get("intent").and_then(|v| v.as_str()));

        let event_id = Uuid::now_v7();
        let now = Utc::now();

        let event = EventEnvelope {
            id: event_id,
            timestamp: now,
            event_type: EventType::MessageSent,
            agent_id: from_agent.to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: from_agent.to_string(),
                to: to_agent.to_string(),
                subject: subject.to_string(),
                content: content.to_string(),
                thread_id: None,
                priority,
                intent,
                expected_response: Self::parse_expected_response(args.get("expected_response").and_then(|v| v.as_str())),
                require_ack: args.get("require_ack").and_then(|v| v.as_bool()).unwrap_or(false),
            }),
        };

        self.write_event(&event).await?;
        let thread_id = event_id.to_string();

        Ok(CallToolResult::text(
            serde_json::to_string_pretty(&serde_json::json!({
                "thread_id": thread_id,
                "message_id": thread_id,
                "created_at": now.to_rfc3339()
            }))
            .unwrap(),
        ))
    }

    async fn tool_read_inbox(
        &self,
        args: Value,
        _agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let agent = args
            .get("agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'agent' is required".to_string()))?;

        let messages: Vec<_> = {
            let indexer = self.state.indexer().await;
            indexer
                .get_messages_to_agent(agent)
                .into_iter()
                .cloned()
                .collect()
        };

        let json_messages: Vec<_> = messages
            .iter()
            .map(|msg| {
                serde_json::json!({
                    "message_id": msg.id,
                    "thread_id": msg.thread_id,
                    "from": msg.from,
                    "subject": msg.subject,
                    "content": msg.content,
                    "intent": msg.intent,
                    "created_at": msg.created_at.to_rfc3339()
                })
            })
            .collect();

        Ok(CallToolResult::text(
            serde_json::to_string_pretty(&serde_json::json!({
                "messages": json_messages
            }))
            .unwrap(),
        ))
    }

    async fn tool_reply_to_thread(
        &self,
        args: Value,
        _agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let thread_id = args
            .get("thread_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'thread_id' is required".to_string()))?;
        let from_agent = args
            .get("from_agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'from_agent' is required".to_string()))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'content' is required".to_string()))?;

        let priority = Self::parse_priority(args.get("priority").and_then(|v| v.as_str()));
        let intent = Self::parse_intent(args.get("intent").and_then(|v| v.as_str()));

        // Look up thread for recipient and subject (with SurrealDB + HTTP fallback)
        // Also resolves message_id → thread_id when agents pass the wrong ID
        let mut resolved_thread_id = thread_id.to_string();

        let found_in_indexer = {
            let indexer = self.state.indexer().await;
            if indexer.get_thread(thread_id).is_some() {
                true
            } else if let Some(msg) = indexer.get_message(thread_id) {
                resolved_thread_id = msg.thread_id.clone();
                indexer.get_thread(&resolved_thread_id).is_some()
            } else {
                false
            }
        };

        if !found_in_indexer {
            // Fallback 1: query SurrealDB for thread events and feed into Indexer
            let events = self
                .state
                .persistence()
                .get_events_by_thread_id(&resolved_thread_id)
                .await
                .unwrap_or_default();

            if !events.is_empty() {
                let mut indexer = self.state.indexer_mut().await;
                for event in &events {
                    let _ = indexer.process_event(event);
                }
            } else {
                // Fallback 2: query the HTTP server (authoritative SurrealDB connection)
                let http_port = {
                    let cfg = self.state.config().await;
                    cfg.port
                };
                let http_url = format!("http://localhost:{}/api/thread/{}", http_port, thread_id);
                if let Ok(resp) = reqwest::Client::new()
                    .get(&http_url)
                    .timeout(Duration::from_secs(3))
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        if let Ok(body) = resp.json::<serde_json::Value>().await {
                            if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
                                let subject = body.get("subject").and_then(|v| v.as_str()).unwrap_or("(unknown)").to_string();
                                let mut indexer = self.state.indexer_mut().await;
                                for msg_val in messages {
                                    let msg_id = msg_val.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                                    let msg_from = msg_val.get("from").and_then(|v| v.as_str()).unwrap_or("unknown");
                                    let msg_to = msg_val.get("to").and_then(|v| v.as_str()).unwrap_or("unknown");
                                    let msg_subject = msg_val.get("subject").and_then(|v| v.as_str()).unwrap_or(&subject);
                                    let msg_content = msg_val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                    let msg_tid = msg_val.get("thread_id").and_then(|v| v.as_str()).map(String::from)
                                        .or_else(|| if msg_id != thread_id { Some(thread_id.to_string()) } else { None });
                                    let ts = msg_val.get("created_at").and_then(|v| v.as_str())
                                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                                        .map(|dt| dt.with_timezone(&Utc))
                                        .unwrap_or_else(Utc::now);
                                    let synth = EventEnvelope {
                                        id: msg_id.parse().unwrap_or_else(|_| Uuid::now_v7()),
                                        timestamp: ts,
                                        event_type: EventType::MessageSent,
                                        agent_id: msg_from.to_string(),
                                        payload: EventPayload::Message(MessageEvent {
                                            from: msg_from.to_string(),
                                            to: msg_to.to_string(),
                                            subject: msg_subject.to_string(),
                                            content: msg_content.to_string(),
                                            thread_id: msg_tid,
                                            priority: Priority::Normal,
                                            intent: MessageIntent::Inform,
                                            expected_response: ExpectedResponse::None,
                                            require_ack: false,
                                        }),
                                    };
                                    let _ = indexer.process_event(&synth);
                                }
                                eprintln!("[ming-qiao] reply_to_thread: resolved thread {} via HTTP API fallback", thread_id);
                            }
                        }
                    }
                }
            }
        }

        let (to_agent, subject) = {
            let indexer = self.state.indexer().await;
            // Try resolved_thread_id first, then original thread_id
            let thread_opt = indexer.get_thread(&resolved_thread_id)
                .or_else(|| indexer.get_thread(thread_id));
            match thread_opt {
                Some(thread) => {
                    resolved_thread_id = thread.id.clone();
                    let to = if thread.participants.len() > 2 {
                        "council".to_string()
                    } else {
                        thread
                            .participants
                            .iter()
                            .find(|p| p.as_str() != from_agent)
                            .cloned()
                            .unwrap_or_else(|| from_agent.to_string())
                    };
                    (to, thread.subject.clone())
                }
                None => {
                    return Err(McpError::NotFound(format!(
                        "Thread not found (checked Indexer, local DB, and HTTP API): {}",
                        thread_id
                    )));
                }
            }
        };

        let event_id = Uuid::now_v7();
        let now = Utc::now();

        let event = EventEnvelope {
            id: event_id,
            timestamp: now,
            event_type: EventType::MessageSent,
            agent_id: from_agent.to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: from_agent.to_string(),
                to: to_agent,
                subject,
                content: content.to_string(),
                thread_id: Some(resolved_thread_id.clone()),
                priority,
                intent,
                expected_response: Self::parse_expected_response(args.get("expected_response").and_then(|v| v.as_str())),
                require_ack: args.get("require_ack").and_then(|v| v.as_bool()).unwrap_or(false),
            }),
        };

        self.write_event(&event).await?;

        Ok(CallToolResult::text(
            serde_json::to_string_pretty(&serde_json::json!({
                "message_id": event_id.to_string(),
                "thread_id": resolved_thread_id,
                "created_at": now.to_rfc3339()
            }))
            .unwrap(),
        ))
    }

    async fn tool_read_thread(
        &self,
        args: Value,
        _agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let thread_id = args
            .get("thread_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'thread_id' is required".to_string()))?;

        // Try Indexer first
        let found_in_indexer = {
            let indexer = self.state.indexer().await;
            indexer.get_thread(thread_id).is_some()
        };

        if !found_in_indexer {
            // Fallback: query SurrealDB for thread events and feed into Indexer
            let events = self
                .state
                .persistence()
                .get_events_by_thread_id(thread_id)
                .await
                .map_err(|e| McpError::Internal(format!("DB thread lookup failed: {}", e)))?;

            if events.is_empty() {
                return Err(McpError::NotFound(format!(
                    "Thread not found: {}",
                    thread_id
                )));
            }

            // Self-heal: feed all thread events into Indexer
            {
                let mut indexer = self.state.indexer_mut().await;
                for event in &events {
                    if let Err(e) = indexer.process_event(event) {
                        tracing::warn!(
                            "Indexer failed to process event {}: {}",
                            event.id, e
                        );
                    }
                }
            }
        }

        // Read from Indexer (now populated either way)
        let (subject, json_messages) = {
            let indexer = self.state.indexer().await;

            let thread = indexer.get_thread(thread_id).ok_or_else(|| {
                McpError::NotFound(format!("Thread not found: {}", thread_id))
            })?;
            let subject = thread.subject.clone();

            let mut msgs: Vec<_> = indexer
                .get_messages_for_thread(thread_id)
                .into_iter()
                .cloned()
                .collect();
            msgs.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            let messages: Vec<_> = msgs
                .iter()
                .map(|msg| {
                    serde_json::json!({
                        "message_id": msg.id,
                        "from": msg.from,
                        "content": msg.content,
                        "created_at": msg.created_at.to_rfc3339()
                    })
                })
                .collect();

            (subject, messages)
        };

        Ok(CallToolResult::text(
            serde_json::to_string_pretty(&serde_json::json!({
                "thread_id": thread_id,
                "subject": subject,
                "messages": json_messages
            }))
            .unwrap(),
        ))
    }

    async fn tool_acknowledge_messages(
        &self,
        args: Value,
        agent_id: &str,
    ) -> Result<CallToolResult, McpError> {
        let message_id = args
            .get("message_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidInput("'message_id' is required".to_string()))?;

        self.state
            .persistence()
            .update_read_cursor(agent_id, message_id)
            .await
            .map_err(|e| McpError::Internal(format!("Failed to acknowledge: {}", e)))?;

        Ok(CallToolResult::text(format!(
            "Acknowledged messages up to {}. Cursor advanced for agent '{}'.",
            message_id, agent_id
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

        assert_eq!(tools.len(), 13); // Was 12, now includes acknowledge_messages

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"send_message"));
        assert!(names.contains(&"check_messages"));
        assert!(names.contains(&"read_message"));
        assert!(names.contains(&"list_threads"));
        assert!(names.contains(&"create_thread"));
        assert!(names.contains(&"read_inbox"));
        assert!(names.contains(&"reply_to_thread"));
        assert!(names.contains(&"read_thread"));
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

    // ====================================================================
    // Council tool tests
    // ====================================================================

    fn extract_text(result: &CallToolResult) -> &str {
        match result.content.first().unwrap() {
            crate::mcp::protocol::ToolContent::Text { text } => text,
            _ => panic!("Expected text content"),
        }
    }

    #[tokio::test]
    async fn test_create_thread() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let result = registry
            .call(
                "create_thread",
                serde_json::json!({
                    "from_agent": "thales",
                    "to_agent": "aleph",
                    "subject": "am.agent.council.test",
                    "content": "Test message from Thales"
                }),
                "thales",
            )
            .await
            .unwrap();

        let text = extract_text(&result);
        let json: Value = serde_json::from_str(text).unwrap();
        assert!(json.get("thread_id").is_some());
        assert!(json.get("message_id").is_some());
        assert!(json.get("created_at").is_some());
    }

    #[tokio::test]
    async fn test_create_thread_missing_params() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let result = registry
            .call(
                "create_thread",
                serde_json::json!({"from_agent": "thales"}),
                "thales",
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(McpError::InvalidInput(msg)) => assert!(msg.contains("to_agent")),
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[tokio::test]
    async fn test_read_inbox_empty() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let result = registry
            .call(
                "read_inbox",
                serde_json::json!({"agent": "thales"}),
                "thales",
            )
            .await
            .unwrap();

        let text = extract_text(&result);
        let json: Value = serde_json::from_str(text).unwrap();
        let messages = json.get("messages").unwrap().as_array().unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_create_then_read_inbox() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);

        // Create a thread from thales to aleph
        registry
            .call(
                "create_thread",
                serde_json::json!({
                    "from_agent": "thales",
                    "to_agent": "aleph",
                    "subject": "am.agent.council.test",
                    "content": "Hello Aleph"
                }),
                "thales",
            )
            .await
            .unwrap();

        // Read aleph's inbox
        let result = registry
            .call(
                "read_inbox",
                serde_json::json!({"agent": "aleph"}),
                "aleph",
            )
            .await
            .unwrap();

        let text = extract_text(&result);
        let json: Value = serde_json::from_str(text).unwrap();
        let messages = json.get("messages").unwrap().as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["from"], "thales");
        assert_eq!(messages[0]["content"], "Hello Aleph");
    }

    #[tokio::test]
    async fn test_reply_to_thread() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);

        // Create a thread
        let create_result = registry
            .call(
                "create_thread",
                serde_json::json!({
                    "from_agent": "luban",
                    "to_agent": "aleph",
                    "subject": "am.agent.council.golden-thread",
                    "content": "明桥通了吗？"
                }),
                "luban",
            )
            .await
            .unwrap();

        let create_json: Value =
            serde_json::from_str(extract_text(&create_result)).unwrap();
        let thread_id = create_json["thread_id"].as_str().unwrap();

        // Reply in the thread
        let reply_result = registry
            .call(
                "reply_to_thread",
                serde_json::json!({
                    "thread_id": thread_id,
                    "from_agent": "aleph",
                    "content": "桥已通。"
                }),
                "aleph",
            )
            .await
            .unwrap();

        let reply_json: Value =
            serde_json::from_str(extract_text(&reply_result)).unwrap();
        assert_eq!(reply_json["thread_id"], thread_id);
        assert!(reply_json.get("message_id").is_some());
    }

    #[tokio::test]
    async fn test_reply_to_nonexistent_thread() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let result = registry
            .call(
                "reply_to_thread",
                serde_json::json!({
                    "thread_id": "nonexistent-id",
                    "from_agent": "aleph",
                    "content": "Hello?"
                }),
                "aleph",
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(McpError::NotFound(msg)) => assert!(msg.contains("nonexistent-id")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_read_thread() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);

        // Create a thread and reply
        let create_result = registry
            .call(
                "create_thread",
                serde_json::json!({
                    "from_agent": "thales",
                    "to_agent": "aleph",
                    "subject": "am.agent.council.design",
                    "content": "First message"
                }),
                "thales",
            )
            .await
            .unwrap();

        let create_json: Value =
            serde_json::from_str(extract_text(&create_result)).unwrap();
        let thread_id = create_json["thread_id"].as_str().unwrap();

        registry
            .call(
                "reply_to_thread",
                serde_json::json!({
                    "thread_id": thread_id,
                    "from_agent": "aleph",
                    "content": "Second message"
                }),
                "aleph",
            )
            .await
            .unwrap();

        // Read the full thread
        let result = registry
            .call(
                "read_thread",
                serde_json::json!({"thread_id": thread_id}),
                "thales",
            )
            .await
            .unwrap();

        let text = extract_text(&result);
        let json: Value = serde_json::from_str(text).unwrap();
        assert_eq!(json["thread_id"], thread_id);
        assert_eq!(json["subject"], "am.agent.council.design");
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["from"], "thales");
        assert_eq!(messages[1]["from"], "aleph");
    }

    #[tokio::test]
    async fn test_read_thread_not_found() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);
        let result = registry
            .call(
                "read_thread",
                serde_json::json!({"thread_id": "no-such-thread"}),
                "thales",
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(McpError::NotFound(msg)) => assert!(msg.contains("no-such-thread")),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_list_threads_with_agent_filter() {
        let state = AppState::new().await;
        let registry = ToolRegistry::with_state(state);

        // Create threads involving different agents
        registry
            .call(
                "create_thread",
                serde_json::json!({
                    "from_agent": "thales",
                    "to_agent": "aleph",
                    "subject": "am.agent.council.thales-aleph",
                    "content": "For Aleph"
                }),
                "thales",
            )
            .await
            .unwrap();

        registry
            .call(
                "create_thread",
                serde_json::json!({
                    "from_agent": "thales",
                    "to_agent": "luban",
                    "subject": "am.agent.council.thales-luban",
                    "content": "For Luban"
                }),
                "thales",
            )
            .await
            .unwrap();

        // List threads for aleph only
        let result = registry
            .call(
                "list_threads",
                serde_json::json!({"agent": "aleph"}),
                "thales",
            )
            .await
            .unwrap();

        let text = extract_text(&result);
        let json: Value = serde_json::from_str(text).unwrap();
        let threads = json["threads"].as_array().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0]["subject"], "am.agent.council.thales-aleph");
    }
}
