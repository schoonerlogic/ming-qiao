//! Configuration types for the watcher system.
//!
//! Watchers are observer agents that receive real-time event streams from
//! ming-qiao without polling. Each watcher subscribes to a set of NATS-style
//! subject patterns and dispatches matching events via a configured action
//! (file append or webhook).

use serde::{Deserialize, Serialize};

/// Role of a watcher agent in the system.
///
/// Determines whether the agent is a pure observer or an active participant.
/// Observer role triggers a warning (not enforcement) on write attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WatcherRole {
    /// Pure observer — receives events, should not modify state.
    #[default]
    Observer,

    /// Active participant — receives events and may also write.
    Participant,
}

/// Filter criteria for which events a watcher receives.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatcherFilter {
    /// Event types to include. Empty means all event types pass.
    #[serde(default)]
    pub event_types: Vec<String>,

    /// Only match messages addressed TO these agents.
    /// Empty means all recipients pass.
    /// Supports: specific agent ID, "council", "all"
    #[serde(default)]
    pub recipients: Vec<String>,
}

/// Action to perform when an event matches a watcher's subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WatcherAction {
    /// Append a compact JSONL line to a file.
    FileAppend {
        /// Path to the JSONL output file (parent dirs created on startup).
        path: String,
    },

    /// POST the full EventEnvelope as JSON to a URL.
    Webhook {
        /// Target URL for the HTTP POST.
        url: String,
    },

    /// macOS system notification via osascript.
    /// Sends a notification to the desktop — useful for alerting the human
    /// (Merlin/Proteus) when agents need attention.
    SystemNotify {
        /// Title for the notification banner (e.g. "Ming-Qiao Council").
        title: String,
    },
}

/// Configuration for a single watcher.
///
/// Deserialized from a `[[watchers]]` TOML array entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Agent identifier (e.g. "laozi-jung").
    pub agent: String,

    /// Role of this watcher.
    #[serde(default)]
    pub role: WatcherRole,

    /// NATS-style subject patterns to subscribe to.
    /// Uses `*` (single token) and `>` (one-or-more trailing tokens) wildcards.
    #[serde(default)]
    pub subjects: Vec<String>,

    /// Optional event type filter.
    #[serde(default)]
    pub filter: WatcherFilter,

    /// Action to perform on matching events.
    pub action: WatcherAction,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watcher_config_deserialization() {
        let toml_str = r#"
            [[watchers]]
            agent = "laozi-jung"
            role = "observer"
            subjects = ["am.events.mingqiao"]

            [watchers.filter]
            event_types = ["message_sent", "decision_recorded"]

            [watchers.action]
            type = "file_append"
            path = "/tmp/stream.jsonl"
        "#;

        #[derive(Deserialize)]
        struct Wrapper {
            watchers: Vec<WatcherConfig>,
        }

        let wrapper: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(wrapper.watchers.len(), 1);

        let w = &wrapper.watchers[0];
        assert_eq!(w.agent, "laozi-jung");
        assert_eq!(w.role, WatcherRole::Observer);
        assert_eq!(w.subjects, vec!["am.events.mingqiao"]);
        assert_eq!(
            w.filter.event_types,
            vec!["message_sent", "decision_recorded"]
        );
        match &w.action {
            WatcherAction::FileAppend { path } => {
                assert_eq!(path, "/tmp/stream.jsonl");
            }
            _ => panic!("Expected FileAppend action"),
        }
    }

    #[test]
    fn test_default_role_is_observer() {
        let toml_str = r#"
            [[watchers]]
            agent = "test"
            subjects = ["am.events.>"]

            [watchers.action]
            type = "webhook"
            url = "http://localhost:8080/events"
        "#;

        #[derive(Deserialize)]
        struct Wrapper {
            watchers: Vec<WatcherConfig>,
        }

        let wrapper: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(wrapper.watchers[0].role, WatcherRole::Observer);
    }

    #[test]
    fn test_empty_filter_defaults() {
        let toml_str = r#"
            [[watchers]]
            agent = "test"
            subjects = ["am.events.>"]

            [watchers.action]
            type = "file_append"
            path = "/tmp/test.jsonl"
        "#;

        #[derive(Deserialize)]
        struct Wrapper {
            watchers: Vec<WatcherConfig>,
        }

        let wrapper: Wrapper = toml::from_str(toml_str).unwrap();
        assert!(wrapper.watchers[0].filter.event_types.is_empty());
        assert!(wrapper.watchers[0].filter.recipients.is_empty());
    }

    #[test]
    fn test_recipient_filter_deserialization() {
        let toml_str = r#"
            [[watchers]]
            agent = "aleph-notify"
            role = "observer"
            subjects = ["am.events.mingqiao"]

            [watchers.filter]
            event_types = ["message_sent"]
            recipients = ["aleph", "council", "all"]

            [watchers.action]
            type = "file_append"
            path = "/tmp/aleph.jsonl"
        "#;

        #[derive(Deserialize)]
        struct Wrapper {
            watchers: Vec<WatcherConfig>,
        }

        let wrapper: Wrapper = toml::from_str(toml_str).unwrap();
        let w = &wrapper.watchers[0];
        assert_eq!(w.agent, "aleph-notify");
        assert_eq!(w.filter.recipients, vec!["aleph", "council", "all"]);
        assert_eq!(w.filter.event_types, vec!["message_sent"]);
    }
}
