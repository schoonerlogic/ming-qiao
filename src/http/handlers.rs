//! HTTP request handlers
//!
//! Handler functions for all API endpoints. Now uses Indexer for O(1) lookups
//! instead of scanning the event log.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

use crate::events::{EventEnvelope, EventPayload, EventType, ExpectedResponse, MessageEvent, MessageIntent, Priority};
use crate::http::auth::AuthenticatedAgent;
use crate::nats::messages::MessageNotification;
use crate::state::AppState;

// ============================================================================
// Common Types
// ============================================================================

/// Standard API error response
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn not_found(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::NOT_FOUND,
            Json(Self {
                error: ApiErrorDetail {
                    code: "NOT_FOUND".to_string(),
                    message: message.into(),
                },
            }),
        )
    }

    pub fn bad_request(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::BAD_REQUEST,
            Json(Self {
                error: ApiErrorDetail {
                    code: "BAD_REQUEST".to_string(),
                    message: message.into(),
                },
            }),
        )
    }

    pub fn internal(message: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Self {
                error: ApiErrorDetail {
                    code: "INTERNAL_ERROR".to_string(),
                    message: message.into(),
                },
            }),
        )
    }
}

// ============================================================================
// Health Check
// ============================================================================

/// Health check endpoint
pub async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    let nats_connected = state.nats_connected().await;
    Json(serde_json::json!({
        "status": "healthy",
        "service": "ming-qiao",
        "version": env!("CARGO_PKG_VERSION"),
        "nats_connected": nats_connected
    }))
}

// ============================================================================
// Agent Read Cursors
// ============================================================================

/// Get read cursor state for all agents (or a specific agent).
///
/// Used by `am-fleet comms` to verify cursor health across the fleet.
/// Returns cursor position, total messages addressed to agent, and gap.
pub async fn get_cursors(
    State(state): State<AppState>,
    Query(query): Query<CursorQuery>,
) -> impl IntoResponse {
    let persistence = state.persistence();

    // Get all cursors from SurrealDB
    let cursors = match persistence.get_all_cursors().await {
        Ok(c) => c,
        Err(e) => {
            return Json(serde_json::json!({
                "error": format!("Failed to read cursors: {}", e)
            }));
        }
    };

    // Build cursor info with message counts from indexer
    let indexer = state.indexer().await;
    let mut results = Vec::new();

    // If a specific agent is requested, only return that one
    let agents: Vec<String> = if let Some(ref agent) = query.agent {
        vec![agent.clone()]
    } else {
        // Collect all agents that have either a cursor or messages
        let mut agent_set: std::collections::HashSet<String> = cursors.iter()
            .map(|c| c.agent_id.clone())
            .collect();
        // Also include agents with messages but no cursor
        for agent_id in indexer.all_recipient_agents() {
            agent_set.insert(agent_id);
        }
        let mut agents: Vec<_> = agent_set.into_iter().collect();
        agents.sort();
        agents
    };

    for agent_id in &agents {
        let total_messages = indexer.get_messages_to_agent(agent_id).len();
        let cursor = cursors.iter().find(|c| &c.agent_id == agent_id);

        let (cursor_event_id, last_read_at, unread_count) = match cursor {
            Some(c) => {
                // Count messages newer than cursor
                let unread = indexer.get_messages_to_agent(agent_id)
                    .iter()
                    .filter(|m| m.id.as_str() > c.last_read_event_id.as_str())
                    .count();
                (Some(c.last_read_event_id.clone()), Some(c.last_read_at.clone()), unread)
            }
            None => (None, None, total_messages),
        };

        results.push(serde_json::json!({
            "agent_id": agent_id,
            "cursor_event_id": cursor_event_id,
            "last_read_at": last_read_at,
            "total_messages": total_messages,
            "unread_count": unread_count,
        }));
    }

    drop(indexer);

    Json(serde_json::json!({
        "cursors": results
    }))
}

#[derive(Debug, Deserialize)]
pub struct CursorQuery {
    pub agent: Option<String>,
}

// ============================================================================
// Admin — Indexer Rehydration
// ============================================================================

/// Re-sync the in-memory indexer from SurrealDB without restart.
///
/// Replays all stored events through the indexer, picking up any events
/// that were written to SurrealDB by external processes (MCP subprocesses)
/// but missed by the running server's event pipeline.
pub async fn rehydrate_indexer(State(state): State<AppState>) -> impl IntoResponse {
    let events = match state.persistence().get_all_events().await {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("Rehydration failed — could not read events: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to read events: {}", e)
                })),
            );
        }
    };

    let total_events = events.len();
    let new_events;

    {
        let mut indexer = state.indexer_mut().await;
        let before = indexer.events_processed();
        for event in &events {
            if let Err(e) = indexer.process_event(event) {
                tracing::warn!("Rehydration skipped event {}: {}", event.id, e);
            }
        }
        new_events = indexer.events_processed() - before;
    }

    tracing::info!(
        "Rehydration complete: {} total events, {} new",
        total_events,
        new_events
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "total_events": total_events,
            "new_events": new_events
        })),
    )
}

