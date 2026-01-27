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

use crate::events::{EventEnvelope, EventPayload, EventType, MessageEvent, Priority};
use crate::state::{AppState, ObservationMode};
use uuid::Uuid;

/// Process a Merlin intervention
///
/// Handles all intervention types from Merlin (inject messages, approve/reject decisions, change mode).
async fn process_intervention(
    intervention: MerlinIntervention,
    state: &AppState,
) -> Result<String, String> {
    match intervention {
        MerlinIntervention::InjectMessage {
            thread_id,
            from,
            content,
        } => {
            // Create message event from Merlin
            let message_event = MessageEvent {
                from: from.clone(),
                to: String::new(), // Will be populated from thread
                subject: format!("Merlin intervention"),
                content,
                thread_id: Some(thread_id.clone()),
                priority: Priority::High,
            };

            let event = EventEnvelope {
                id: Uuid::now_v7(),
                timestamp: chrono::Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: "merlin".to_string(),
                payload: EventPayload::Message(message_event),
            };

            // Write to event log
            state
                .event_writer()
                .append(&event)
                .map_err(|e| format!("Failed to write event: {}", e))?;

            // Broadcast and notify
            state.broadcast_event(event.clone());
            state.merlin_notifier().notify(event.clone(), state);

            // Refresh indexer
            let _ = state.refresh_indexer().await;

            Ok(format!("Message injected into thread {}", thread_id))
        }

        MerlinIntervention::ApproveDecision {
            decision_id,
            reason,
        } => {
            // TODO: Implement decision approval
            // For now, just log it
            info!(
                decision_id = %decision_id,
                reason = ?reason,
                "Decision approved"
            );
            Ok(format!("Decision {} approved", decision_id))
        }

        MerlinIntervention::RejectDecision {
            decision_id,
            reason,
        } => {
            // TODO: Implement decision rejection
            // For now, just log it
            info!(
                decision_id = %decision_id,
                reason = ?reason,
                "Decision rejected"
            );
            Ok(format!("Decision {} rejected", decision_id))
        }

        MerlinIntervention::SetMode { mode } => {
            // Parse mode
            let new_mode = match mode.as_str() {
                "passive" => ObservationMode::Passive,
                "advisory" => ObservationMode::Advisory,
                "gated" => ObservationMode::Gated,
                _ => return Err(format!("Invalid mode: {}", mode)),
            };

            // Update mode
            state.set_mode(new_mode).await;

            info!(mode = %mode, "Observation mode changed");
            Ok(format!("Mode changed to {}", mode))
        }
    }
}

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
    let state_clone = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => {
                    info!("Merlin client sent close");
                    break;
                }
                Ok(Message::Text(text)) => {
                    // Handle Merlin intervention messages
                    info!(raw_message = %text, "Received WebSocket message from Merlin");

                    match serde_json::from_str::<MerlinIntervention>(&text) {
                        Ok(intervention) => {
                            info!(intervention = ?intervention, "Parsed Merlin intervention");

                            // Process the intervention
                            match process_intervention(intervention, &state_clone).await {
                                Ok(msg) => info!(result = %msg, "Intervention succeeded"),
                                Err(e) => tracing::error!(error = %e, "Intervention failed"),
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                message = %text,
                                "Failed to parse Merlin intervention"
                            );
                        }
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
