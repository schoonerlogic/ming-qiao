//! HTTP request handlers
//!
//! Handler functions for all API endpoints. Connected to the event log
//! for reading historical data.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::events::{EventPayload, EventReader, EventType};
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
pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "ming-qiao",
        "version": env!("CARGO_PKG_VERSION")
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
    let reader = match EventReader::open(state.events_path()) {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "agent": agent,
                "messages": [],
                "unread_count": 0,
                "total_count": 0
            }));
        }
    };

    let replay = match reader.replay() {
        Ok(r) => r,
        Err(e) => {
            return Json(serde_json::json!({
                "agent": agent,
                "error": format!("Failed to read events: {}", e)
            }));
        }
    };

    let mut messages = Vec::new();

    for event_result in replay {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.event_type != EventType::MessageSent {
            continue;
        }

        if let EventPayload::Message(ref msg) = event.payload {
            // Check if message is for this agent (or broadcast)
            if msg.to != agent && msg.to != "all" {
                continue;
            }

            // Filter by sender if specified
            if let Some(ref from) = query.from {
                if &msg.from != from {
                    continue;
                }
            }

            messages.push(serde_json::json!({
                "id": event.id.to_string(),
                "from": msg.from,
                "subject": msg.subject,
                "preview": msg.content.chars().take(100).collect::<String>(),
                "priority": format!("{:?}", msg.priority).to_lowercase(),
                "sent_at": event.timestamp,
                "thread_id": msg.thread_id
            }));

            if messages.len() >= query.limit as usize {
                break;
            }
        }
    }

    let total_count = messages.len();

    Json(serde_json::json!({
        "agent": agent,
        "messages": messages,
        "unread_count": total_count,
        "total_count": total_count
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
    let reader = match EventReader::open(state.events_path()) {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "threads": [],
                "total": 0
            }));
        }
    };

    let replay = match reader.replay() {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "threads": [],
                "total": 0
            }));
        }
    };

    // Collect unique threads from messages
    let mut threads: HashMap<String, serde_json::Value> = HashMap::new();

    for event_result in replay {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.event_type != EventType::MessageSent {
            continue;
        }

        if let EventPayload::Message(ref msg) = event.payload {
            // Filter by participant if specified
            if let Some(ref p) = query.participant {
                if &msg.from != p && &msg.to != p {
                    continue;
                }
            }

            // Use thread_id or message id as thread identifier
            let thread_key = msg
                .thread_id
                .clone()
                .unwrap_or_else(|| event.id.to_string());

            threads.entry(thread_key.clone()).or_insert_with(|| {
                serde_json::json!({
                    "id": thread_key,
                    "subject": msg.subject,
                    "started_by": msg.from,
                    "started_at": event.timestamp,
                    "participants": [&msg.from, &msg.to],
                    "status": "active",
                    "message_count": 1
                })
            });
        }
    }

    // Sort by timestamp and apply pagination
    let mut thread_list: Vec<_> = threads.into_values().collect();
    thread_list.sort_by(|a, b| {
        let a_time = a.get("started_at").and_then(|v| v.as_str()).unwrap_or("");
        let b_time = b.get("started_at").and_then(|v| v.as_str()).unwrap_or("");
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
    let reader = match EventReader::open(state.events_path()) {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "error": {
                    "code": "NOT_FOUND",
                    "message": format!("Thread not found: {}", id)
                }
            }));
        }
    };

    let replay = match reader.replay() {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "error": {
                    "code": "INTERNAL_ERROR",
                    "message": "Failed to read events"
                }
            }));
        }
    };

    let mut messages = Vec::new();
    let mut participants = std::collections::HashSet::new();
    let mut subject = String::new();
    let mut started_at = None;

    for event_result in replay {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.event_type != EventType::MessageSent {
            continue;
        }

        if let EventPayload::Message(ref msg) = event.payload {
            // Match thread_id or the original message id
            let msg_thread = msg
                .thread_id
                .clone()
                .unwrap_or_else(|| event.id.to_string());

            if msg_thread == id || event.id.to_string() == id {
                if subject.is_empty() {
                    subject = msg.subject.clone();
                    started_at = Some(event.timestamp);
                }

                participants.insert(msg.from.clone());
                participants.insert(msg.to.clone());

                messages.push(serde_json::json!({
                    "id": event.id.to_string(),
                    "from": msg.from,
                    "to": msg.to,
                    "content": msg.content,
                    "priority": format!("{:?}", msg.priority).to_lowercase(),
                    "sent_at": event.timestamp
                }));
            }
        }
    }

    if messages.is_empty() {
        return Json(serde_json::json!({
            "error": {
                "code": "NOT_FOUND",
                "message": format!("Thread not found: {}", id)
            }
        }));
    }

    Json(serde_json::json!({
        "thread_id": id,
        "subject": subject,
        "participants": participants.into_iter().collect::<Vec<_>>(),
        "status": "active",
        "started_at": started_at,
        "messages": messages,
        "message_count": messages.len()
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateThreadRequest {
    pub subject: String,
    pub from_agent: String,
    pub to_agent: String,
    pub content: String,
    #[serde(default = "default_normal")]
    pub priority: String,
}

fn default_normal() -> String {
    "normal".to_string()
}

/// Create a new thread
pub async fn create_thread(Json(req): Json<CreateThreadRequest>) -> impl IntoResponse {
    // TODO: Create thread in event log
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "thread_id": "thread-stub-123",
            "message_id": "msg-stub-123",
            "created_at": chrono::Utc::now(),
            "_stub": true,
            "_request": {
                "subject": req.subject,
                "from": req.from_agent,
                "to": req.to_agent
            }
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
    pub from_agent: String,
    pub content: String,
    #[serde(default = "default_normal")]
    pub priority: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
}

/// Reply to a thread
pub async fn reply_to_thread(
    Path(id): Path<String>,
    Json(req): Json<ReplyRequest>,
) -> impl IntoResponse {
    // TODO: Add message to thread in event log
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "message_id": "msg-stub-456",
            "thread_id": id,
            "sent_at": chrono::Utc::now(),
            "_stub": true,
            "_request": {
                "from": req.from_agent,
                "priority": req.priority
            }
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
    let reader = match EventReader::open(state.events_path()) {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "error": {
                    "code": "NOT_FOUND",
                    "message": format!("Message not found: {}", id)
                }
            }));
        }
    };

    let replay = match reader.replay() {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "error": {
                    "code": "INTERNAL_ERROR",
                    "message": "Failed to read events"
                }
            }));
        }
    };

    for event_result in replay {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.id.to_string() == id {
            if let EventPayload::Message(msg) = event.payload {
                return Json(serde_json::json!({
                    "message_id": id,
                    "thread_id": msg.thread_id,
                    "from_agent": msg.from,
                    "to_agent": msg.to,
                    "subject": msg.subject,
                    "content": msg.content,
                    "priority": format!("{:?}", msg.priority).to_lowercase(),
                    "sent_at": event.timestamp,
                    "read_at": null
                }));
            }
        }
    }

    Json(serde_json::json!({
        "error": {
            "code": "NOT_FOUND",
            "message": format!("Message not found: {}", id)
        }
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
    let reader = match EventReader::open(state.events_path()) {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "artifacts": [],
                "total": 0
            }));
        }
    };

    let replay = match reader.replay() {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "artifacts": [],
                "total": 0
            }));
        }
    };

    let mut artifacts = Vec::new();

    for event_result in replay {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.event_type != EventType::ArtifactShared {
            continue;
        }

        if let EventPayload::Artifact(ref artifact) = event.payload {
            // Filter by shared_by if provided
            if let Some(ref by) = query.shared_by {
                if &event.agent_id != by {
                    continue;
                }
            }

            artifacts.push(serde_json::json!({
                "id": event.id.to_string(),
                "path": artifact.path,
                "description": artifact.description,
                "checksum": artifact.checksum,
                "shared_by": event.agent_id,
                "shared_at": event.timestamp
            }));

            if artifacts.len() >= query.limit as usize {
                break;
            }
        }
    }

    let total = artifacts.len();

    Json(serde_json::json!({
        "artifacts": artifacts,
        "total": total
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
    let reader = match EventReader::open(state.events_path()) {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "decisions": [],
                "total": 0
            }));
        }
    };

    let replay = match reader.replay() {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "decisions": [],
                "total": 0
            }));
        }
    };

    let mut decisions = Vec::new();

    for event_result in replay {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.event_type != EventType::DecisionRecorded {
            continue;
        }

        if let EventPayload::Decision(ref decision) = event.payload {
            // Filter by search query if provided
            if let Some(ref q) = query.q {
                let q_lower = q.to_lowercase();
                if !decision.title.to_lowercase().contains(&q_lower)
                    && !decision.context.to_lowercase().contains(&q_lower)
                {
                    continue;
                }
            }

            // Filter by thread_id if provided
            if let Some(ref tid) = query.thread_id {
                if !decision.context.contains(tid) {
                    continue;
                }
            }

            let chosen_opt = decision
                .options
                .get(decision.chosen)
                .map(|o| o.description.as_str())
                .unwrap_or("(unknown)");

            decisions.push(serde_json::json!({
                "id": event.id.to_string(),
                "question": decision.title,
                "resolution": chosen_opt,
                "rationale": decision.rationale,
                "decided_by": event.agent_id,
                "decided_at": event.timestamp,
                "status": "approved"
            }));

            if decisions.len() >= query.limit as usize {
                break;
            }
        }
    }

    let total = decisions.len();

    Json(serde_json::json!({
        "decisions": decisions,
        "total": total
    }))
}