// ============================================================================
// Inbox Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct InboxQuery {
    #[serde(default = "default_true")]
    pub unread_only: bool,
    #[serde(default = "default_limit")]
    pub limit: u32,
    pub from: Option<String>,
    /// If true, return messages without advancing the read cursor.
    /// Used by hooks to peek at unread messages for alert content.
    #[serde(default)]
    pub peek: bool,
}

fn default_true() -> bool {
    true
}

fn default_limit() -> u32 {
    20
}

/// Get inbox for an agent
pub async fn get_inbox(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    Query(query): Query<InboxQuery>,
) -> impl IntoResponse {
    // Get server-side read cursor for unread filtering
    let read_cursor = if query.unread_only {
        state.persistence().get_read_cursor(&agent).await.unwrap_or(None)
    } else {
        None
    };

    // Use indexer for O(1) lookup of messages sent TO this agent
    let indexer = state.indexer().await;
    let messages_clone: Vec<_> = indexer
        .get_messages_to_agent(&agent)
        .into_iter()
        .cloned()
        .collect();
    drop(indexer);

    let mut filtered: Vec<_> = messages_clone
        .into_iter()
        .filter(|msg| {
            // Filter by sender if specified
            if let Some(ref from) = query.from {
                if &msg.from != from {
                    return false;
                }
            }
            // Filter by read cursor (unread_only): only show messages newer than cursor
            if let Some(ref cursor_id) = read_cursor {
                // UUIDv7 event IDs are lexicographically ordered by time
                if msg.id.as_str() <= cursor_id.as_str() {
                    return false;
                }
            }
            true
        })
        .collect();

    // Sort by timestamp (most recent first), then apply limit
    filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let total_count = filtered.len();
    filtered.truncate(query.limit as usize);

    // Read cursor is NOT auto-advanced by inbox reads.
    // Agents must explicitly call POST /api/inbox/{agent}/ack or the
    // acknowledge_messages MCP tool after processing messages.
    // This prevents the cursor race condition where non-agent readers
    // (orchestrator, comms-check, hooks) silently advance the cursor.

    let messages: Vec<_> = filtered
        .into_iter()
        .map(|msg| {
            serde_json::json!({
                "id": msg.id,
                "thread_id": msg.thread_id,
                "from": msg.from,
                "to": msg.to,
                "subject": msg.subject,
                "content": msg.content,
                "intent": msg.intent,
                "priority": msg.priority,
                "cc": msg.cc,
                "timestamp": msg.created_at
            })
        })
        .collect();

    Json(serde_json::json!({
        "agent": agent,
        "messages": messages,
        "unread_count": total_count,
        "total_count": total_count
    }))
}

// ============================================================================
// Inbox Acknowledgment
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct AckRequest {
    /// ID of the newest message to acknowledge. All messages up to and
    /// including this ID are marked as read.
    pub message_id: String,
}

/// Explicitly acknowledge messages up to a given ID.
///
/// This is the recommended way to advance the read cursor — agents call
/// this after processing messages rather than relying on auto-advance.
pub async fn acknowledge_inbox(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    Json(req): Json<AckRequest>,
) -> impl IntoResponse {
    match state
        .persistence()
        .update_read_cursor(&agent, &req.message_id)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "agent": agent,
                "acknowledged_up_to": req.message_id,
                "status": "ok"
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": {
                    "code": "ACK_FAILED",
                    "message": format!("Failed to acknowledge: {}", e)
                }
            })),
        ),
    }
}

// ============================================================================
// Thread Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ThreadsQuery {
    #[serde(default = "default_active")]
    pub status: String,
    pub participant: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_active() -> String {
    "active".to_string()
}

/// List threads
pub async fn list_threads(
    State(state): State<AppState>,
    Query(query): Query<ThreadsQuery>,
) -> impl IntoResponse {
    // Use indexer for O(1) lookup of all threads
    let indexer = state.indexer().await;
    let threads_clone: Vec<_> = indexer.get_all_threads().into_iter().cloned().collect();
    drop(indexer);

    // Filter by participant if specified
    let filtered: Vec<_> = threads_clone
        .into_iter()
        .filter(|thread| {
            if let Some(ref participant) = query.participant {
                thread.participants.contains(participant)
            } else {
                true
            }
        })
        .collect();

    // Sort by created_at (newest first) and apply pagination
    let mut thread_list: Vec<_> = filtered
        .into_iter()
        .map(|thread| {
            serde_json::json!({
                "id": thread.id,
                "subject": thread.subject,
                "created_at": thread.created_at.to_rfc3339(),
                "participants": thread.participants,
                "status": thread.status,
                "message_count": thread.message_count
            })
        })
        .collect();

    thread_list.sort_by(|a, b| {
        let a_time = a.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        let b_time = b.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        b_time.cmp(a_time)
    });

    let total = thread_list.len();
    let offset = query.offset as usize;
    let limit = query.limit as usize;
    let threads: Vec<_> = thread_list.into_iter().skip(offset).take(limit).collect();

    Json(serde_json::json!({
        "threads": threads,
        "total": total
    }))
}

