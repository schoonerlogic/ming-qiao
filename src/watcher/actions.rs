//! Watcher action implementations.
//!
//! Actions define how matched events are delivered to watchers:
//! - `FileAppendAction` — appends compact JSONL lines to a file
//! - `WebhookAction` — POSTs the full EventEnvelope as JSON to a URL

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::warn;

use crate::events::{EventEnvelope, EventPayload};

/// Compact JSONL line format for file_append watchers.
///
/// Extracts the most useful fields from any EventPayload variant into
/// a flat, greppable structure.
#[derive(Debug, Serialize)]
pub struct EventLine {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_preview: Option<String>,
    /// Full message content (only present for Message events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// What the sender expects the receiver to do next (only for Message events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_response: Option<String>,
    /// Whether the sender requires receipt acknowledgment (only for Message events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_ack: Option<bool>,
    pub event_id: String,
}

/// Truncate a string to at most `max_len` bytes, respecting UTF-8 boundaries.
fn truncate_utf8(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    // Find the last char boundary at or before max_len
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

impl EventLine {
    /// Extract an EventLine from an EventEnvelope.
    pub fn from_envelope(event: &EventEnvelope) -> Self {
        let event_type = event.event_type.to_string();
        let event_id = event.id.to_string();

        match &event.payload {
            EventPayload::Message(m) => {
                let er_str = serde_json::to_value(&m.expected_response)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| "none".to_string());
                EventLine {
                    timestamp: event.timestamp,
                    event_type,
                    thread_id: m.thread_id.clone(),
                    from: Some(m.from.clone()),
                    to: Some(m.to.clone()),
                    subject: Some(m.subject.clone()),
                    intent: Some(serde_json::to_value(&m.intent)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| "inform".to_string())),
                    content_preview: Some(truncate_utf8(&m.content, 200)),
                    content: Some(m.content.clone()),
                    expected_response: Some(er_str),
                    require_ack: if m.require_ack { Some(true) } else { None },
                    event_id,
                }
            },
            EventPayload::Decision(d) => EventLine {
                timestamp: event.timestamp,
                event_type,
                thread_id: None,
                from: None,
                to: None,
                subject: Some(d.title.clone()),
                intent: None,
                content_preview: Some(truncate_utf8(&d.rationale, 200)),
                content: None,
                expected_response: None,
                require_ack: None,
                event_id,
            },
            EventPayload::Artifact(a) => EventLine {
                timestamp: event.timestamp,
                event_type,
                thread_id: None,
                from: None,
                to: None,
                subject: Some(a.path.clone()),
                intent: None,
                content_preview: Some(truncate_utf8(&a.description, 200)),
                content: None,
                expected_response: None,
                require_ack: None,
                event_id,
            },
            EventPayload::Task(t) => EventLine {
                timestamp: event.timestamp,
                event_type,
                thread_id: None,
                from: Some(t.assigned_by.clone()),
                to: Some(t.assigned_to.clone()),
                subject: Some(t.title.clone()),
                intent: None,
                content_preview: None,
                content: None,
                expected_response: None,
                require_ack: None,
                event_id,
            },
            EventPayload::Status(s) => EventLine {
                timestamp: event.timestamp,
                event_type,
                thread_id: None,
                from: Some(s.agent_id.clone()),
                to: None,
                subject: None,
                intent: None,
                content_preview: s.reason.as_ref().map(|r| truncate_utf8(r, 200)),
                content: None,
                expected_response: None,
                require_ack: None,
                event_id,
            },
        }
    }
}

/// File-append action: writes compact JSONL lines to a file.
///
/// The file is opened once on construction (creating parent dirs as needed).
/// Writes are serialized through a Mutex for thread safety.
pub struct FileAppendAction {
    writer: Arc<Mutex<tokio::io::BufWriter<tokio::fs::File>>>,
}

