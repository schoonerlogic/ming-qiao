//! HTTP request handlers
//!
//! Handler functions for all API endpoints. Now uses Indexer for O(1) lookups
//! instead of scanning the event log.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

use crate::events::{EventEnvelope, EventPayload, EventType, MessageEvent, Priority};
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
// Inbox Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct InboxQuery {
    #[serde(default = "default_true")]
    pub unread_only: bool,
    #[serde(default = "default_limit")]
    pub limit: u32,
    pub from: Option<String>,
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
    // Use indexer for O(1) lookup of messages sent TO this agent
    let indexer = state.indexer().await;
    let messages_clone: Vec<_> = indexer
        .get_messages_to_agent(&agent)
        .into_iter()
        .cloned()
        .collect();
    drop(indexer);

    let messages: Vec<_> = messages_clone
        .into_iter()
        .filter(|msg| {
            // Filter by sender if specified
            if let Some(ref from) = query.from {
                &msg.from == from
            } else {
                true
            }
        })
        .take(query.limit as usize)
        .map(|msg| {
            serde_json::json!({
                "id": msg.id,
                "thread_id": msg.thread_id,
                "from": msg.from,
                "content": msg.content,
                "timestamp": msg.created_at
            })
        })
        .collect();

    Json(serde_json::json!({
        "agent": agent,
        "messages": messages,
        "unread_count": messages.len(),
        "total_count": messages.len()
    }))
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
                "content": msg.content,
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
    /// Optional thread ID to append to an existing thread instead of creating a new one.
    pub thread_id: Option<String>,
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

/// Create a new thread
pub async fn create_thread(
    State(state): State<AppState>,
    Json(req): Json<CreateThreadRequest>,
) -> impl IntoResponse {
    let event_id = Uuid::now_v7();
    let now = Utc::now();

    let priority = parse_priority(&req.priority);

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

    // Broadcast to WebSocket listeners
    state.broadcast_event(event);

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
    #[serde(default)]
    pub artifact_refs: Vec<String>,
}

/// Reply to a thread
pub async fn reply_to_thread(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Json(req): Json<ReplyRequest>,
) -> impl IntoResponse {
    let event_id = Uuid::now_v7();
    let now = Utc::now();

    // Look up thread to find the recipient and subject
    let (to_agent, subject) = {
        let indexer = state.indexer().await;
        match indexer.get_thread(&thread_id) {
            Some(thread) => {
                let to = thread
                    .participants
                    .iter()
                    .find(|p| *p != &req.from_agent)
                    .cloned()
                    .unwrap_or_else(|| req.from_agent.clone());
                (to, thread.subject.clone())
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": { "code": "NOT_FOUND", "message": format!("Thread not found: {}", thread_id) }
                    })),
                );
            }
        }
    };

    let priority = parse_priority(&req.priority);

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

    // Broadcast to WebSocket listeners
    state.broadcast_event(event);

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