/// Get a single thread with messages
pub async fn get_thread(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Use indexer for O(1) lookup
    let indexer = state.indexer().await;

    let thread_clone = match indexer.get_thread(&id) {
        Some(t) => t.clone(),
        None => {
            return Json(serde_json::json!({
                "error": {
                    "code": "NOT_FOUND",
                    "message": format!("Thread not found: {}", id)
                }
            }))
        }
    };

    let messages_clone: Vec<_> = indexer
        .get_messages_for_thread(&id)
        .into_iter()
        .cloned()
        .collect();
    drop(indexer);

    let message_count = messages_clone.len();

    Json(serde_json::json!({
        "thread_id": id,
        "subject": thread_clone.subject,
        "participants": thread_clone.participants,
        "status": thread_clone.status,
        "created_at": thread_clone.created_at.to_rfc3339(),
        "messages": messages_clone.into_iter().map(|msg| {
            serde_json::json!({
                "id": msg.id,
                "from": msg.from,
                "to": msg.to,
                "subject": msg.subject,
                "content": msg.content,
                "intent": msg.intent,
                "priority": msg.priority,
                "created_at": msg.created_at.to_rfc3339()
            })
        }).collect::<Vec<_>>(),
        "message_count": message_count
    }))
}
#[derive(Debug, Deserialize)]
pub struct CreateThreadRequest {
    pub subject: String,
    #[serde(alias = "from")]
    pub from_agent: String,
    #[serde(alias = "to")]
    pub to_agent: String,
    pub content: String,
    #[serde(default = "default_normal")]
    pub priority: String,
    #[serde(default = "default_inform")]
    pub intent: String,
    /// Optional thread ID to append to an existing thread instead of creating a new one.
    pub thread_id: Option<String>,
    #[serde(default = "default_none_response")]
    pub expected_response: String,
    #[serde(default)]
    pub require_ack: bool,
    /// CC recipients — additional agents who should see this message
    #[serde(default)]
    pub cc: Vec<String>,
}

/// Adjutant mirroring rules — auto-inject CC for tiered agents
fn apply_adjutant_mirroring(to: &str, cc: &mut Vec<String>) {
    // If addressed to hypatia, auto-CC hypatia-adjutant
    if to == "hypatia" && !cc.contains(&"hypatia-adjutant".to_string()) {
        cc.push("hypatia-adjutant".to_string());
    }
    // If addressed to hypatia-adjutant, auto-CC hypatia
    if to == "hypatia-adjutant" && !cc.contains(&"hypatia".to_string()) {
        cc.push("hypatia".to_string());
    }
}

fn default_normal() -> String {
    "normal".to_string()
}

fn parse_priority(s: &str) -> Priority {
    match s {
        "low" => Priority::Low,
        "high" => Priority::High,
        "critical" => Priority::Critical,
        _ => Priority::Normal,
    }
}

fn parse_intent(s: &str) -> MessageIntent {
    match s {
        "discuss" => MessageIntent::Discuss,
        "request" => MessageIntent::Request,
        _ => MessageIntent::Inform,
    }
}

fn default_inform() -> String {
    "inform".to_string()
}

fn default_none_response() -> String {
    "none".to_string()
}

fn parse_expected_response(s: &str) -> ExpectedResponse {
    match s {
        "reply" => ExpectedResponse::Reply,
        "ack" => ExpectedResponse::Ack,
        "comply" => ExpectedResponse::Comply,
        "standby" => ExpectedResponse::Standby,
        _ => ExpectedResponse::None,
    }
}

