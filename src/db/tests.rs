// Serialization tests for database models + Indexer tests
//
// Models tests verify JSON round-trip and field names.
// Indexer tests verify event processing via process_event() directly.

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
            intent: crate::events::MessageIntent::Inform,
            expected_response: crate::events::ExpectedResponse::None,
            require_ack: false,
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
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
                created_at: Utc::now(),
                read_by: vec![],
            };

            let json = serde_json::to_string(&message).unwrap();
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
            intent: crate::events::MessageIntent::Inform,
            expected_response: crate::events::ExpectedResponse::None,
            require_ack: false,
            created_at: Utc::now(),
            read_by: vec![],
        };

        let json = serde_json::to_string(&message).unwrap();

        assert!(json.contains("thread_id"), "Should have thread_id field");
        assert!(json.contains("read_by"), "Should have read_by field");
        assert!(json.contains("created_at"), "Should have created_at field");
    }

    // ========================================================================
    // Indexer Tests — push events via process_event() directly
    // ========================================================================

    use crate::db::Indexer;
    use crate::events::{EventEnvelope, EventPayload, EventType};
    use uuid::Uuid;

    #[test]
    fn test_indexer_new_empty() {
        let indexer = Indexer::new();
        assert!(indexer.get_thread("test").is_none());
        assert!(indexer.get_messages_for_thread("test").is_empty());
        assert_eq!(indexer.events_processed(), 0);
    }

    #[test]
    fn test_indexer_process_message_event() {
        let event_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: event_id,
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: "aleph".to_string(),
                to: "thales".to_string(),
                subject: "Test Subject".to_string(),
                content: "Test message".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let thread = indexer.get_thread(&event_id.to_string());
        assert!(thread.is_some());
        let thread = thread.unwrap();
        assert_eq!(thread.id, event_id.to_string());
        assert_eq!(thread.subject, "Test Subject");
        assert_eq!(thread.status, ThreadStatus::Active);
        assert_eq!(thread.message_count, 1);
        assert!(thread.participants.contains(&"aleph".to_string()));
        assert!(thread.participants.contains(&"thales".to_string()));

        let messages = indexer.get_messages_for_thread(&event_id.to_string());
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, event_id.to_string());
        assert_eq!(messages[0].from, "aleph");

        let aleph = indexer.get_agent("aleph");
        assert!(aleph.is_some());
        assert_eq!(aleph.unwrap().status, AgentStatus::Available);
    }

    #[test]
    fn test_indexer_process_artifact_event() {
        let event_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: event_id,
            timestamp: Utc::now(),
            event_type: EventType::ArtifactShared,
            agent_id: "luban".to_string(),
            payload: EventPayload::Artifact(crate::events::ArtifactEvent {
                path: "/files/design.pdf".to_string(),
                description: "Architecture diagram".to_string(),
                checksum: "abc123".to_string(),
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let artifacts = indexer.get_artifacts();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].id, event_id.to_string());
        assert_eq!(artifacts[0].path, "/files/design.pdf");
        assert_eq!(artifacts[0].description, "Architecture diagram");
        assert_eq!(artifacts[0].shared_by, "luban");
    }

    #[test]
    fn test_indexer_process_decision_event() {
        let event_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: event_id,
            timestamp: Utc::now(),
            event_type: EventType::DecisionRecorded,
            agent_id: "thales".to_string(),
            payload: EventPayload::Decision(crate::events::DecisionEvent {
                title: "Use Rust for implementation".to_string(),
                context: "Performance and safety requirements".to_string(),
                options: vec![crate::events::DecisionOption {
                    description: "Rust".to_string(),
                    pros: vec!["Performance".to_string()],
                    cons: vec!["Learning curve".to_string()],
                }],
                chosen: 0,
                rationale: "Best choice for performance".to_string(),
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let decisions = indexer.get_decisions();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].id, event_id.to_string());
        assert_eq!(decisions[0].title, "Use Rust for implementation");
        assert_eq!(decisions[0].options.len(), 1);
        assert_eq!(decisions[0].chosen, 0);
        assert_eq!(decisions[0].status, DecisionStatus::Pending);
        assert_eq!(decisions[0].recorded_by, "thales");
    }

    #[test]
    fn test_indexer_process_multiple_events() {
        let event1_id = Uuid::now_v7();
        let event2_id = Uuid::now_v7();
        let events = vec![
            EventEnvelope {
                id: event1_id,
                timestamp: Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: "aleph".to_string(),
                payload: EventPayload::Message(crate::events::MessageEvent {
                    from: "aleph".to_string(),
                    to: "luban".to_string(),
                    subject: "Thread 1".to_string(),
                    content: "First message".to_string(),
                    thread_id: None,
                    priority: Priority::Normal,
                    intent: crate::events::MessageIntent::Inform,
                    expected_response: crate::events::ExpectedResponse::None,
                    require_ack: false,
                }),
            },
            EventEnvelope {
                id: event2_id,
                timestamp: Utc::now(),
                event_type: EventType::ArtifactShared,
                agent_id: "aleph".to_string(),
                payload: EventPayload::Artifact(crate::events::ArtifactEvent {
                    path: "/files/doc.txt".to_string(),
                    description: "A document".to_string(),
                    checksum: "xyz789".to_string(),
                }),
            },
        ];

        let mut indexer = Indexer::new();
        for event in &events {
            indexer.process_event(event).unwrap();
        }

        assert_eq!(indexer.events_processed(), 2);
        assert!(indexer.get_thread(&event1_id.to_string()).is_some());
        assert_eq!(indexer.get_artifacts().len(), 1);
    }

    #[test]
    fn test_indexer_query_messages_for_agent() {
        let msg1_id = Uuid::now_v7();
        let msg2_id = Uuid::now_v7();
        let msg3_id = Uuid::now_v7();
        let events = vec![
            EventEnvelope {
                id: msg1_id,
                timestamp: Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: "aleph".to_string(),
                payload: EventPayload::Message(crate::events::MessageEvent {
                    from: "aleph".to_string(),
                    to: "luban".to_string(),
                    subject: "Test".to_string(),
                    content: "Message from Aleph".to_string(),
                    thread_id: None,
                    priority: Priority::Normal,
                    intent: crate::events::MessageIntent::Inform,
                    expected_response: crate::events::ExpectedResponse::None,
                    require_ack: false,
                }),
            },
            EventEnvelope {
                id: msg2_id,
                timestamp: Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: "thales".to_string(),
                payload: EventPayload::Message(crate::events::MessageEvent {
                    from: "thales".to_string(),
                    to: "luban".to_string(),
                    subject: "Test".to_string(),
                    content: "Message from Thales".to_string(),
                    thread_id: None,
                    priority: Priority::Normal,
                    intent: crate::events::MessageIntent::Inform,
                    expected_response: crate::events::ExpectedResponse::None,
                    require_ack: false,
                }),
            },
            EventEnvelope {
                id: msg3_id,
                timestamp: Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: "aleph".to_string(),
                payload: EventPayload::Message(crate::events::MessageEvent {
                    from: "aleph".to_string(),
                    to: "thales".to_string(),
                    subject: "Test".to_string(),
                    content: "Another message from Aleph".to_string(),
                    thread_id: None,
                    priority: Priority::Normal,
                    intent: crate::events::MessageIntent::Inform,
                    expected_response: crate::events::ExpectedResponse::None,
                    require_ack: false,
                }),
            },
        ];

        let mut indexer = Indexer::new();
        for event in &events {
            indexer.process_event(event).unwrap();
        }

        let aleph_messages = indexer.get_messages_for_agent("aleph");
        assert_eq!(aleph_messages.len(), 2);

        let thales_messages = indexer.get_messages_for_agent("thales");
        assert_eq!(thales_messages.len(), 1);

        let luban_messages = indexer.get_messages_for_agent("luban");
        assert_eq!(luban_messages.len(), 0);
    }

    #[test]
    fn test_indexer_process_task_assigned() {
        let event_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: event_id,
            timestamp: Utc::now(),
            event_type: EventType::TaskAssigned,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Task(crate::events::TaskEvent {
                task_id: "task-001".to_string(),
                title: "Build database indexer".to_string(),
                assigned_to: "luban".to_string(),
                assigned_by: "aleph".to_string(),
                status: crate::events::TaskStatus::Assigned,
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let agent = indexer.get_agent("luban");
        assert!(agent.is_some());
        assert_eq!(
            agent.unwrap().current_task,
            Some("Build database indexer".to_string())
        );
    }

    #[test]
    fn test_indexer_process_status_changed() {
        let event_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: event_id,
            timestamp: Utc::now(),
            event_type: EventType::StatusChanged,
            agent_id: "luban".to_string(),
            payload: EventPayload::Status(crate::events::StatusEvent {
                agent_id: "luban".to_string(),
                previous: AgentStatus::Available,
                current: AgentStatus::Working,
                reason: Some("Working on indexer".to_string()),
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let agent = indexer.get_agent("luban");
        assert!(agent.is_some());
        assert_eq!(agent.unwrap().status, AgentStatus::Working);
    }

    #[test]
    fn test_indexer_get_all_threads() {
        let msg1_id = Uuid::now_v7();
        let msg2_id = Uuid::now_v7();
        let events = vec![
            EventEnvelope {
                id: msg1_id,
                timestamp: Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: "aleph".to_string(),
                payload: EventPayload::Message(crate::events::MessageEvent {
                    from: "aleph".to_string(),
                    to: "luban".to_string(),
                    subject: "Thread 1".to_string(),
                    content: "Message 1".to_string(),
                    thread_id: None,
                    priority: Priority::Normal,
                    intent: crate::events::MessageIntent::Inform,
                    expected_response: crate::events::ExpectedResponse::None,
                    require_ack: false,
                }),
            },
            EventEnvelope {
                id: msg2_id,
                timestamp: Utc::now(),
                event_type: EventType::MessageSent,
                agent_id: "thales".to_string(),
                payload: EventPayload::Message(crate::events::MessageEvent {
                    from: "thales".to_string(),
                    to: "aleph".to_string(),
                    subject: "Thread 2".to_string(),
                    content: "Message 2".to_string(),
                    thread_id: None,
                    priority: Priority::Normal,
                    intent: crate::events::MessageIntent::Inform,
                    expected_response: crate::events::ExpectedResponse::None,
                    require_ack: false,
                }),
            },
        ];

        let mut indexer = Indexer::new();
        for event in &events {
            indexer.process_event(event).unwrap();
        }

        let all_threads = indexer.get_all_threads();
        assert_eq!(all_threads.len(), 2);
    }

    #[test]
    fn test_indexer_get_message() {
        let msg_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: msg_id,
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: "aleph".to_string(),
                to: "luban".to_string(),
                subject: "Test".to_string(),
                content: "Hello".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let message = indexer.get_message(&msg_id.to_string());
        assert!(message.is_some());
        assert_eq!(message.unwrap().content, "Hello");

        let missing = indexer.get_message("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_indexer_get_decision() {
        let dec_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: dec_id,
            timestamp: Utc::now(),
            event_type: EventType::DecisionRecorded,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Decision(crate::events::DecisionEvent {
                title: "Use Rust".to_string(),
                context: "Language choice".to_string(),
                options: vec![],
                chosen: 0,
                rationale: "Best choice for performance".to_string(),
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let decision = indexer.get_decision(&dec_id.to_string());
        assert!(decision.is_some());
        assert_eq!(decision.unwrap().title, "Use Rust");

        let missing = indexer.get_decision("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_indexer_get_artifact() {
        let art_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: art_id,
            timestamp: Utc::now(),
            event_type: EventType::ArtifactShared,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Artifact(crate::events::ArtifactEvent {
                path: "/doc.txt".to_string(),
                description: "A document".to_string(),
                checksum: "abc123".to_string(),
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();

        let artifact = indexer.get_artifact(&art_id.to_string());
        assert!(artifact.is_some());
        assert_eq!(artifact.unwrap().path, "/doc.txt");

        let missing = indexer.get_artifact("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_indexer_dedup() {
        let event_id = Uuid::now_v7();
        let event = EventEnvelope {
            id: event_id,
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "aleph".to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: "aleph".to_string(),
                to: "thales".to_string(),
                subject: "Dedup Test".to_string(),
                content: "Should only count once".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
            }),
        };

        let mut indexer = Indexer::new();
        indexer.process_event(&event).unwrap();
        indexer.process_event(&event).unwrap(); // duplicate

        assert_eq!(indexer.events_processed(), 1);
        let thread = indexer.get_thread(&event_id.to_string()).unwrap();
        assert_eq!(thread.message_count, 1);
    }

    #[test]
    fn test_indexer_get_all_artifacts() {
        let art1_id = Uuid::now_v7();
        let art2_id = Uuid::now_v7();
        let events = vec![
            EventEnvelope {
                id: art1_id,
                timestamp: Utc::now(),
                event_type: EventType::ArtifactShared,
                agent_id: "aleph".to_string(),
                payload: EventPayload::Artifact(crate::events::ArtifactEvent {
                    path: "/doc1.txt".to_string(),
                    description: "Document 1".to_string(),
                    checksum: "abc123".to_string(),
                }),
            },
            EventEnvelope {
                id: art2_id,
                timestamp: Utc::now(),
                event_type: EventType::ArtifactShared,
                agent_id: "luban".to_string(),
                payload: EventPayload::Artifact(crate::events::ArtifactEvent {
                    path: "/doc2.txt".to_string(),
                    description: "Document 2".to_string(),
                    checksum: "def456".to_string(),
                }),
            },
        ];

        let mut indexer = Indexer::new();
        for event in &events {
            indexer.process_event(event).unwrap();
        }

        let all_artifacts = indexer.get_all_artifacts();
        assert_eq!(all_artifacts.len(), 2);
    }

    #[test]
    fn test_indexer_get_messages_to_agent_includes_council_broadcast() {
        let mut indexer = Indexer::new();

        // Direct message to aleph
        let e1 = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "thales".to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: "thales".to_string(),
                to: "aleph".to_string(),
                subject: "Direct".to_string(),
                content: "For you".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
            }),
        };

        // Broadcast to "all"
        let e2 = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "luban".to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: "luban".to_string(),
                to: "all".to_string(),
                subject: "All broadcast".to_string(),
                content: "Everyone".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
            }),
        };

        // Broadcast to "council"
        let e3 = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "laozi-jung".to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: "laozi-jung".to_string(),
                to: "council".to_string(),
                subject: "Council broadcast".to_string(),
                content: "Observation".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
            }),
        };

        // Message to luban (should not appear for aleph)
        let e4 = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "thales".to_string(),
            payload: EventPayload::Message(crate::events::MessageEvent {
                from: "thales".to_string(),
                to: "luban".to_string(),
                subject: "Not for aleph".to_string(),
                content: "Private".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: crate::events::MessageIntent::Inform,
                expected_response: crate::events::ExpectedResponse::None,
                require_ack: false,
            }),
        };

        for event in &[&e1, &e2, &e3, &e4] {
            indexer.process_event(event).unwrap();
        }

        let aleph_inbox = indexer.get_messages_to_agent("aleph");
        assert_eq!(aleph_inbox.len(), 3, "aleph should see direct + all + council");

        let subjects: Vec<&str> = aleph_inbox.iter().map(|m| m.subject.as_str()).collect();
        assert!(subjects.contains(&"Direct"));
        assert!(subjects.contains(&"All broadcast"));
        assert!(subjects.contains(&"Council broadcast"));
    }
}