/// Get a single decision
pub async fn get_decision(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let reader = match EventReader::open(state.events_path()) {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "error": {
                    "code": "NOT_FOUND",
                    "message": format!("Decision not found: {}", id)
                }
            }));
        }
    };

    let replay = match reader.replay() {
        Ok(r) => r,
        Err(_) => {
            return Json(serde_json::json!({
                "error": {
                    "code": "INTERNAL_ERROR",
                    "message": "Failed to read events"
                }
            }));
        }
    };

    for event_result in replay {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if event.id.to_string() == id {
            if let EventPayload::Decision(decision) = event.payload {
                let chosen_opt = decision
                    .options
                    .get(decision.chosen)
                    .map(|o| o.description.as_str())
                    .unwrap_or("(unknown)");

                return Json(serde_json::json!({
                    "decision_id": id,
                    "question": decision.title,
                    "context": decision.context,
                    "options": decision.options.iter().map(|o| &o.description).collect::<Vec<_>>(),
                    "resolution": chosen_opt,
                    "rationale": decision.rationale,
                    "decided_by": event.agent_id,
                    "decided_at": event.timestamp,
                    "status": "approved"
                }));
            }
        }
    }

    Json(serde_json::json!({
        "error": {
            "code": "NOT_FOUND",
            "message": format!("Decision not found: {}", id)
        }
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
        let response = health_check().await;
        let json = response.into_response();
        assert_eq!(json.status(), StatusCode::OK);
    }
}
