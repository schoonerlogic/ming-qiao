//! JetStream stream and consumer configuration
//!
//! Defines the stream topology for agent coordination:
//!
//! | Stream | Subjects | Delivery | Retention | Purpose |
//! |--------|----------|----------|-----------|---------|
//! | `AGENT_TASKS` | `am.agent.*.task.>` | Work queue | 7 days | Task coordination |
//! | `AGENT_NOTES` | `am.agent.*.notes.>` | Standard | 30 days | Session notes |
//! | `AGENT_OBSERVATIONS` | `am.observe.>` | Standard | 30 days | Observer insights |
//!
//! Presence heartbeats use core NATS (no JetStream) — they are ephemeral
//! and don't need persistence or replay.

use async_nats::jetstream;
use std::time::Duration;

// ============================================================================
// Stream names (constants for referencing from consumers)
// ============================================================================

/// Stream name for task coordination messages.
pub const STREAM_AGENT_TASKS: &str = "AGENT_TASKS";

/// Stream name for session notes.
pub const STREAM_AGENT_NOTES: &str = "AGENT_NOTES";

/// Stream name for observer insights (Laozi-Jung, future observers).
pub const STREAM_AGENT_OBSERVATIONS: &str = "AGENT_OBSERVATIONS";

/// Stream name for durable agent message delivery (Phase 2).
pub const STREAM_AGENT_MESSAGES: &str = "AGENT_MESSAGES";

// ============================================================================
// Stream configurations
// ============================================================================

/// Configuration for the AGENT_TASKS JetStream stream.
///
/// Captures all task coordination subjects (`am.agent.*.task.>`).
/// Uses work queue retention so each task message is consumed exactly once
/// by the target agent — prevents duplicate delivery of assignments.
///
/// - Retention: work queue (messages removed after ack)
/// - Max age: 7 days (unacked messages expire)
/// - Storage: file (survives server restart)
pub fn agent_tasks_stream() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: STREAM_AGENT_TASKS.to_string(),
        subjects: vec!["am.agent.*.task.>".to_string()],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        max_age: Duration::from_secs(7 * 24 * 3600), // 7 days
        storage: jetstream::stream::StorageType::File,
        ..Default::default()
    }
}

/// Configuration for the AGENT_NOTES JetStream stream.
///
/// Captures all session note subjects (`am.agent.*.notes.>`).
/// Uses standard retention with 30-day max age — notes accumulate for
/// review by Laozi-Jung and other observers, then age out.
///
/// - Retention: limits (standard — messages persist until max_age)
/// - Max age: 30 days
/// - Storage: file
pub fn agent_notes_stream() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: STREAM_AGENT_NOTES.to_string(),
        subjects: vec!["am.agent.*.notes.>".to_string()],
        retention: jetstream::stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(30 * 24 * 3600), // 30 days
        storage: jetstream::stream::StorageType::File,
        ..Default::default()
    }
}

/// Configuration for the AGENT_OBSERVATIONS JetStream stream.
///
/// Captures all observation subjects (`am.observe.>`).
/// Observations are curated synthesis from observer agents (e.g. Laozi-Jung)
/// — too valuable to lose if a subscriber is down.
///
/// - Retention: limits (standard — messages persist until max_age)
/// - Max age: 30 days
/// - Storage: file
pub fn agent_observations_stream() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: STREAM_AGENT_OBSERVATIONS.to_string(),
        subjects: vec!["am.observe.>".to_string()],
        retention: jetstream::stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(30 * 24 * 3600), // 30 days
        storage: jetstream::stream::StorageType::File,
        ..Default::default()
    }
}

/// Configuration for the AGENT_MESSAGES JetStream stream.
///
/// Captures all agent message subjects (`am.agent.*.msg.*`).
/// Used for durable delivery of inter-agent messages. The HTTP server's
/// consumer ingests these into SurrealDB + Indexer, ensuring no messages
/// are silently dropped even if the HTTP server is temporarily down.
///
/// Subject pattern: `am.agent.{to_agent}.msg.{event_type}` — no project
/// dimension (per Thales directive: simplify to avoid combinatorial subjects).
///
/// - Retention: limits (messages persist until max_age)
/// - Max age: 7 days
/// - Storage: file (survives server restart)
/// - Duplicate window: 120s (dedup by Nats-Msg-Id header)
pub fn agent_messages_stream() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: STREAM_AGENT_MESSAGES.to_string(),
        subjects: vec!["am.agent.*.msg.>".to_string()],
        retention: jetstream::stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(7 * 24 * 3600), // 7 days
        storage: jetstream::stream::StorageType::File,
        duplicate_window: Duration::from_secs(120), // 2 min dedup
        ..Default::default()
    }
}

