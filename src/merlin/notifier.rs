//! Merlin notification implementation
//!
//! Sends notifications to Merlin (human operator) based on observation mode
//! and event triggers.

use crate::events::EventEnvelope;
use crate::state::{AppState, ObservationMode};
use tokio::sync::broadcast;

/// Merlin notification types
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MerlinNotification {
    /// High-priority message requires attention
    PriorityAlert {
        event: EventEnvelope,
        reason: String,
    },

    /// Keyword detected in message
    KeywordDetected {
        event: EventEnvelope,
        keyword: String,
    },

    /// Decision requires review
    DecisionReview {
        event: EventEnvelope,
        decision_type: String,
    },

    /// Agent blocked by gated mode
    ActionBlocked {
        event: EventEnvelope,
        reason: String,
    },

    /// System status update
    StatusUpdate {
        message: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

/// Merlin notification manager
///
/// Evaluates events against observation mode rules and sends notifications
/// to connected UI clients.
pub struct MerlinNotifier {
    /// Broadcast channel for notifications
    tx: broadcast::Sender<MerlinNotification>,
}

impl MerlinNotifier {
    /// Channel capacity for notifications
    const CHANNEL_CAPACITY: usize = 100;

    /// Create a new Merlin notifier
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(Self::CHANNEL_CAPACITY);
        Self { tx }
    }

    /// Subscribe to Merlin notifications
    ///
    /// UI components call this to receive notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<MerlinNotification> {
        self.tx.subscribe()
    }

    /// Check if an event should trigger a notification
    pub async fn should_notify(&self, event: &EventEnvelope, state: &AppState) -> bool {
        let config = state.config().await;
        let mode = config.mode;

        match mode {
            ObservationMode::Passive => false,
            ObservationMode::Advisory => self.check_advisory_triggers(event, &config),
            ObservationMode::Gated => true, // All events notify in gated mode
        }
    }

    /// Send notification for an event
    ///
    /// Called after event is written to log.
    pub fn notify(&self, event: EventEnvelope, state: &AppState) {
        // We need to block to get config since this is called from sync context
        let config = match std::thread::spawn({
            let state = state.clone();
            move || {
                tokio::runtime::Handle::try_current()
                    .and_then(|h| Ok(h.block_on(async { state.config().await })))
            }
        })
        .join()
        {
            Ok(Ok(c)) => c,
            _ => return,
        };

        if let Some(notif) = self.create_notification(event, &config) {
            let _ = self.tx.send(notif);
        }
    }

    /// Check advisory mode triggers
    fn check_advisory_triggers(
        &self,
        event: &EventEnvelope,
        config: &crate::state::Config,
    ) -> bool {
        use crate::events::EventPayload;

        // Check priority triggers
        if let EventPayload::Message(msg) = &event.payload {
            let priority_str = format!("{:?}", msg.priority).to_lowercase();
            if config.notify_on.priority.contains(&priority_str) {
                return true;
            }

            // Check keyword triggers
            for keyword in &config.notify_on.keywords {
                let content_lower = msg.content.to_lowercase();
                let subject_lower = msg.subject.to_lowercase();

                if content_lower.contains(&keyword.to_lowercase())
                    || subject_lower.contains(&keyword.to_lowercase())
                {
                    return true;
                }
            }
        }

        // Check decision type triggers
        if let EventPayload::Decision(dec) = &event.payload {
            for decision_type in &config.notify_on.decision_type {
                if dec
                    .title
                    .to_lowercase()
                    .contains(&decision_type.to_lowercase())
                {
                    return true;
                }
            }
        }

        false
    }

    /// Create appropriate notification based on event and mode
    fn create_notification(
        &self,
        event: EventEnvelope,
        config: &crate::state::Config,
    ) -> Option<MerlinNotification> {
        use crate::events::EventPayload;

        match config.mode {
            ObservationMode::Advisory => {
                // Priority-based notification
                if let EventPayload::Message(msg) = &event.payload {
                    let priority_str = format!("{:?}", msg.priority).to_lowercase();
                    if config.notify_on.priority.contains(&priority_str) {
                        return Some(MerlinNotification::PriorityAlert {
                            event,
                            reason: format!("Message has {} priority", priority_str),
                        });
                    }

                    // Keyword-based notification
                    for keyword in &config.notify_on.keywords {
                        let content_lower = msg.content.to_lowercase();
                        let subject_lower = msg.subject.to_lowercase();

                        if content_lower.contains(&keyword.to_lowercase())
                            || subject_lower.contains(&keyword.to_lowercase())
                        {
                            return Some(MerlinNotification::KeywordDetected {
                                event,
                                keyword: keyword.clone(),
                            });
                        }
                    }
                }

                // Decision notification
                if let EventPayload::Decision(dec) = &event.payload {
                    for decision_type in &config.notify_on.decision_type {
                        if dec
                            .title
                            .to_lowercase()
                            .contains(&decision_type.to_lowercase())
                        {
                            return Some(MerlinNotification::DecisionReview {
                                event,
                                decision_type: decision_type.clone(),
                            });
                        }
                    }
                }

                None
            }
            ObservationMode::Gated => {
                // Check if action should be blocked
                if let EventPayload::Decision(dec) = &event.payload {
                    for decision_type in &config.gate.decision_type {
                        if dec
                            .title
                            .to_lowercase()
                            .contains(&decision_type.to_lowercase())
                        {
                            return Some(MerlinNotification::ActionBlocked {
                                event,
                                reason: format!(
                                    "Decision type '{}' requires approval",
                                    decision_type
                                ),
                            });
                        }
                    }
                }

                // In gated mode, notify on all events
                Some(MerlinNotification::StatusUpdate {
                    message: "Event logged in gated mode".to_string(),
                    timestamp: chrono::Utc::now(),
                })
            }
            ObservationMode::Passive => None,
        }
    }
}

impl Default for MerlinNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merlin_notification_serialization() {
        let notif = MerlinNotification::StatusUpdate {
            message: "test".to_string(),
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"type\":\"status_update\""));
    }
}
