//! WebSocket handler for real-time event streaming
//!
//! Provides a WebSocket endpoint that streams events to connected clients
//! in real-time. Clients can optionally filter events by type or agent.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::events::EventEnvelope;
use crate::state::AppState;

/// Query parameters for WebSocket connection
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    /// Filter events by agent ID (optional)
    pub agent: Option<String>,

    /// Filter events by type (optional, comma-separated)
    /// e.g., "message_sent,decision_recorded"
    pub event_types: Option<String>,
}

/// Message sent to WebSocket clients
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// A new event occurred
    Event { event: EventEnvelope },

    /// Connection established
    Connected { message: String },

    /// Error occurred
    Error { message: String },

    /// Ping/keepalive
    Ping,
}

/// WebSocket upgrade handler
///
/// Upgrades an HTTP connection to WebSocket and starts streaming events.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, query))
}

/// Handle an established WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState, query: WsQuery) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to the event broadcast channel
    let mut event_rx = state.subscribe_events();

    // Parse event type filters
    let event_type_filters: Option<Vec<String>> = query
        .event_types
        .map(|types| types.split(',').map(|s| s.trim().to_string()).collect());

    info!(
        agent_filter = ?query.agent,
        event_types = ?event_type_filters,
        "WebSocket client connected"
    );

    // Send connected message
    let connected_msg = WsMessage::Connected {
        message: "Connected to ming-qiao event stream".to_string(),
    };
    if let Ok(json) = serde_json::to_string(&connected_msg) {
        if sender.send(Message::Text(json)).await.is_err() {
            return; // Client disconnected
        }
    }

    // Spawn a task to handle incoming messages (for ping/pong and close)
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => {
                    debug!("WebSocket client sent close");
                    break;
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping");
                    // Pong is handled automatically by axum
                    let _ = data;
                }
                Ok(Message::Text(text)) => {
                    debug!(text = %text, "Received text message (ignored)");
                }
                Err(e) => {
                    warn!(error = %e, "WebSocket receive error");
                    break;
                }
                _ => {}
            }
        }
    });

    // Main event streaming loop
    loop {
        tokio::select! {
            // Check if receive task finished (client disconnected)
            _ = &mut recv_task => {
                info!("WebSocket client disconnected");
                break;
            }

            // Receive event from broadcast channel
            result = event_rx.recv() => {
                match result {
                    Ok(event) => {
                        // Apply filters
                        if let Some(ref agent) = query.agent {
                            if &event.agent_id != agent {
                                continue; // Skip events from other agents
                            }
                        }

                        if let Some(ref filters) = event_type_filters {
                            let event_type_str = format!("{:?}", event.event_type).to_lowercase();
                            if !filters.iter().any(|f| event_type_str.contains(f)) {
                                continue; // Skip non-matching event types
                            }
                        }

                        // Send event to client
                        let ws_msg = WsMessage::Event { event };
                        match serde_json::to_string(&ws_msg) {
                            Ok(json) => {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    info!("WebSocket send failed, client disconnected");
                                    break;
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "Failed to serialize event");
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "WebSocket client lagged, skipped events");
                        // Send warning to client
                        let warn_msg = WsMessage::Error {
                            message: format!("Lagged behind, skipped {} events", n),
                        };
                        if let Ok(json) = serde_json::to_string(&warn_msg) {
                            let _ = sender.send(Message::Text(json)).await;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Event broadcast channel closed");
                        break;
                    }
                }
            }
        }
    }

    // Cleanup
    recv_task.abort();
    info!("WebSocket handler finished");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_message_serialization() {
        let msg = WsMessage::Connected {
            message: "test".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"connected\""));
        assert!(json.contains("\"message\":\"test\""));
    }

    #[test]
    fn test_ws_message_error() {
        let msg = WsMessage::Error {
            message: "something went wrong".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"error\""));
    }
}
