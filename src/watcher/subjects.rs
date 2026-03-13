//! NATS-compatible subject construction and pattern matching for watchers.
//!
//! Events generate synthetic NATS-style subjects that watchers subscribe to
//! using wildcard patterns. This allows flexible routing without requiring
//! an actual NATS server.

use crate::events::EventEnvelope;

/// Construct synthetic NATS subjects for an event.
///
/// Every event produces at least two subjects:
/// - `am.events.{project}` — the main project event stream
/// - `am.agent.{agent_id}.events.{event_type}` — agent+type specific
pub fn subjects_for_event(event: &EventEnvelope, project: &str) -> Vec<String> {
    let event_type = event.event_type.to_string();
    vec![
        format!("am.events.{}", project),
        format!("am.agent.{}.events.{}", event.agent_id, event_type),
    ]
}

/// Check if a subject matches a NATS-compatible pattern.
///
/// Pattern rules:
/// - Exact token match: `foo` matches `foo`
/// - `*` matches exactly one token
/// - `>` matches one or more tokens (must be last token in pattern)
pub fn matches_subject(subject: &str, pattern: &str) -> bool {
    let subject_tokens: Vec<&str> = subject.split('.').collect();
    let pattern_tokens: Vec<&str> = pattern.split('.').collect();

    let mut si = 0;
    let mut pi = 0;

    while pi < pattern_tokens.len() {
        let pat = pattern_tokens[pi];

        if pat == ">" {
            // `>` must be the last token and matches one or more remaining
            return si < subject_tokens.len();
        }

        // Need a subject token to match against
        if si >= subject_tokens.len() {
            return false;
        }

        if pat == "*" || pat == subject_tokens[si] {
            si += 1;
            pi += 1;
        } else {
            return false;
        }
    }

    // Both must be fully consumed
    si == subject_tokens.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventEnvelope, EventPayload, EventType, ExpectedResponse, MessageEvent, MessageIntent, Priority};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_exact_match() {
        assert!(matches_subject("am.events.mingqiao", "am.events.mingqiao"));
    }

    #[test]
    fn test_exact_no_match() {
        assert!(!matches_subject(
            "am.events.mingqiao",
            "am.events.builder"
        ));
    }

    #[test]
    fn test_star_wildcard_single_token() {
        assert!(matches_subject("am.events.mingqiao", "am.events.*"));
        assert!(matches_subject("am.events.builder", "am.events.*"));
    }

    #[test]
    fn test_star_no_match_multiple() {
        // `*` matches exactly one token, not two
        assert!(!matches_subject(
            "am.agent.aleph.events.message_sent",
            "am.agent.*.message_sent"
        ));
    }

    #[test]
    fn test_gt_matches_one_or_more() {
        assert!(matches_subject("am.agent.aleph.events", "am.agent.>"));
        assert!(matches_subject(
            "am.agent.aleph.events.message_sent",
            "am.agent.>"
        ));
    }

    #[test]
    fn test_gt_requires_at_least_one() {
        // `>` requires at least one token after the prefix
        assert!(!matches_subject("am.agent", "am.agent.>"));
    }

    #[test]
    fn test_pattern_longer_than_subject() {
        assert!(!matches_subject("am.events", "am.events.mingqiao"));
    }

    #[test]
    fn test_subject_longer_than_pattern() {
        assert!(!matches_subject(
            "am.events.mingqiao.extra",
            "am.events.mingqiao"
        ));
    }

    #[test]
    fn test_subjects_for_message_event() {
        let event = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: "aleph".to_string(),
                to: "luban".to_string(),
                subject: "test".to_string(),
                content: "hello".to_string(),
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
        };

        let subjects = subjects_for_event(&event, "mingqiao");
        assert_eq!(subjects.len(), 2);
        assert_eq!(subjects[0], "am.events.mingqiao");
        assert_eq!(subjects[1], "am.agent.aleph.events.message_sent");
    }
}