/// Create a new thread
pub async fn create_thread(
    State(state): State<AppState>,
    Extension(caller): Extension<AuthenticatedAgent>,
    Json(req): Json<CreateThreadRequest>,
) -> impl IntoResponse {
    // RA-008: Enforce identity binding — from_agent must match authenticated token
    if !caller.is_privileged && caller.agent_id != "anonymous" && caller.agent_id != req.from_agent {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": {
                    "code": "IDENTITY_MISMATCH",
                    "message": format!(
                        "Authenticated as '{}' but from_agent claims '{}'. Use your own identity or a privileged token.",
                        caller.agent_id, req.from_agent
                    )
                }
            })),
        );
    }

    let event_id = Uuid::now_v7();
    let now = Utc::now();

    let priority = parse_priority(&req.priority);
    let intent = parse_intent(&req.intent);

    // Apply adjutant mirroring rules
    let mut cc = req.cc;
    apply_adjutant_mirroring(&req.to_agent, &mut cc);

    let event = EventEnvelope {
        id: event_id,
        timestamp: now,
        event_type: EventType::MessageSent,
        agent_id: req.from_agent.clone(),
        payload: EventPayload::Message(MessageEvent {
            from: req.from_agent,
            to: req.to_agent,
            subject: req.subject,
            content: req.content,
            thread_id: req.thread_id.clone(),
            priority,
            intent,
            expected_response: parse_expected_response(&req.expected_response),
            require_ack: req.require_ack,
            cc,
        }),
    };

    // Persist to SurrealDB
    if let Err(e) = state.persistence().store_event(&event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": { "code": "STORE_FAILED", "message": format!("Failed to persist event: {}", e) }
            })),
        );
    }

    // Update in-memory indexer
    {
        let mut indexer = state.indexer_mut().await;
        if let Err(e) = indexer.process_event(&event) {
            warn!("Indexer failed to process event: {}", e);
        }
    }

    // Extract message fields — clone for NATS and SSE push
    let (msg_to, msg_from, msg_subject, msg_intent) = match &event.payload {
        EventPayload::Message(m) => (m.to.clone(), m.from.clone(), m.subject.clone(), m.intent.clone()),
        _ => unreachable!(),
    };
    let (sse_to, sse_from, sse_subject, sse_intent) = (
        msg_to.clone(), msg_from.clone(), msg_subject.clone(), format!("{:?}", msg_intent),
    );
    let event_for_js = event.clone();

    // Broadcast to WebSocket listeners
    state.broadcast_event(event);

    // Publish to JetStream AGENT_MESSAGES for durable cross-process sync (Phase 2)
    {
        let nats_guard = state.nats_client_mut().await;
        if let Some(ref client) = *nats_guard {
            if let Err(e) = client.publish_message_event(&msg_to, &event_for_js).await {
                warn!("JetStream message publish failed: {} (SurrealDB is authoritative)", e);
            }
        }
    }

    // Publish NATS message notification (ephemeral hint for live sessions)
    {
        let nats_guard = state.nats_client_mut().await;
        if let Some(ref client) = *nats_guard {
            let notification = MessageNotification {
                event_id: event_id.to_string(),
                from: msg_from,
                subject: msg_subject,
                intent: msg_intent,
                timestamp: now,
            };
            if let Err(e) = client.publish_message_notification(&msg_to, &notification).await {
                warn!("NATS message notification failed: {}", e);
            }
        }
    }

    // Push to connected Streamable HTTP agents via PushBroker → Peer notification
    state.push_broker().publish(
        &sse_to,
        crate::mcp::streamable_http::PushEvent {
            from: sse_from,
            subject: sse_subject,
            intent: sse_intent,
            message_id: event_id.to_string(),
        },
    ).await;

    // Thread ID = provided thread_id or event ID (indexer convention when thread_id is None)
    let thread_id = req.thread_id.unwrap_or_else(|| event_id.to_string());

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "thread_id": thread_id,
            "message_id": event_id.to_string(),
            "created_at": now
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct UpdateThreadRequest {
    pub status: Option<String>,
    pub reason: Option<String>,
}

/// Update thread status
pub async fn update_thread(
    Path(id): Path<String>,
    Json(req): Json<UpdateThreadRequest>,
) -> impl IntoResponse {
    // TODO: Update thread in event log
    Json(serde_json::json!({
        "thread_id": id,
        "status": req.status.unwrap_or_else(|| "active".to_string()),
        "updated_at": chrono::Utc::now(),
        "_stub": true
    }))
}

#[derive(Debug, Deserialize)]
pub struct ReplyRequest {
    #[serde(alias = "from")]
    pub from_agent: String,
    pub content: String,
    #[serde(default = "default_normal")]
    pub priority: String,
    #[serde(default = "default_inform")]
    pub intent: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default = "default_none_response")]
    pub expected_response: String,
    #[serde(default)]
    pub require_ack: bool,
    /// CC recipients for the reply
    #[serde(default)]
    pub cc: Option<Vec<String>>,
}

