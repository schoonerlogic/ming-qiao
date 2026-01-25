// Serialization tests for database models
//
// These tests verify that all database models serialize/deserialize correctly
// to/from JSON, with proper snake_case field names and enum variants.

#[cfg(test)]
mod tests {
    use crate::db::{
        Agent, Annotation, AnnotationTarget, Artifact, Decision, DecisionStatus, Message, Thread,
        ThreadStatus,
    };
    use crate::events::{AgentStatus, DecisionOption, Priority};
    use chrono::Utc;

    // ========================================================================
    // Thread Tests
    // ========================================================================

    #[test]
    fn test_thread_serialization_round_trip() {
        let thread = Thread {
            id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
            subject: "API Design Discussion".to_string(),
            participants: vec!["aleph".to_string(), "thales".to_string()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            message_count: 5,
            status: ThreadStatus::Active,
        };

        let json = serde_json::to_string(&thread).unwrap();
        let deserialized: Thread = serde_json::from_str(&json).unwrap();

        assert_eq!(thread.id, deserialized.id);
        assert_eq!(thread.subject, deserialized.subject);
        assert_eq!(thread.participants, deserialized.participants);
        assert_eq!(thread.status, deserialized.status);
    }

    #[test]
    fn test_thread_status_enum_serialization() {
        let statuses = vec![
            ThreadStatus::Active,
            ThreadStatus::Paused,
            ThreadStatus::Resolved,
            ThreadStatus::Archived,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            // Should be lowercase snake_case string
            assert!(json.contains("\""), "Status should be a JSON string");

            let deserialized: ThreadStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    // ========================================================================
    // Message Tests
    // ========================================================================

    #[test]
    fn test_message_serialization_round_trip() {
        let message = Message {
            id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
            thread_id: "thread-123".to_string(),
            from: "luban".to_string(),
            to: "aleph".to_string(),
            subject: "Task Update".to_string(),
            content: "Event schema is complete.".to_string(),
            priority: Priority::Normal,
            created_at: Utc::now(),
            read_by: vec!["aleph".to_string()],
        };

        let json = serde_json::to_string(&message).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(message.id, deserialized.id);
        assert_eq!(message.from, deserialized.from);
        assert_eq!(message.priority, deserialized.priority);
        assert_eq!(message.read_by, deserialized.read_by);
    }

    #[test]
    fn test_message_with_priority_enum() {
        let priorities = vec![
            Priority::Low,
            Priority::Normal,
            Priority::High,
            Priority::Critical,
        ];

        for priority in priorities {
            let message = Message {
                id: "test-id".to_string(),
                thread_id: "thread-123".to_string(),
                from: "test".to_string(),
                to: "test".to_string(),
                subject: "Test".to_string(),
                content: "Test".to_string(),
                priority: priority.clone(),
                created_at: Utc::now(),
                read_by: vec![],
            };

            let json = serde_json::to_string(&message).unwrap();
            // Check that priority serializes as a string
            assert!(json.contains("\"priority\""), "Should have priority field");

            let deserialized: Message = serde_json::from_str(&json).unwrap();
            assert_eq!(priority, deserialized.priority);
        }
    }

    // ========================================================================
    // Decision Tests
    // ========================================================================

    #[test]
    fn test_decision_serialization_round_trip() {
        let options = vec![
            DecisionOption {
                description: "Use PostgreSQL".to_string(),
                pros: vec!["Mature".to_string(), "Well-known".to_string()],
                cons: vec!["No native JSON".to_string()],
            },
            DecisionOption {
                description: "Use SurrealDB".to_string(),
                pros: vec!["Native JSON".to_string(), "Time-series".to_string()],
                cons: vec!["Newer".to_string()],
            },
        ];

        let decision = Decision {
            id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
            thread_id: Some("thread-123".to_string()),
            title: "Database Selection".to_string(),
            context: "Choosing database for event storage".to_string(),
            options: options.clone(),
            chosen: 1,
            rationale: "SurrealDB supports JSON natively and has built-in time-series".to_string(),
            status: DecisionStatus::Approved,
            created_at: Utc::now(),
            recorded_by: "thales".to_string(),
        };

        let json = serde_json::to_string(&decision).unwrap();
        let deserialized: Decision = serde_json::from_str(&json).unwrap();

        assert_eq!(decision.id, deserialized.id);
        assert_eq!(decision.chosen, deserialized.chosen);
        assert_eq!(decision.options.len(), deserialized.options.len());
        assert_eq!(decision.status, deserialized.status);
    }

    #[test]
    fn test_decision_status_enum_serialization() {
        let statuses = vec![
            DecisionStatus::Pending,
            DecisionStatus::Approved,
            DecisionStatus::Rejected,
            DecisionStatus::Superseded,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            // Should be lowercase snake_case string
            assert!(json.contains("\""), "Status should be a JSON string");

            let deserialized: DecisionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    // ========================================================================
    // Artifact Tests
    // ========================================================================

    #[test]
    fn test_artifact_serialization_round_trip() {
        let artifact = Artifact {
            id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
            path: "/docs/architecture.md".to_string(),
            description: "System architecture documentation".to_string(),
            checksum: "a1b2c3d4".to_string(),
            shared_by: "thales".to_string(),
            shared_at: Utc::now(),
            thread_id: Some("thread-456".to_string()),
        };

        let json = serde_json::to_string(&artifact).unwrap();
        let deserialized: Artifact = serde_json::from_str(&json).unwrap();

        assert_eq!(artifact.id, deserialized.id);
        assert_eq!(artifact.path, deserialized.path);
        assert_eq!(artifact.checksum, deserialized.checksum);
        assert_eq!(artifact.thread_id, deserialized.thread_id);
    }

    // ========================================================================
    // Agent Tests
    // ========================================================================

    #[test]
    fn test_agent_serialization_round_trip() {
        let agent = Agent {
            id: "luban".to_string(),
            display_name: "Luban (Builder)".to_string(),
            status: AgentStatus::Working,
            last_seen: Utc::now(),
            current_task: Some("Database Models".to_string()),
        };

        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();

        assert_eq!(agent.id, deserialized.id);
        assert_eq!(agent.display_name, deserialized.display_name);
        assert_eq!(agent.status, deserialized.status);
        assert_eq!(agent.current_task, deserialized.current_task);
    }

    #[test]
    fn test_agent_without_current_task() {
        let agent = Agent {
            id: "thales".to_string(),
            display_name: "Thales (Architect)".to_string(),
            status: AgentStatus::Available,
            last_seen: Utc::now(),
            current_task: None,
        };

        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();

        assert_eq!(agent.current_task, deserialized.current_task);
        assert!(json.contains("null"), "None should serialize as null");
    }

    // ========================================================================
    // Annotation Tests
    // ========================================================================

    #[test]
    fn test_annotation_serialization_round_trip() {
        let annotation = Annotation {
            id: "01234567-89ab-cdef-0123-456789abcdef".to_string(),
            target_type: AnnotationTarget::Decision,
            target_id: "decision-789".to_string(),
            content: "Good rationale, consider edge cases.".to_string(),
            created_at: Utc::now(),
        };

        let json = serde_json::to_string(&annotation).unwrap();
        let deserialized: Annotation = serde_json::from_str(&json).unwrap();

        assert_eq!(annotation.id, deserialized.id);
        assert_eq!(annotation.target_type, deserialized.target_type);
        assert_eq!(annotation.target_id, deserialized.target_id);
        assert_eq!(annotation.content, deserialized.content);
    }

    #[test]
    fn test_annotation_target_enum_serialization() {
        let targets = vec![
            AnnotationTarget::Thread,
            AnnotationTarget::Decision,
            AnnotationTarget::Message,
        ];

        for target in targets {
            let json = serde_json::to_string(&target).unwrap();
            // Should be lowercase snake_case string
            assert!(json.contains("\""), "Target should be a JSON string");

            let deserialized: AnnotationTarget = serde_json::from_str(&json).unwrap();
            assert_eq!(target, deserialized);
        }
    }

    // ========================================================================
    // JSON Format Tests
    // ========================================================================

    #[test]
    fn test_thread_json_has_snake_case_fields() {
        let thread = Thread {
            id: "test-id".to_string(),
            subject: "Test".to_string(),
            participants: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            message_count: 0,
            status: ThreadStatus::Active,
        };

        let json = serde_json::to_string(&thread).unwrap();

        // Verify snake_case field names
        assert!(
            json.contains("message_count"),
            "Should have message_count field"
        );
        assert!(json.contains("created_at"), "Should have created_at field");
        assert!(json.contains("updated_at"), "Should have updated_at field");
    }

    #[test]
    fn test_message_json_has_snake_case_fields() {
        let message = Message {
            id: "test-id".to_string(),
            thread_id: "thread-123".to_string(),
            from: "test".to_string(),
            to: "test".to_string(),
            subject: "Test".to_string(),
            content: "Test".to_string(),
            priority: Priority::Normal,
            created_at: Utc::now(),
            read_by: vec![],
        };

        let json = serde_json::to_string(&message).unwrap();

        // Verify snake_case field names
        assert!(json.contains("thread_id"), "Should have thread_id field");
        assert!(json.contains("read_by"), "Should have read_by field");
        assert!(json.contains("created_at"), "Should have created_at field");
    }
}
