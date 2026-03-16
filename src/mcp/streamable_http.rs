//! MCP Streamable HTTP transport handler
//!
//! Implements the MCP Streamable HTTP transport at `/mcp`:
//! - POST: receives JSON-RPC messages (requests, notifications, responses)
//! - GET: opens SSE stream for server-to-client push notifications
//! - DELETE: terminates a session
//!
//! Security (Ogma review):
//! - Identity from auth (bearer token), not session ID
//! - Session-principal binding: session ID locked to authenticated agent
//! - SSE pushes metadata only (id, from, subject, intent) — not content
//! - Authenticated ack: only the inbox owner can acknowledge

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response, Sse};
use axum::Json;
use serde_json::Value;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::mcp::protocol::{
    CallToolParams, InitializeParams, InitializeResult, JsonRpcError,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, McpError, McpErrorCode,
    RequestId, ServerCapabilities, ServerInfo, ToolsCapability,
};
use crate::mcp::server::{PROTOCOL_VERSION, SERVER_NAME, SERVER_VERSION};
use crate::mcp::tools::ToolRegistry;
use crate::state::AppState;

// ============================================================================
// Session management
// ============================================================================

#[derive(Debug)]
pub struct McpSession {
    agent_id: String,
    push_tx: broadcast::Sender<SseEvent>,
}

#[derive(Debug, Clone)]
pub struct SseEvent {
    id: String,
    data: String,
}

pub type SessionStore = Arc<RwLock<HashMap<String, McpSession>>>;

pub fn new_session_store() -> SessionStore {
    Arc::new(RwLock::new(HashMap::new()))
}

// ============================================================================
// Auth helpers
// ============================================================================

async fn authenticate(headers: &HeaderMap, state: &AppState) -> Option<String> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let config = state.auth_config().await;
    config.validate_token(token).map(|s| s.to_string())
}

fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Resolve agent from session, verifying session-principal binding.
async fn agent_from_session(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<String, StatusCode> {
    // Identity comes from auth token (Ogma finding #2)
    let authed_agent = authenticate(headers, state).await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // If there's a session ID, verify it's bound to this agent (Ogma finding #4)
    if let Some(sid) = session_id_from_headers(headers) {
        let store = state.mcp_sessions().read().await;
        if let Some(session) = store.get(&sid) {
            if session.agent_id != authed_agent {
                warn!(
                    "Session-principal mismatch: session={} bound to {}, but auth says {}",
                    sid, session.agent_id, authed_agent
                );
                return Err(StatusCode::FORBIDDEN);
            }
        } else {
            return Err(StatusCode::NOT_FOUND); // Session expired
        }
    }

    Ok(authed_agent)
}

// ============================================================================
// POST /mcp
// ============================================================================

pub async fn handle_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid UTF-8").into_response();
        }
    };

    let raw: Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(JsonRpcResponse::error(
                RequestId::Null,
                JsonRpcError {
                    code: McpErrorCode::ParseError.code(),
                    message: format!("Parse error: {}", e),
                    data: None,
                },
            ))).into_response();
        }
    };

    // Batch not yet supported
    if raw.is_array() {
        return (StatusCode::BAD_REQUEST, Json(JsonRpcResponse::error(
            RequestId::Null,
            JsonRpcError {
                code: -32600,
                message: "Batch requests not yet supported".to_string(),
                data: None,
            },
        ))).into_response();
    }

    // Notification (no id) → 202 Accepted
    if raw.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    // Parse request
    let request: JsonRpcRequest = match serde_json::from_value(raw) {
        Ok(req) => req,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(JsonRpcResponse::error(
                RequestId::Null,
                JsonRpcError {
                    code: McpErrorCode::ParseError.code(),
                    message: format!("Parse error: {}", e),
                    data: None,
                },
            ))).into_response();
        }
    };

    // Dispatch
    let mut response = dispatch(&request, &headers, &state).await;

    // If initialize, extract session ID into header and remove from body
    if request.method == "initialize" {
        if let Some(result) = response.result.as_mut() {
            if let Some(sid) = result.get("_mcpSessionId").and_then(|v| v.as_str()).map(|s| s.to_string()) {
                // Remove the transient field from the response body
                result.as_object_mut().map(|obj| obj.remove("_mcpSessionId"));

                let mut http_resp = Json(&response).into_response();
                if let Ok(val) = sid.parse() {
                    http_resp.headers_mut().insert("mcp-session-id", val);
                }
                return http_resp;
            }
        }
    }

    Json(&response).into_response()
}