/// Reply to a thread
pub async fn reply_to_thread(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Extension(caller): Extension<AuthenticatedAgent>,
    Json(req): Json<ReplyRequest>,
) -> impl IntoResponse {
    // RA-008: Enforce identity binding — from_agent must match authenticated token
    if !caller.is_privileged && caller.agent_id != "anonymous" && caller.agent_id != req.from_agent {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": {
                    "code": "IDENTITY_MISMATCH",
                    "message": format!(
                        "Authenticated as '{}' but from_agent claims '{}'. Use your own identity or a privileged token.",
                        caller.agent_id, req.from_agent
                    )
                }
            })),
        );
    }

    let event_id = Uuid::now_v7();
    let now = Utc::now();

    // Look up thread to find the recipient and subject (with SurrealDB fallback — RC1 fix)
    let found_in_indexer = {
        let indexer = state.indexer().await;
        indexer.get_thread(&thread_id).is_some()
    };

    if !found_in_indexer {
        // Fallback: query SurrealDB for thread events and feed into Indexer
        match state.persistence().get_events_by_thread_id(&thread_id).await {
            Ok(events) if !events.is_empty() => {
                let mut indexer = state.indexer_mut().await;
                for event in &events {
                    if let Err(e) = indexer.process_event(event) {
                        tracing::warn!(
                            "Indexer self-heal failed for event {}: {}",
                            event.id, e
                        );
                    }
                }
            }
            Ok(_) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": { "code": "NOT_FOUND", "message": format!("Thread not found: {}", thread_id) }
                    })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": { "code": "DB_ERROR", "message": format!("DB thread lookup failed: {}", e) }
                    })),
                );
            }
        }
    }

    let (to_agent, subject) = {
        let indexer = state.indexer().await;
        match indexer.get_thread(&thread_id) {
            Some(thread) => {
                let to = if thread.participants.len() > 2 {
                    "council".to_string()
                } else {
                    thread
                        .participants
                        .iter()
                        .find(|p| *p != &req.from_agent)
                        .cloned()
                        .unwrap_or_else(|| req.from_agent.clone())
                };
                (to, thread.subject.clone())
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": { "code": "NOT_FOUND", "message": format!("Thread not found after DB fallback: {}", thread_id) }
                    })),
                );
            }
        }
    };

    let priority = parse_priority(&req.priority);
    let intent = parse_intent(&req.intent);

    // Apply adjutant mirroring rules for replies
    let mut cc = req.cc.clone().unwrap_or_default();
    apply_adjutant_mirroring(&to_agent, &mut cc);

    let event = EventEnvelope {
        id: event_id,
        timestamp: now,
        event_type: EventType::MessageSent,
        agent_id: req.from_agent.clone(),
        payload: EventPayload::Message(MessageEvent {
            from: req.from_agent,
            to: to_agent,
            subject,
            content: req.content,
            thread_id: Some(thread_id.clone()),
            priority,
            intent,
            expected_response: parse_expected_response(&req.expected_response),
            require_ack: req.require_ack,
            cc,
        }),
    };

    // Persist to SurrealDB
    if let Err(e) = state.persistence().store_event(&event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": { "code": "STORE_FAILED", "message": format!("Failed to persist event: {}", e) }
            })),
        );
    }

    // Update in-memory indexer
    {
        let mut indexer = state.indexer_mut().await;
        if let Err(e) = indexer.process_event(&event) {
            warn!("Indexer failed to process event: {}", e);
        }
    }

    // Extract message fields and clone event for NATS before broadcast consumes it
    let (reply_to, reply_from, reply_subject, reply_intent) = match &event.payload {
        EventPayload::Message(m) => (m.to.clone(), m.from.clone(), m.subject.clone(), m.intent.clone()),
        _ => unreachable!(),
    };
    let event_for_js = event.clone();

    // Broadcast to WebSocket listeners
    state.broadcast_event(event);

    // Publish to JetStream AGENT_MESSAGES for durable cross-process sync (Phase 2)
    {
        let nats_guard = state.nats_client_mut().await;
        if let Some(ref client) = *nats_guard {
            if let Err(e) = client.publish_message_event(&reply_to, &event_for_js).await {
                warn!("JetStream message publish failed: {} (SurrealDB is authoritative)", e);
            }
        }
    }

    // Publish NATS message notification (ephemeral hint for live sessions)
    {
        let nats_guard = state.nats_client_mut().await;
        if let Some(ref client) = *nats_guard {
            let notification = MessageNotification {
                event_id: event_id.to_string(),
                from: reply_from,
                subject: reply_subject,
                intent: reply_intent,
                timestamp: now,
            };
            if let Err(e) = client.publish_message_notification(&reply_to, &notification).await {
                warn!("NATS message notification failed: {}", e);
            }
        }
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "message_id": event_id.to_string(),
            "thread_id": thread_id,
            "created_at": now
        })),
    )
}

// ============================================================================
// Message Handlers
// ============================================================================