// ============================================================================
// Consumer configurations
// ============================================================================

/// Create a durable pull consumer config for an agent's task assignments.
///
/// This consumer filters to `am.agent.{agent}.task.{project}.>` so the agent
/// only receives tasks on their own subject. Work queue delivery ensures
/// each assignment is consumed exactly once.
///
/// Consumer name: `tasks-{agent}-{project}` (e.g., `tasks-luban-mingqiao`)
pub fn task_consumer_config(agent: &str, project: &str) -> (String, jetstream::consumer::pull::Config) {
    let consumer_name = format!("tasks-{}-{}", agent, project);
    let filter = format!("am.agent.{}.task.{}.>", agent, project);

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(consumer_name.clone()),
        filter_subject: filter,
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ..Default::default()
    };

    (consumer_name, config)
}

/// Create a durable pull consumer config for observing all task activity on a project.
///
/// This consumer uses the wildcard `am.agent.*.task.{project}.>` to receive
/// task events from all agents. Used by agents that want full visibility
/// into the project (both Aleph and Luban subscribe to this).
///
/// Consumer name: `tasks-observer-{agent}-{project}`
pub fn task_observer_consumer_config(agent: &str, project: &str) -> (String, jetstream::consumer::pull::Config) {
    let consumer_name = format!("tasks-observer-{}-{}", agent, project);
    let filter = format!("am.agent.*.task.{}.>", project);

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(consumer_name.clone()),
        filter_subject: filter,
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ..Default::default()
    };

    (consumer_name, config)
}

/// Create a durable pull consumer config for session notes on a project.
///
/// Subscribes to `am.agent.*.notes.{project}` to receive notes from all agents.
/// Used by Laozi-Jung and any agent that wants to review session summaries.
///
/// Consumer name: `notes-{agent}-{project}`
pub fn notes_consumer_config(agent: &str, project: &str) -> (String, jetstream::consumer::pull::Config) {
    let consumer_name = format!("notes-{}-{}", agent, project);
    let filter = format!("am.agent.*.notes.{}", project);

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(consumer_name.clone()),
        filter_subject: filter,
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ..Default::default()
    };

    (consumer_name, config)
}

/// Create a durable pull consumer for all session notes across all projects.
///
/// Subscribes to `am.agent.*.notes.>`. Used by Laozi-Jung to observe
/// session notes from all agents on all projects.
///
/// Consumer name: `notes-all-{agent}`
pub fn notes_all_consumer_config(agent: &str) -> (String, jetstream::consumer::pull::Config) {
    let consumer_name = format!("notes-all-{}", agent);

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(consumer_name.clone()),
        filter_subject: "am.agent.*.notes.>".to_string(),
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ..Default::default()
    };

    (consumer_name, config)
}

/// Create a durable pull consumer for all observations.
///
/// Subscribes to `am.observe.>`. Used by agents that want to receive
/// all observation types (scans, insights, drift alerts, onboarding briefs).
///
/// Consumer name: `observations-all-{agent}`
pub fn observations_all_consumer_config(agent: &str) -> (String, jetstream::consumer::pull::Config) {
    let consumer_name = format!("observations-all-{}", agent);

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(consumer_name.clone()),
        filter_subject: "am.observe.>".to_string(),
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ..Default::default()
    };

    (consumer_name, config)
}

/// Create a durable pull consumer config for the HTTP server's message ingester.
///
/// Subscribes to `am.agent.*.msg.>` to receive all agent messages from JetStream.
/// The HTTP server is the sole writer to SurrealDB for message events — this
/// consumer ingests Tier 2 (JetStream fallback) messages into the database.
///
/// Consumer name: `messages-ingester-{instance}`
pub fn messages_ingester_consumer_config(instance: &str) -> (String, jetstream::consumer::pull::Config) {
    let consumer_name = format!("messages-ingester-{}", instance);

    let config = jetstream::consumer::pull::Config {
        durable_name: Some(consumer_name.clone()),
        filter_subject: "am.agent.*.msg.>".to_string(),
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ..Default::default()
    };

    (consumer_name, config)
}

// ============================================================================
// Helper for ensuring all streams exist
// ============================================================================

