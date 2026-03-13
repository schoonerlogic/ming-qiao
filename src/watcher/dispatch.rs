//! Watcher dispatcher — background task that routes events to watchers.
//!
//! Subscribes to the AppState broadcast channel and dispatches matching events
//! to each watcher's configured action (file append or webhook).

use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::events::EventPayload;
use crate::state::AppState;
use crate::watcher::actions::{FileAppendAction, SystemNotifyAction, WebhookAction};
use crate::watcher::config::{WatcherAction, WatcherConfig, WatcherRole};
use crate::watcher::subjects::{matches_subject, subjects_for_event};

/// A resolved watcher ready for dispatch.
enum ResolvedAction {
    FileAppend(FileAppendAction),
    Webhook(WebhookAction),
    SystemNotify(SystemNotifyAction),
}

struct ResolvedWatcher {
    agent: String,
    subjects: Vec<String>,
    event_types: Vec<String>,
    recipients: Vec<String>,
    action: ResolvedAction,
}

/// Background dispatcher that routes broadcast events to watchers.
///
/// Holds a `JoinHandle` to the background tokio task. When dropped, the task
/// is aborted for clean shutdown.
pub struct WatcherDispatcher {
    handle: JoinHandle<()>,
}

impl WatcherDispatcher {
    /// Start the dispatcher if any watchers are configured.
    ///
    /// Resolves all watcher configs at startup (opens files, creates HTTP
    /// clients). Per-watcher failures are logged and skipped.
    ///
    /// Returns `None` if no watchers are configured or all fail to resolve.
    pub async fn start(
        state: &AppState,
        watchers: &[WatcherConfig],
        project: &str,
    ) -> Option<Self> {
        if watchers.is_empty() {
            return None;
        }

        let mut resolved = Vec::new();

        for w in watchers {
            let action = match &w.action {
                WatcherAction::FileAppend { path } => match FileAppendAction::open(path).await {
                    Ok(a) => {
                        info!("Watcher '{}': file_append → {}", w.agent, path);
                        ResolvedAction::FileAppend(a)
                    }
                    Err(e) => {
                        warn!("Watcher '{}': failed to open {}: {}", w.agent, path, e);
                        continue;
                    }
                },
                WatcherAction::Webhook { url } => {
                    info!("Watcher '{}': webhook → {}", w.agent, url);
                    ResolvedAction::Webhook(WebhookAction::new(
                        url.clone(),
                        w.agent.clone(),
                    ))
                }
                WatcherAction::SystemNotify { title } => {
                    info!("Watcher '{}': system_notify → {}", w.agent, title);
                    ResolvedAction::SystemNotify(SystemNotifyAction::new(
                        title.clone(),
                    ))
                }
            };

            resolved.push(ResolvedWatcher {
                agent: w.agent.clone(),
                subjects: w.subjects.clone(),
                event_types: w.filter.event_types.clone(),
                recipients: w.filter.recipients.clone(),
                action,
            });
        }

        if resolved.is_empty() {
            warn!("All watcher configs failed to resolve, dispatcher not started");
            return None;
        }

        info!(
            "WatcherDispatcher starting with {} watcher(s)",
            resolved.len()
        );

        let mut rx = state.subscribe_events();
        let project = project.to_string();

        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let event_subjects = subjects_for_event(&event, &project);
                        let event_type_str = event.event_type.to_string();

                        for watcher in &resolved {
                            // Check event type filter
                            if !watcher.event_types.is_empty()
                                && !watcher.event_types.contains(&event_type_str)
                            {
                                continue;
                            }

                            // Check recipient filter (messages only)
                            if !watcher.recipients.is_empty() {
                                if let EventPayload::Message(m) = &event.payload {
                                    let matches_recipient = watcher.recipients.iter().any(|r| {
                                        r == &m.to
                                            || (r == "council" && m.to == "council")
                                            || (r == "all" && m.to == "all")
                                    });
                                    if !matches_recipient {
                                        continue;
                                    }
                                }
                            }

                            // Check subject patterns
                            let matches = event_subjects.iter().any(|subj| {
                                watcher
                                    .subjects
                                    .iter()
                                    .any(|pat| matches_subject(subj, pat))
                            });

                            if !matches {
                                continue;
                            }

                            match &watcher.action {
                                ResolvedAction::FileAppend(action) => {
                                    if let Err(e) = action.write_event(&event).await {
                                        warn!(
                                            "Watcher '{}' file write failed: {}",
                                            watcher.agent, e
                                        );
                                    }
                                }
                                ResolvedAction::Webhook(action) => {
                                    action.send_event(&event).await;
                                }
                                ResolvedAction::SystemNotify(action) => {
                                    action.notify(&event).await;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WatcherDispatcher lagged by {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Event channel closed, WatcherDispatcher stopping");
                        break;
                    }
                }
            }
        });

        Some(WatcherDispatcher { handle })
    }
}