/// Get a single message
pub async fn get_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Use indexer for O(1) lookup
    let indexer = state.indexer().await;
    let message_clone = match indexer.get_message(&id) {
        Some(m) => m.clone(),
        None => {
            return Json(serde_json::json!({
                "error": {
                    "code": "NOT_FOUND",
                    "message": format!("Message not found: {}", id)
                }
            }))
        }
    };
    drop(indexer);

    Json(serde_json::json!({
        "message_id": id,
        "thread_id": message_clone.thread_id,
        "from_agent": message_clone.from,
        "to_agent": message_clone.to,
        "content": message_clone.content,
        "priority": message_clone.priority,
        "intent": message_clone.intent,
        "expected_response": message_clone.expected_response,
        "require_ack": message_clone.require_ack,
        "created_at": message_clone.created_at.to_rfc3339(),
        "read_at": null
    }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateMessageRequest {
    pub read: Option<bool>,
}

/// Update message (mark read)
pub async fn update_message(
    Path(id): Path<String>,
    Json(req): Json<UpdateMessageRequest>,
) -> impl IntoResponse {
    // TODO: Update message in event log
    Json(serde_json::json!({
        "message_id": id,
        "read": req.read.unwrap_or(false),
        "updated_at": chrono::Utc::now(),
        "_stub": true
    }))
}

// ============================================================================
// Artifact Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ArtifactsQuery {
    pub shared_by: Option<String>,
    #[serde(default = "default_artifact_limit")]
    pub limit: u32,
}

fn default_artifact_limit() -> u32 {
    50
}

/// List artifacts
pub async fn list_artifacts(
    State(state): State<AppState>,
    Query(query): Query<ArtifactsQuery>,
) -> impl IntoResponse {
    // Use indexer for O(1) lookup
    let indexer = state.indexer().await;
    let artifacts_clone: Vec<_> = indexer.get_artifacts().into_iter().cloned().collect();
    drop(indexer);

    let filtered: Vec<_> = artifacts_clone
        .into_iter()
        .filter(|artifact| {
            if let Some(ref shared_by) = query.shared_by {
                &artifact.shared_by == shared_by
            } else {
                true
            }
        })
        .take(query.limit as usize)
        .map(|artifact| {
            serde_json::json!({
                "id": artifact.id,
                "path": artifact.path,
                "description": artifact.description,
                "checksum": artifact.checksum,
                "shared_by": artifact.shared_by,
                "shared_at": artifact.shared_at.to_rfc3339()
            })
        })
        .collect();

    Json(serde_json::json!({
        "artifacts": filtered,
        "total": filtered.len()
    }))
}

/// Get artifact content
pub async fn get_artifact(Path(path): Path<String>) -> impl IntoResponse {
    // TODO: Serve artifact file
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": {
                "code": "NOT_FOUND",
                "message": format!("Artifact not found: {}", path)
            },
            "_stub": true
        })),
    )
}

// ============================================================================
// Decision Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct DecisionsQuery {
    pub q: Option<String>,
    #[serde(default = "default_all")]
    pub status: String,
    pub thread_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_all() -> String {
    "all".to_string()
}

/// List decisions
pub async fn list_decisions(
    State(state): State<AppState>,
    Query(query): Query<DecisionsQuery>,
) -> impl IntoResponse {
    // Use indexer for O(1) lookup
    let indexer = state.indexer().await;
    let decisions_clone: Vec<_> = indexer.get_decisions().into_iter().cloned().collect();
    drop(indexer);

    let filtered: Vec<_> = decisions_clone
        .into_iter()
        .filter(|decision| {
            // Filter by search query if provided
            if let Some(ref q) = query.q {
                let q_lower = q.to_lowercase();
                if !decision.title.to_lowercase().contains(&q_lower)
                    && !decision.context.to_lowercase().contains(&q_lower)
                {
                    return false;
                }
            }

            // Filter by thread_id if provided
            if let Some(ref tid) = query.thread_id {
                if decision.thread_id.as_ref() != Some(tid) {
                    return false;
                }
            }

            true
        })
        .take(query.limit as usize)
        .map(|decision| {
            serde_json::json!({
                "id": decision.id,
                "subject": decision.title,
                "context": decision.context,
                "chosen": decision.chosen,
                "status": decision.status,
                "created_at": decision.created_at.to_rfc3339()
            })
        })
        .collect();

    Json(serde_json::json!({
        "decisions": filtered,
        "total": filtered.len()
    }))
}

/// Get a single decision
pub async fn get_decision(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Use indexer for O(1) lookup
    let indexer = state.indexer().await;
    let decision_clone = match indexer.get_decision(&id) {
        Some(d) => d.clone(),
        None => {
            return Json(serde_json::json!({
                "error": {
                    "code": "NOT_FOUND",
                    "message": format!("Decision not found: {}", id)
                }
            }))
        }
    };
    drop(indexer);

    Json(serde_json::json!({
        "decision_id": id,
        "subject": decision_clone.title,
        "context": decision_clone.context,
        "options": decision_clone.options,
        "chosen": decision_clone.chosen,
        "rationale": decision_clone.rationale,
        "status": decision_clone.status,
        "created_at": decision_clone.created_at.to_rfc3339()
    }))
}

#[derive(Debug, Deserialize)]
pub struct ApproveRequest {
    pub comment: Option<String>,
}