/// Ensure all JetStream streams exist, creating them if necessary.
///
/// Returns `Ok(())` if all streams are ready, or the first error encountered.
/// Call this during agent startup after connecting to NATS.
pub async fn ensure_streams(js: &jetstream::Context) -> Result<(), StreamSetupError> {
    let tasks = js
        .get_or_create_stream(agent_tasks_stream())
        .await
        .map_err(|e| StreamSetupError::Create {
            stream: STREAM_AGENT_TASKS.to_string(),
            reason: e.to_string(),
        })?;

    tracing::info!(
        "Stream '{}' ready ({} messages)",
        STREAM_AGENT_TASKS,
        tasks.cached_info().state.messages
    );

    let notes = js
        .get_or_create_stream(agent_notes_stream())
        .await
        .map_err(|e| StreamSetupError::Create {
            stream: STREAM_AGENT_NOTES.to_string(),
            reason: e.to_string(),
        })?;

    tracing::info!(
        "Stream '{}' ready ({} messages)",
        STREAM_AGENT_NOTES,
        notes.cached_info().state.messages
    );

    let observations = js
        .get_or_create_stream(agent_observations_stream())
        .await
        .map_err(|e| StreamSetupError::Create {
            stream: STREAM_AGENT_OBSERVATIONS.to_string(),
            reason: e.to_string(),
        })?;

    tracing::info!(
        "Stream '{}' ready ({} messages)",
        STREAM_AGENT_OBSERVATIONS,
        observations.cached_info().state.messages
    );

    let messages = js
        .get_or_create_stream(agent_messages_stream())
        .await
        .map_err(|e| StreamSetupError::Create {
            stream: STREAM_AGENT_MESSAGES.to_string(),
            reason: e.to_string(),
        })?;

    tracing::info!(
        "Stream '{}' ready ({} messages)",
        STREAM_AGENT_MESSAGES,
        messages.cached_info().state.messages
    );

    Ok(())
}