impl Drop for WatcherDispatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Log a warning if an observer-role watcher attempts a write operation.
///
/// This is advisory only — no enforcement. Called from write paths (MCP tools,
/// HTTP handlers) when the acting agent matches an observer watcher.
pub fn warn_observer_write(agent_id: &str, watchers: &[WatcherConfig]) {
    for w in watchers {
        if w.agent == agent_id && w.role == WatcherRole::Observer {
            warn!(
                "Observer agent '{}' is performing a write operation",
                agent_id
            );
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::*;
    use crate::state::AppState;
    use crate::watcher::config::{WatcherAction, WatcherConfig, WatcherFilter};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_message_event() -> EventEnvelope {
        EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: "aleph".to_string(),
                to: "luban".to_string(),
                subject: "hello".to_string(),
                content: "world".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: MessageIntent::Inform,
                expected_response: ExpectedResponse::None,
                require_ack: false,
                claimed_source_model: None,
                claimed_source_runtime: None,
                claimed_source_mode: None,
                verified_source_model: None,
                verified_source_runtime: None,
                verified_source_mode: None,
                source_worktree: None,
                source_session_id: None,
                provenance_level: crate::events::ProvenanceLevel::Legacy,
                provenance_issuer: None,
            }),
        }
    }

    fn make_decision_event() -> EventEnvelope {
        EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::DecisionRecorded,
            agent_id: "thales".to_string(),
            payload: EventPayload::Decision(DecisionEvent {
                title: "Use Rust".to_string(),
                context: "Need speed".to_string(),
                options: vec![],
                chosen: 0,
                rationale: "Fast and safe".to_string(),
            }),
        }
    }

    #[tokio::test]
    async fn test_dispatcher_file_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stream.jsonl");
        let path_str = path.to_str().unwrap().to_string();

        let state = AppState::new().await;
        let watchers = vec![WatcherConfig {
            agent: "test-watcher".to_string(),
            role: WatcherRole::Observer,
            subjects: vec!["am.events.mingqiao".to_string()],
            filter: WatcherFilter::default(),
            action: WatcherAction::FileAppend {
                path: path_str.clone(),
            },
        }];

        let _dispatcher = WatcherDispatcher::start(&state, &watchers, "mingqiao")
            .await
            .expect("dispatcher should start");

        // Give the dispatcher time to subscribe
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Broadcast an event
        state.broadcast_event(make_message_event());

        // Give the dispatcher time to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["event_type"], "message_sent");
    }

    #[tokio::test]
    async fn test_dispatcher_event_type_filter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filtered.jsonl");
        let path_str = path.to_str().unwrap().to_string();

        let state = AppState::new().await;
        let watchers = vec![WatcherConfig {
            agent: "filter-watcher".to_string(),
            role: WatcherRole::Observer,
            subjects: vec!["am.events.mingqiao".to_string()],
            filter: WatcherFilter {
                event_types: vec!["decision_recorded".to_string()],
                recipients: vec![],
            },
            action: WatcherAction::FileAppend {
                path: path_str.clone(),
            },
        }];

        let _dispatcher = WatcherDispatcher::start(&state, &watchers, "mingqiao")
            .await
            .expect("dispatcher should start");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Broadcast a message event — should NOT match the filter
        state.broadcast_event(make_message_event());

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // File should be empty (created but no lines)
        let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        assert!(content.is_empty(), "Expected no lines, got: {}", content);

        // Now broadcast a decision — should match
        state.broadcast_event(make_decision_event());

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["event_type"], "decision_recorded");
    }

    #[test]
    fn test_warn_observer_write() {
        // Just verify it doesn't panic
        let watchers = vec![WatcherConfig {
            agent: "laozi-jung".to_string(),
            role: WatcherRole::Observer,
            subjects: vec![],
            filter: WatcherFilter::default(),
            action: WatcherAction::FileAppend {
                path: "/dev/null".to_string(),
            },
        }];

        warn_observer_write("laozi-jung", &watchers);
        warn_observer_write("aleph", &watchers); // no match, also doesn't panic
    }

    fn make_message_event_to(to: &str) -> EventEnvelope {
        EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "thales".to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: "thales".to_string(),
                to: to.to_string(),
                subject: "test".to_string(),
                content: "hello".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: MessageIntent::Request,
                expected_response: ExpectedResponse::Reply,
                require_ack: false,
                claimed_source_model: None,
                claimed_source_runtime: None,
                claimed_source_mode: None,
                verified_source_model: None,
                verified_source_runtime: None,
                verified_source_mode: None,
                source_worktree: None,
                source_session_id: None,
                provenance_level: crate::events::ProvenanceLevel::Legacy,
                provenance_issuer: None,
            }),
        }
    }

    #[tokio::test]
    async fn test_dispatcher_recipient_filter() {
        let dir = tempfile::tempdir().unwrap();
        let aleph_path = dir.path().join("aleph.jsonl");
        let aleph_str = aleph_path.to_str().unwrap().to_string();

        let state = AppState::new().await;
        let watchers = vec![WatcherConfig {
            agent: "aleph-notify".to_string(),
            role: WatcherRole::Observer,
            subjects: vec!["am.events.mingqiao".to_string()],
            filter: WatcherFilter {
                event_types: vec!["message_sent".to_string()],
                recipients: vec!["aleph".to_string(), "council".to_string(), "all".to_string()],
            },
            action: WatcherAction::FileAppend {
                path: aleph_str.clone(),
            },
        }];

        let _dispatcher = WatcherDispatcher::start(&state, &watchers, "mingqiao")
            .await
            .expect("dispatcher should start");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Message to luban — should NOT match aleph's watcher
        state.broadcast_event(make_message_event_to("luban"));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let content = tokio::fs::read_to_string(&aleph_path).await.unwrap_or_default();
        assert!(content.is_empty(), "Expected no lines for luban message, got: {}", content);

        // Message to aleph — should match
        state.broadcast_event(make_message_event_to("aleph"));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let content = tokio::fs::read_to_string(&aleph_path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "Expected 1 line for aleph message");

        // Message to council — should also match
        state.broadcast_event(make_message_event_to("council"));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let content = tokio::fs::read_to_string(&aleph_path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "Expected 2 lines after council message");

        // Message to all — should also match
        state.broadcast_event(make_message_event_to("all"));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let content = tokio::fs::read_to_string(&aleph_path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "Expected 3 lines after 'all' message");
    }
}