/// Approve a decision
pub async fn approve_decision(
    Path(id): Path<String>,
    Json(req): Json<ApproveRequest>,
) -> impl IntoResponse {
    // TODO: Update decision in event log
    Json(serde_json::json!({
        "decision_id": id,
        "status": "approved",
        "approved_at": chrono::Utc::now(),
        "comment": req.comment,
        "_stub": true
    }))
}

#[derive(Debug, Deserialize)]
pub struct RejectRequest {
    pub reason: String,
}

/// Reject a decision
pub async fn reject_decision(
    Path(id): Path<String>,
    Json(req): Json<RejectRequest>,
) -> impl IntoResponse {
    // TODO: Update decision in event log
    Json(serde_json::json!({
        "decision_id": id,
        "status": "rejected",
        "rejected_at": chrono::Utc::now(),
        "reason": req.reason,
        "_stub": true
    }))
}

// ============================================================================
// Merlin Action Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct InjectRequest {
    pub thread_id: String,
    pub content: String,
    #[serde(default = "default_comment")]
    pub action: String,
}

fn default_comment() -> String {
    "comment".to_string()
}

/// Inject a message into a thread
pub async fn inject_message(Json(req): Json<InjectRequest>) -> impl IntoResponse {
    // TODO: Add Merlin message to thread
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "message_id": "msg-merlin-stub",
            "thread_id": req.thread_id,
            "action": req.action,
            "injected_at": chrono::Utc::now(),
            "_stub": true
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct AnnotateRequest {
    pub target_type: String,
    pub target_id: String,
    pub content: String,
}

/// Add annotation
pub async fn add_annotation(Json(req): Json<AnnotateRequest>) -> impl IntoResponse {
    // TODO: Add annotation to event log
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "annotation_id": "ann-stub-123",
            "target_type": req.target_type,
            "target_id": req.target_id,
            "created_at": chrono::Utc::now(),
            "_stub": true
        })),
    )
}

/// Get configuration
pub async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    let config = state.config().await;
    Json(serde_json::json!({
        "mode": config.mode,
        "notify_on": config.notify_on,
        "gate": config.gate,
        "data_dir": config.data_dir,
        "port": config.port
    }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub mode: Option<String>,
}

/// Update configuration
pub async fn update_config(
    State(state): State<AppState>,
    Json(req): Json<UpdateConfigRequest>,
) -> impl IntoResponse {
    if let Some(mode_str) = &req.mode {
        let mode = match mode_str.as_str() {
            "passive" => crate::state::ObservationMode::Passive,
            "advisory" => crate::state::ObservationMode::Advisory,
            "gated" => crate::state::ObservationMode::Gated,
            _ => {
                return Json(serde_json::json!({
                    "error": {
                        "code": "INVALID_MODE",
                        "message": format!("Invalid mode: {}. Must be passive, advisory, or gated.", mode_str)
                    }
                }))
            }
        };
        state.set_mode(mode).await;
    }

    let config = state.config().await;
    Json(serde_json::json!({
        "mode": config.mode,
        "updated_at": chrono::Utc::now()
    }))
}

// ============================================================================
// Observation Handler (Laozi-Jung return path)
// ============================================================================

/// Observation types that Laozi-Jung can publish.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationType {
    /// Scan results for a project
    Scan,
    /// Pattern observations / insights
    Insight,
    /// Direction drift detected
    Drift,
    /// Onboarding brief generated
    Onboard,
}