/// Errors from stream setup operations
#[derive(Debug, thiserror::Error)]
pub enum StreamSetupError {
    #[error("Failed to create stream '{stream}': {reason}")]
    Create { stream: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Stream configs
    // ========================================================================

    #[test]
    fn test_agent_tasks_stream_config() {
        let config = agent_tasks_stream();
        assert_eq!(config.name, "AGENT_TASKS");
        assert_eq!(config.subjects, vec!["am.agent.*.task.>"]);
        assert_eq!(
            config.retention,
            jetstream::stream::RetentionPolicy::WorkQueue
        );
        assert_eq!(config.max_age, Duration::from_secs(7 * 24 * 3600));
        assert_eq!(config.storage, jetstream::stream::StorageType::File);
    }

    #[test]
    fn test_agent_notes_stream_config() {
        let config = agent_notes_stream();
        assert_eq!(config.name, "AGENT_NOTES");
        assert_eq!(config.subjects, vec!["am.agent.*.notes.>"]);
        assert_eq!(
            config.retention,
            jetstream::stream::RetentionPolicy::Limits
        );
        assert_eq!(config.max_age, Duration::from_secs(30 * 24 * 3600));
        assert_eq!(config.storage, jetstream::stream::StorageType::File);
    }

    #[test]
    fn test_agent_observations_stream_config() {
        let config = agent_observations_stream();
        assert_eq!(config.name, "AGENT_OBSERVATIONS");
        assert_eq!(config.subjects, vec!["am.observe.>"]);
        assert_eq!(
            config.retention,
            jetstream::stream::RetentionPolicy::Limits
        );
        assert_eq!(config.max_age, Duration::from_secs(30 * 24 * 3600));
        assert_eq!(config.storage, jetstream::stream::StorageType::File);
    }

    #[test]
    fn test_agent_messages_stream_config() {
        let config = agent_messages_stream();
        assert_eq!(config.name, "AGENT_MESSAGES");
        assert_eq!(config.subjects, vec!["am.agent.*.msg.>"]);
        assert_eq!(
            config.retention,
            jetstream::stream::RetentionPolicy::Limits
        );
        assert_eq!(config.max_age, Duration::from_secs(7 * 24 * 3600));
        assert_eq!(config.storage, jetstream::stream::StorageType::File);
        assert_eq!(config.duplicate_window, Duration::from_secs(120));
    }

    #[test]
    fn test_messages_ingester_consumer_config() {
        let (name, config) = messages_ingester_consumer_config("main");
        assert_eq!(name, "messages-ingester-main");
        assert_eq!(config.durable_name.as_deref(), Some("messages-ingester-main"));
        assert_eq!(config.filter_subject, "am.agent.*.msg.>");
        assert_eq!(
            config.ack_policy,
            jetstream::consumer::AckPolicy::Explicit
        );
    }

    #[test]
    fn test_streams_use_am_prefix() {
        let tasks = agent_tasks_stream();
        let notes = agent_notes_stream();
        let observations = agent_observations_stream();
        let messages = agent_messages_stream();

        for subject in tasks
            .subjects
            .iter()
            .chain(notes.subjects.iter())
            .chain(observations.subjects.iter())
            .chain(messages.subjects.iter())
        {
            assert!(
                subject.starts_with("am."),
                "Stream subject '{}' does not use am. prefix",
                subject
            );
        }
    }

    // ========================================================================
    // Consumer configs — task
    // ========================================================================

    #[test]
    fn test_task_consumer_config() {
        let (name, config) = task_consumer_config("luban", "mingqiao");
        assert_eq!(name, "tasks-luban-mingqiao");
        assert_eq!(config.durable_name.as_deref(), Some("tasks-luban-mingqiao"));
        assert_eq!(
            config.filter_subject,
            "am.agent.luban.task.mingqiao.>"
        );
        assert_eq!(
            config.ack_policy,
            jetstream::consumer::AckPolicy::Explicit
        );
    }

    #[test]
    fn test_task_observer_consumer_config() {
        let (name, config) = task_observer_consumer_config("aleph", "mingqiao");
        assert_eq!(name, "tasks-observer-aleph-mingqiao");
        assert_eq!(
            config.filter_subject,
            "am.agent.*.task.mingqiao.>"
        );
    }

    #[test]
    fn test_task_consumers_for_different_agents() {
        let (luban_name, luban_config) = task_consumer_config("luban", "mingqiao");
        let (aleph_name, aleph_config) = task_consumer_config("aleph", "mingqiao");

        // Different agents get different consumer names
        assert_ne!(luban_name, aleph_name);

        // Different filter subjects
        assert_ne!(luban_config.filter_subject, aleph_config.filter_subject);
        assert!(luban_config.filter_subject.contains("luban"));
        assert!(aleph_config.filter_subject.contains("aleph"));
    }

    // ========================================================================
    // Consumer configs — notes
    // ========================================================================

    #[test]
    fn test_notes_consumer_config() {
        let (name, config) = notes_consumer_config("aleph", "mingqiao");
        assert_eq!(name, "notes-aleph-mingqiao");
        assert_eq!(
            config.filter_subject,
            "am.agent.*.notes.mingqiao"
        );
    }

    #[test]
    fn test_notes_all_consumer_config() {
        let (name, config) = notes_all_consumer_config("laozi-jung");
        assert_eq!(name, "notes-all-laozi-jung");
        assert_eq!(config.filter_subject, "am.agent.*.notes.>");
    }

    // ========================================================================
    // Consumer naming consistency
    // ========================================================================

    #[test]
    fn test_observations_all_consumer_config() {
        let (name, config) = observations_all_consumer_config("aleph");
        assert_eq!(name, "observations-all-aleph");
        assert_eq!(config.filter_subject, "am.observe.>");
        assert_eq!(
            config.ack_policy,
            jetstream::consumer::AckPolicy::Explicit
        );
    }

    #[test]
    fn test_consumer_names_are_nats_safe() {
        // NATS consumer names must be alphanumeric + hyphens + underscores
        let names = vec![
            task_consumer_config("luban", "mingqiao").0,
            task_observer_consumer_config("aleph", "mingqiao").0,
            notes_consumer_config("thales", "mingqiao").0,
            notes_all_consumer_config("laozi-jung").0,
            observations_all_consumer_config("laozi-jung").0,
        ];

        for name in &names {
            assert!(
                name.chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
                "Consumer name '{}' contains invalid characters",
                name
            );
        }
    }

    #[test]
    fn test_all_consumers_use_explicit_ack() {
        let configs: Vec<jetstream::consumer::pull::Config> = vec![
            task_consumer_config("a", "p").1,
            task_observer_consumer_config("a", "p").1,
            notes_consumer_config("a", "p").1,
            notes_all_consumer_config("a").1,
            observations_all_consumer_config("a").1,
            messages_ingester_consumer_config("main").1,
        ];

        for config in &configs {
            assert_eq!(
                config.ack_policy,
                jetstream::consumer::AckPolicy::Explicit,
                "All consumers must use explicit ack for reliable delivery"
            );
        }
    }
}