async fn dispatch(
    request: &JsonRpcRequest,
    headers: &HeaderMap,
    state: &AppState,
) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => {
            let agent_id = authenticate(headers, state).await
                .unwrap_or_else(|| "unknown".to_string());

            let params: InitializeParams = match &request.params {
                Some(p) => match serde_json::from_value(p.clone()) {
                    Ok(p) => p,
                    Err(e) => return JsonRpcResponse::error(
                        request.id.clone(),
                        JsonRpcError { code: -32602, message: e.to_string(), data: None },
                    ),
                },
                None => return JsonRpcResponse::error(
                    request.id.clone(),
                    JsonRpcError { code: -32602, message: "Missing params".to_string(), data: None },
                ),
            };

            info!("MCP Streamable HTTP initialize: agent={}, client={} v{}",
                agent_id, params.client_info.name, params.client_info.version);

            // Create session bound to this agent (Ogma finding #4)
            let session_id = Uuid::now_v7().to_string();
            let (push_tx, _) = broadcast::channel::<SseEvent>(256);

            state.mcp_sessions().write().await.insert(
                session_id.clone(),
                McpSession { agent_id, push_tx },
            );

            let result = InitializeResult {
                protocol_version: PROTOCOL_VERSION.to_string(),
                capabilities: ServerCapabilities {
                    tools: Some(ToolsCapability { list_changed: false }),
                    resources: None,
                    prompts: None,
                    logging: Some(serde_json::json!({})),
                },
                server_info: ServerInfo {
                    name: SERVER_NAME.to_string(),
                    version: SERVER_VERSION.to_string(),
                },
            };

            // Session ID goes in the Mcp-Session-Id response header only,
            // not in the body (per MCP spec). Store it for header injection.
            let result_val = serde_json::to_value(&result).unwrap();
            let mut response = JsonRpcResponse::success(request.id.clone(), result_val);
            // Stash session_id for the POST handler to extract into a header
            // Use a transient field that we'll strip before sending
            response.result.as_mut().unwrap()["_mcpSessionId"] = serde_json::json!(session_id);
            response
        }

        "shutdown" => {
            if let Some(sid) = session_id_from_headers(headers) {
                state.mcp_sessions().write().await.remove(&sid);
            }
            JsonRpcResponse::success(request.id.clone(), serde_json::json!(null))
        }

        "tools/list" => {
            let tools = ToolRegistry::list_definitions();
            JsonRpcResponse::success(request.id.clone(), serde_json::json!({ "tools": tools }))
        }

        "tools/call" => {
            let agent_id = match agent_from_session(headers, state).await {
                Ok(id) => id,
                Err(status) => {
                    return JsonRpcResponse::error(
                        request.id.clone(),
                        JsonRpcError {
                            code: -32600,
                            message: format!("Auth failed: {}", status),
                            data: None,
                        },
                    );
                }
            };

            let params: CallToolParams = match &request.params {
                Some(p) => match serde_json::from_value(p.clone()) {
                    Ok(p) => p,
                    Err(e) => return JsonRpcResponse::error(
                        request.id.clone(),
                        JsonRpcError { code: -32602, message: e.to_string(), data: None },
                    ),
                },
                None => return JsonRpcResponse::error(
                    request.id.clone(),
                    JsonRpcError { code: -32602, message: "Missing params".to_string(), data: None },
                ),
            };

            let registry = ToolRegistry::with_state(state.clone());
            match registry.call(&params.name, params.arguments, &agent_id).await {
                Ok(result) => {
                    use crate::mcp::protocol::ToolContent;
                    let content: Vec<Value> = result.content.into_iter().map(|c| match c {
                        ToolContent::Text { text } => serde_json::json!({"type": "text", "text": text}),
                        ToolContent::Image { data, mime_type } => serde_json::json!({"type": "image", "data": data, "mimeType": mime_type}),
                        ToolContent::Resource { uri, mime_type, text } => serde_json::json!({"type": "resource", "resource": {"uri": uri, "mimeType": mime_type, "text": text}}),
                    }).collect();

                    JsonRpcResponse::success(request.id.clone(), serde_json::json!({
                        "content": content,
                        "isError": result.is_error.unwrap_or(false)
                    }))
                }
                Err(e) => JsonRpcResponse::error(request.id.clone(), e.to_rpc_error()),
            }
        }

        _ => JsonRpcResponse::error(request.id.clone(), McpErrorCode::MethodNotFound.into()),
    }
}