impl std::fmt::Display for ObservationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scan => write!(f, "scan"),
            Self::Insight => write!(f, "insight"),
            Self::Drift => write!(f, "drift"),
            Self::Onboard => write!(f, "onboard"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ObserveRequest {
    /// Type of observation
    #[serde(rename = "type")]
    pub observation_type: ObservationType,
    /// Target project or agent name
    pub target: String,
    /// Short observation title
    pub title: String,
    /// Full observation content (markdown)
    pub content: String,
    /// Priority (defaults to normal)
    #[serde(default = "default_normal")]
    pub priority: String,
}

/// Submit an observation from a watcher agent.
///
/// Creates a message event and publishes to NATS `am.observe.{type}.{target}`.
/// This is the return path for observer agents like Laozi-Jung.
pub async fn submit_observation(
    State(state): State<AppState>,
    Json(req): Json<ObserveRequest>,
) -> impl IntoResponse {
    let event_id = Uuid::now_v7();
    let now = Utc::now();
    let obs_type = req.observation_type.to_string();
    let nats_subject = format!("am.observe.{}.{}", obs_type, req.target);

    let priority = parse_priority(&req.priority);

    let event = EventEnvelope {
        id: event_id,
        timestamp: now,
        event_type: EventType::MessageSent,
        agent_id: "laozi-jung".to_string(),
        payload: EventPayload::Message(MessageEvent {
            from: "laozi-jung".to_string(),
            to: "council".to_string(),
            subject: format!("[observe:{}] {}", obs_type, req.title),
            content: req.content,
            thread_id: None,
            priority,
            intent: MessageIntent::Inform,
            expected_response: ExpectedResponse::None,
            require_ack: false,
            cc: vec![],
        }),
    };

    // Persist to SurrealDB
    if let Err(e) = state.persistence().store_event(&event).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": { "code": "STORE_FAILED", "message": format!("Failed to persist: {}", e) }
            })),
        );
    }

    // Update in-memory indexer
    {
        let mut indexer = state.indexer_mut().await;
        if let Err(e) = indexer.process_event(&event) {
            warn!("Indexer failed to process observation: {}", e);
        }
    }

    // Broadcast to watchers, WebSocket, etc.
    state.broadcast_event(event);

    // Publish to JetStream AGENT_OBSERVATIONS stream (durable, 30-day retention)
    {
        let nats_guard = state.nats_client_mut().await;
        if let Some(ref client) = *nats_guard {
            let payload = serde_json::json!({
                "event_id": event_id.to_string(),
                "observation_type": obs_type,
                "target": req.target,
                "title": req.title,
                "timestamp": now,
            });
            if let Ok(bytes) = serde_json::to_vec(&payload) {
                match client
                    .jetstream()
                    .publish(nats_subject.clone(), bytes.into())
                    .await
                {
                    Ok(ack_future) => {
                        if let Err(e) = ack_future.await {
                            warn!("NATS observe JetStream ack failed: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("NATS observe JetStream publish failed: {}", e);
                    }
                }
            }
        }
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "event_id": event_id.to_string(),
            "nats_subject": nats_subject,
            "created_at": now
        })),
    )
}

// ============================================================================
// Search Handler
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_all")]
    pub r#type: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

/// Full-text search
pub async fn search(Query(query): Query<SearchQuery>) -> impl IntoResponse {
    // TODO: Implement search against database
    Json(serde_json::json!({
        "query": query.q,
        "results": [],
        "total": 0,
        "_stub": true,
        "_options": {
            "type": query.r#type,
            "limit": query.limit
        }
    }))
}

// ============================================================================
// Signed Event Handler (RA-004)
// ============================================================================

/// Accept and verify a signed event envelope.
///
/// Verifies Ed25519 signature, timestamp freshness (60s), and nonce uniqueness (120s TTL).
/// On success, the inner payload is processed as a standard event.
/// On failure, returns 401/403 with specific error code.
pub async fn submit_signed_event(
    State(state): State<AppState>,
    Json(envelope): Json<crate::crypto::envelope::SignedEnvelope>,
) -> impl IntoResponse {
    let keyring = state.keyring();
    let nonce_registry = state.nonce_registry();

    match envelope.verify(keyring, nonce_registry) {
        Ok(verified_agent) => {
            tracing::info!(
                "Signed event {} from '{}' verified successfully",
                envelope.event_id,
                verified_agent
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "verified",
                    "event_id": envelope.event_id,
                    "verified_agent": verified_agent,
                    "payload": envelope.payload
                })),
            )
        }
        Err(e) => {
            tracing::warn!(
                "Signed event {} rejected: {}",
                envelope.event_id,
                e
            );
            let (status, code) = match &e {
                crate::crypto::envelope::EnvelopeError::Expired { .. } => {
                    (StatusCode::UNAUTHORIZED, "ENVELOPE_EXPIRED")
                }
                crate::crypto::envelope::EnvelopeError::FutureTimestamp => {
                    (StatusCode::UNAUTHORIZED, "ENVELOPE_FUTURE")
                }
                crate::crypto::envelope::EnvelopeError::ReplayedNonce => {
                    (StatusCode::FORBIDDEN, "NONCE_REPLAYED")
                }
                crate::crypto::envelope::EnvelopeError::PayloadHashMismatch => {
                    (StatusCode::FORBIDDEN, "PAYLOAD_TAMPERED")
                }
                crate::crypto::envelope::EnvelopeError::UnknownAgent(_) => {
                    (StatusCode::UNAUTHORIZED, "UNKNOWN_AGENT")
                }
                crate::crypto::envelope::EnvelopeError::InvalidSignature => {
                    (StatusCode::FORBIDDEN, "INVALID_SIGNATURE")
                }
                crate::crypto::envelope::EnvelopeError::InvalidSignatureFormat => {
                    (StatusCode::BAD_REQUEST, "INVALID_SIGNATURE_FORMAT")
                }
                crate::crypto::envelope::EnvelopeError::InvalidTimestamp => {
                    (StatusCode::BAD_REQUEST, "INVALID_TIMESTAMP")
                }
            };
            (
                status,
                Json(serde_json::json!({
                    "error": {
                        "code": code,
                        "message": e.to_string()
                    }
                })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let state = AppState::new().await;
        let response = health_check(State(state)).await;
        let json = response.into_response();
        assert_eq!(json.status(), StatusCode::OK);
    }
}
