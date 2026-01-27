//! Merlin-specific HTTP handlers
//!
//! Endpoints for the human operator (Merlin/Proteus) to receive notifications
//! and intervene in agent conversations.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::state::AppState;

/// WebSocket upgrade handler for Merlin notifications
///
/// Merlin connects to this endpoint to receive real-time notifications
/// based on observation mode (Advisory/Gated).
pub async fn merlin_notifications_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_merlin_socket(socket, state))
}

/// Handle an established Merlin WebSocket connection
async fn handle_merlin_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to Merlin notifications
    let mut notif_rx = state.merlin_notifier().subscribe();

    info!("Merlin connected to notification stream");

    // Send welcome message
    let welcome = serde_json::json!({
        "type": "connected",
        "message": "Connected to ming-qiao Merlin notifications",
        "mode": serde_json::to_value(state.mode().await).unwrap()
    });

    if let Ok(json) = serde_json::to_string(&welcome) {
        if sender.send(Message::Text(json)).await.is_err() {
            return;
        }
    }

    // Spawn task to handle incoming messages (Merlin interventions)
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => {
                    info!("Merlin client sent close");
                    break;
                }
                Ok(Message::Text(text)) => {
                    // Handle Merlin intervention messages
                    if let Ok(intervention) = serde_json::from_str::<MerlinIntervention>(&text) {
                        info!(intervention = ?intervention, "Received Merlin intervention");
                        // TODO: Process intervention (inject message, approve decision, etc.)
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Merlin WebSocket receive error");
                    break;
                }
                _ => {}
            }
        }
    });

    // Main notification loop
    loop {
        tokio::select! {
            _ = &mut recv_task => {
                info!("Merlin client disconnected");
                break;
            }

            result = notif_rx.recv() => {
                match result {
                    Ok(notif) => {
                        match serde_json::to_string(&notif) {
                            Ok(json) => {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    info!("Failed to send notification to Merlin");
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to serialize notification");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Merlin notification channel closed");
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Merlin client lagged, skipped notifications");
                    }
                }
            }
        }
    }

    recv_task.abort();
    info!("Merlin notification handler finished");
}

/// Merlin intervention message
///
/// Sent by Merlin to inject messages, approve decisions, or change mode.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MerlinIntervention {
    /// Inject a message into a thread
    InjectMessage {
        thread_id: String,
        from: String,
        content: String,
    },

    /// Approve a pending decision
    ApproveDecision {
        decision_id: String,
        reason: Option<String>,
    },

    /// Reject a pending decision
    RejectDecision {
        decision_id: String,
        reason: Option<String>,
    },

    /// Change observation mode
    SetMode {
        mode: String, // "passive", "advisory", or "gated"
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merlin_intervention_serialization() {
        let intervention = MerlinIntervention::SetMode {
            mode: "advisory".to_string(),
        };

        let json = serde_json::to_string(&intervention).unwrap();
        assert!(json.contains("\"action\":\"set_mode\""));
        assert!(json.contains("\"mode\":\"advisory\""));
    }
}