// ============================================================================
// GET /mcp — SSE stream for server-to-client push
// ============================================================================

pub async fn handle_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let agent_id = match agent_from_session(&headers, &state).await {
        Ok(id) => id,
        Err(status) => return status.into_response(),
    };

    let session_id = match session_id_from_headers(&headers) {
        Some(sid) => sid,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let push_rx = {
        let store = state.mcp_sessions().read().await;
        match store.get(&session_id) {
            Some(session) => session.push_tx.subscribe(),
            None => return StatusCode::NOT_FOUND.into_response(),
        }
    };

    info!("SSE stream opened for agent={} session={}", agent_id, &session_id[..8]);

    let stream = async_stream::stream! {
        let mut rx = push_rx;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    yield Ok::<_, std::convert::Infallible>(
                        axum::response::sse::Event::default()
                            .id(event.id)
                            .data(event.data)
                    );
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("SSE stream lagged by {} events for {}", n, agent_id);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(30))
        )
        .into_response()
}

// ============================================================================
// DELETE /mcp — terminate session
// ============================================================================

pub async fn handle_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> StatusCode {
    if let Some(sid) = session_id_from_headers(&headers) {
        state.mcp_sessions().write().await.remove(&sid);
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    }
}

// ============================================================================
// Push notification to connected agents (metadata only — Ogma finding #3)
// ============================================================================

/// Push a message notification to a connected agent's SSE stream.
/// Sends metadata only (id, from, subject, intent) — NOT message content.
/// Agent calls check_messages to get the full content.
pub async fn push_message_notification(
    sessions: &SessionStore,
    to_agent: &str,
    event_id: &str,
    from: &str,
    subject: &str,
    intent: &str,
) {
    let notification = JsonRpcNotification::new(
        "notifications/message",
        Some(serde_json::json!({
            "id": event_id,
            "from": from,
            "subject": subject,
            "intent": intent,
        })),
    );

    let store = sessions.read().await;
    let session_count = store.len();
    let mut pushed = false;
    for session in store.values() {
        if session.agent_id == to_agent {
            let event = SseEvent {
                id: Uuid::now_v7().to_string(),
                data: serde_json::to_string(&notification).unwrap_or_default(),
            };
            match session.push_tx.send(event) {
                Ok(n) => {
                    info!("SSE push to {}: delivered to {} receiver(s)", to_agent, n);
                    pushed = true;
                }
                Err(_) => {
                    warn!("SSE push to {}: no active receivers", to_agent);
                }
            }
            return;
        }
    }
    if !pushed {
        debug!("SSE push to {}: no session found ({} total sessions)", to_agent, session_count);
    }
}