impl FileAppendAction {
    /// Open (or create) the target file in append mode.
    ///
    /// Creates parent directories if they don't exist.
    pub async fn open(path: &str) -> std::io::Result<Self> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        Ok(Self {
            writer: Arc::new(Mutex::new(tokio::io::BufWriter::new(file))),
        })
    }

    /// Write an event as a JSONL line and flush.
    pub async fn write_event(&self, event: &EventEnvelope) -> std::io::Result<()> {
        let line = EventLine::from_envelope(event);
        let mut json = serde_json::to_vec(&line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        json.push(b'\n');

        let mut w = self.writer.lock().await;
        w.write_all(&json).await?;
        w.flush().await?;
        Ok(())
    }
}

/// Webhook action: POSTs the full EventEnvelope as JSON to a URL.
///
/// Fire-and-forget: errors are logged but never block the event pipeline.
pub struct WebhookAction {
    client: reqwest::Client,
    url: String,
    agent_name: String,
}

impl WebhookAction {
    /// Create a webhook action with a 5-second timeout.
    pub fn new(url: String, agent_name: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to build reqwest client");
        Self {
            client,
            url,
            agent_name,
        }
    }

    /// POST the event envelope as JSON. Logs errors, never panics.
    pub async fn send_event(&self, event: &EventEnvelope) {
        let result = self
            .client
            .post(&self.url)
            .header("X-MingQiao-Agent", &self.agent_name)
            .json(event)
            .send()
            .await;

        match result {
            Ok(resp) if !resp.status().is_success() => {
                warn!(
                    "Webhook {} returned status {} for watcher {}",
                    self.url,
                    resp.status(),
                    self.agent_name
                );
            }
            Err(e) => {
                warn!(
                    "Webhook {} failed for watcher {}: {}",
                    self.url, self.agent_name, e
                );
            }
            _ => {}
        }
    }
}

/// System notification action: sends macOS desktop notifications via osascript.
///
/// Fire-and-forget: errors are logged but never block the event pipeline.
/// Extracts sender and subject from message events for the notification body.
pub struct SystemNotifyAction {
    title: String,
}

impl SystemNotifyAction {
    pub fn new(title: String) -> Self {
        Self { title }
    }

    /// Send a macOS notification for an event.
    pub async fn notify(&self, event: &EventEnvelope) {
        let (body, subtitle) = match &event.payload {
            EventPayload::Message(m) => {
                let body = format!("From {}: {}", m.from, m.subject);
                let subtitle = match m.intent {
                    crate::events::MessageIntent::Request => "Action needed",
                    crate::events::MessageIntent::Discuss => "Discussion",
                    crate::events::MessageIntent::Inform => "FYI",
                };
                (body, subtitle)
            }
            EventPayload::Decision(d) => {
                (format!("Decision: {}", d.title), "Decision recorded")
            }
            EventPayload::Task(t) => {
                (format!("Task: {} → {}", t.title, t.assigned_to), "Task update")
            }
            EventPayload::Artifact(a) => {
                (format!("Artifact: {}", a.path), "Artifact shared")
            }
            EventPayload::Status(s) => {
                (format!("Status: {} → {:?}", s.agent_id, s.current), "Status change")
            }
        };

        // Escape for osascript: use double quotes with backslash-escaped inner quotes
        let body_escaped = body.replace('\\', "\\\\").replace('"', "\\\"");
        let title_escaped = self.title.replace('\\', "\\\\").replace('"', "\\\"");
        let subtitle_escaped = subtitle.replace('\\', "\\\\").replace('"', "\\\"");

        let script = format!(
            "display notification \"{}\" with title \"{}\" subtitle \"{}\"",
            body_escaped, title_escaped, subtitle_escaped
        );

        match tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
        {
            Ok(output) if !output.status.success() => {
                warn!(
                    "SystemNotify osascript failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                warn!("SystemNotify failed to spawn osascript: {}", e);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_message_envelope() -> EventEnvelope {
        EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: "aleph".to_string(),
                to: "luban".to_string(),
                subject: "Task update".to_string(),
                content: "The MCP server is ready for review.".to_string(),
                thread_id: Some("thread-001".to_string()),
                priority: Priority::Normal,
                intent: MessageIntent::Inform,
                expected_response: ExpectedResponse::None,
                require_ack: false,
            }),
        }
    }

    fn make_decision_envelope() -> EventEnvelope {
        EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::DecisionRecorded,
            agent_id: "thales".to_string(),
            payload: EventPayload::Decision(DecisionEvent {
                title: "Use SurrealDB".to_string(),
                context: "Need a database for persistence".to_string(),
                options: vec![],
                chosen: 0,
                rationale: "In-memory mode for tests, WebSocket for production".to_string(),
            }),
        }
    }

    #[test]
    fn test_event_line_from_message_envelope() {
        let event = make_message_envelope();
        let line = EventLine::from_envelope(&event);

        assert_eq!(line.event_type, "message_sent");
        assert_eq!(line.from.as_deref(), Some("aleph"));
        assert_eq!(line.to.as_deref(), Some("luban"));
        assert_eq!(line.subject.as_deref(), Some("Task update"));
        assert_eq!(line.thread_id.as_deref(), Some("thread-001"));
        assert!(line
            .content_preview
            .as_ref()
            .unwrap()
            .contains("MCP server"));
        // Full content is present for message events
        assert_eq!(
            line.content.as_deref(),
            Some("The MCP server is ready for review.")
        );
        assert!(!line.event_id.is_empty());
    }

    #[test]
    fn test_event_line_message_long_content_has_truncated_preview_and_full_content() {
        let long_content = "x".repeat(500);
        let event = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: "aleph".to_string(),
                to: "luban".to_string(),
                subject: "Long message".to_string(),
                content: long_content.clone(),
                thread_id: None,
                priority: Priority::Normal,
                intent: MessageIntent::Inform,
                expected_response: ExpectedResponse::None,
                require_ack: false,
            }),
        };
        let line = EventLine::from_envelope(&event);

        // content_preview is truncated
        assert!(line.content_preview.as_ref().unwrap().len() < 500);
        assert!(line.content_preview.as_ref().unwrap().ends_with("..."));
        // content is the full 500-char string
        assert_eq!(line.content.as_deref(), Some(long_content.as_str()));
    }

    #[test]
    fn test_event_line_from_decision_envelope() {
        let event = make_decision_envelope();
        let line = EventLine::from_envelope(&event);

        assert_eq!(line.event_type, "decision_recorded");
        assert_eq!(line.subject.as_deref(), Some("Use SurrealDB"));
        assert!(line
            .content_preview
            .as_ref()
            .unwrap()
            .contains("In-memory"));
        assert!(line.from.is_none());
        assert!(line.to.is_none());
        // Non-message events have no content
        assert!(line.content.is_none());
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate_utf8("hello", 200), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let long = "a".repeat(300);
        let truncated = truncate_utf8(&long, 200);
        assert!(truncated.ends_with("..."));
        // 200 bytes of 'a' + "..."
        assert_eq!(truncated.len(), 203);
    }

    #[test]
    fn test_truncate_multibyte() {
        // Each CJK char is 3 bytes in UTF-8
        let s = "\u{4e16}\u{754c}".repeat(40); // 80 chars, 240 bytes
        let truncated = truncate_utf8(&s, 200);
        assert!(truncated.ends_with("..."));
        // Must end at a char boundary: 198 bytes = 66 CJK chars
        let without_dots = &truncated[..truncated.len() - 3];
        assert!(without_dots.len() <= 200);
        // Verify it's valid UTF-8
        assert!(std::str::from_utf8(without_dots.as_bytes()).is_ok());
    }

    #[tokio::test]
    async fn test_file_append_writes_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let path_str = path.to_str().unwrap();

        let action = FileAppendAction::open(path_str).await.unwrap();

        let event1 = make_message_envelope();
        let event2 = make_decision_envelope();

        action.write_event(&event1).await.unwrap();
        action.write_event(&event2).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        // Both should be valid JSON
        let _: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let _: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    }
}
