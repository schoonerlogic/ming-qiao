//! Tests for event schemas
//!
//! Comprehensive tests ensuring all event types:
//! - Serialize to and from JSON correctly
//! - Use snake_case field names in JSON
//! - Serialize enums as human-readable strings

use super::*;
use chrono::Utc;
use serde_json;
use uuid::Uuid;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_envelope_serialization_round_trip() {
        // Arrange
        let original = EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: "luban".to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: "luban".to_string(),
                to: "aleph".to_string(),
                subject: "Task Complete".to_string(),
                content: "Event schema foundation is complete.".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: MessageIntent::default(),
            }),
        };

        // Act - Serialize to JSON
        let json = serde_json::to_string_pretty(&original)
            .expect("Failed to serialize EventEnvelope");

        println!("Serialized JSON:\n{}", json);

        // Assert - Check JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("Failed to parse JSON");
        
        // Verify snake_case fields
        assert!(parsed["event_type"].is_string());
        assert_eq!(parsed["event_type"], "message_sent");
        assert!(parsed["agent_id"].is_string());
        assert_eq!(parsed["agent_id"], "luban");
        assert!(parsed["payload"].is_object());
        
        // Verify nested payload structure - with tag-based serialization
        // payload will be: {"type": "message", "data": {...}}
        let payload_obj = parsed["payload"].as_object().expect("payload should be object");
        assert!(payload_obj.contains_key("type"));
        assert!(payload_obj.contains_key("data"));
        
        let data = &payload_obj["data"];
        assert_eq!(data["from"], "luban");
        assert_eq!(data["to"], "aleph");
        assert_eq!(data["subject"], "Task Complete");

        // Act - Deserialize back
        let deserialized: EventEnvelope = serde_json::from_str(&json)
            .expect("Failed to deserialize EventEnvelope");

        // Assert - Round trip preserves data
        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.agent_id, deserialized.agent_id);
        assert_eq!(original.event_type, deserialized.event_type);
    }

    #[test]
    fn test_message_event_serialization() {
        // Arrange
        let event = MessageEvent {
            from: "aleph".to_string(),
            to: "thales".to_string(),
            subject: "Architecture Question".to_string(),
            content: "Please review the MCP protocol design.".to_string(),
            thread_id: Some(Uuid::now_v7().to_string()),
            priority: Priority::High,
            intent: MessageIntent::Request,
        };

        // Act
        let json = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: MessageEvent = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        // Assert
        assert_eq!(event.from, deserialized.from);
        assert_eq!(event.to, deserialized.to);
        assert_eq!(event.subject, deserialized.subject);
        assert_eq!(event.content, deserialized.content);
        assert_eq!(event.thread_id, deserialized.thread_id);
        assert_eq!(event.priority, deserialized.priority);

        // Verify JSON uses snake_case
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["from"].is_string());
        assert!(parsed["to"].is_string());
        assert!(parsed["subject"].is_string());
        assert!(parsed["content"].is_string());
        assert!(parsed["thread_id"].is_string());
        assert_eq!(parsed["priority"], "high");
    }

    #[test]
    fn test_artifact_event_serialization() {
        // Arrange
        let event = ArtifactEvent {
            path: "/ming-qiao/docs/ARCHITECTURE.md".to_string(),
            description: "System architecture documentation".to_string(),
            checksum: "a1b2c3d4e5f6".to_string(),
        };

        // Act
        let json = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: ArtifactEvent = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        // Assert
        assert_eq!(event.path, deserialized.path);
        assert_eq!(event.description, deserialized.description);
        assert_eq!(event.checksum, deserialized.checksum);
    }

    #[test]
    fn test_decision_event_serialization() {
        // Arrange
        let event = DecisionEvent {
            title: "Database Choice".to_string(),
            context: "Need persistent storage for events".to_string(),
            options: vec![
                DecisionOption {
                    description: "SurrealDB".to_string(),
                    pros: vec!["Multi-model".to_string(), "Modern".to_string()],
                    cons: vec!["Less mature".to_string()],
                },
                DecisionOption {
                    description: "SQLite".to_string(),
                    pros: vec!["Battle-tested".to_string(), "Embedded".to_string()],
                    cons: vec!["Single-model".to_string()],
                },
            ],
            chosen: 0,
            rationale: "Multi-model flexibility outweighs maturity concerns".to_string(),
        };

        // Act
        let json = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: DecisionEvent = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        // Assert
        assert_eq!(event.title, deserialized.title);
        assert_eq!(event.context, deserialized.context);
        assert_eq!(event.options.len(), deserialized.options.len());
        assert_eq!(event.chosen, deserialized.chosen);
        assert_eq!(event.rationale, deserialized.rationale);
        assert_eq!(event.options[0].description, deserialized.options[0].description);
        assert_eq!(event.options[0].pros, deserialized.options[0].pros);
    }

    #[test]
    fn test_task_event_serialization() {
        // Arrange
        let event = TaskEvent {
            task_id: Uuid::now_v7().to_string(),
            title: "Implement Event Schema".to_string(),
            assigned_to: "luban".to_string(),
            assigned_by: "aleph".to_string(),
            status: TaskStatus::InProgress,
        };

        // Act
        let json = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: TaskEvent = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        // Assert
        assert_eq!(event.task_id, deserialized.task_id);
        assert_eq!(event.title, deserialized.title);
        assert_eq!(event.assigned_to, deserialized.assigned_to);
        assert_eq!(event.assigned_by, deserialized.assigned_by);
        assert_eq!(event.status, deserialized.status);

        // Verify TaskStatus serializes correctly
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "in_progress");
    }

    #[test]
    fn test_status_event_serialization() {
        // Arrange
        let event = StatusEvent {
            agent_id: "luban".to_string(),
            previous: AgentStatus::Available,
            current: AgentStatus::Working,
            reason: Some("Working on event schema".to_string()),
        };

        // Act
        let json = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: StatusEvent = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        // Assert
        assert_eq!(event.agent_id, deserialized.agent_id);
        assert_eq!(event.previous, deserialized.previous);
        assert_eq!(event.current, deserialized.current);
        assert_eq!(event.reason, deserialized.reason);

        // Verify AgentStatus serializes correctly
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["previous"], "available");
        assert_eq!(parsed["current"], "working");
    }

    #[test]
    fn test_event_type_enum_serialization() {
        // Test all EventType variants serialize as snake_case strings
        let cases = vec![
            (EventType::MessageSent, "message_sent"),
            (EventType::MessageReceived, "message_received"),
            (EventType::ArtifactShared, "artifact_shared"),
            (EventType::DecisionRecorded, "decision_recorded"),
            (EventType::TaskAssigned, "task_assigned"),
            (EventType::TaskCompleted, "task_completed"),
            (EventType::StatusChanged, "status_changed"),
        ];

        for (event_type, expected_string) in cases {
            let json = serde_json::to_string(&event_type).expect("Failed to serialize");
            let deserialized: EventType = serde_json::from_str(&json)
                .expect("Failed to deserialize");

            assert_eq!(event_type, deserialized);
            assert_eq!(json, format!("\"{}\"", expected_string));
        }
    }

    #[test]
    fn test_priority_enum_serialization() {
        let cases = vec![
            (Priority::Low, "low"),
            (Priority::Normal, "normal"),
            (Priority::High, "high"),
            (Priority::Critical, "critical"),
        ];

        for (priority, expected_string) in cases {
            let json = serde_json::to_string(&priority).expect("Failed to serialize");
            let deserialized: Priority = serde_json::from_str(&json)
                .expect("Failed to deserialize");

            assert_eq!(priority, deserialized);
            assert_eq!(json, format!("\"{}\"", expected_string));
        }
    }

    #[test]
    fn test_task_status_enum_serialization() {
        let cases = vec![
            (TaskStatus::Assigned, "assigned"),
            (TaskStatus::InProgress, "in_progress"),
            (TaskStatus::Blocked, "blocked"),
            (TaskStatus::Ready, "ready"),
            (TaskStatus::Completed, "completed"),
            (TaskStatus::Cancelled, "cancelled"),
        ];

        for (status, expected_string) in cases {
            let json = serde_json::to_string(&status).expect("Failed to serialize");
            let deserialized: TaskStatus = serde_json::from_str(&json)
                .expect("Failed to deserialize");

            assert_eq!(status, deserialized);
            assert_eq!(json, format!("\"{}\"", expected_string));
        }
    }

    #[test]
    fn test_agent_status_enum_serialization() {
        let cases = vec![
            (AgentStatus::Available, "available"),
            (AgentStatus::Working, "working"),
            (AgentStatus::Blocked, "blocked"),
            (AgentStatus::Offline, "offline"),
        ];

        for (status, expected_string) in cases {
            let json = serde_json::to_string(&status).expect("Failed to serialize");
            let deserialized: AgentStatus = serde_json::from_str(&json)
                .expect("Failed to deserialize");

            assert_eq!(status, deserialized);
            assert_eq!(json, format!("\"{}\"", expected_string));
        }
    }

    #[test]
    fn test_default_priority() {
        let priority = Priority::default();
        assert_eq!(priority, Priority::Normal);
    }

    #[test]
    fn test_default_task_status() {
        let status = TaskStatus::default();
        assert_eq!(status, TaskStatus::Assigned);
    }

    #[test]
    fn test_message_intent_enum_serialization() {
        let cases = vec![
            (MessageIntent::Discuss, "discuss"),
            (MessageIntent::Request, "request"),
            (MessageIntent::Inform, "inform"),
        ];

        for (intent, expected_string) in cases {
            let json = serde_json::to_string(&intent).expect("Failed to serialize");
            let deserialized: MessageIntent = serde_json::from_str(&json)
                .expect("Failed to deserialize");

            assert_eq!(intent, deserialized);
            assert_eq!(json, format!("\"{}\"", expected_string));
        }
    }

    #[test]
    fn test_default_message_intent() {
        let intent = MessageIntent::default();
        assert_eq!(intent, MessageIntent::Inform);
    }

    #[test]
    fn test_message_intent_missing_field_defaults_to_inform() {
        // Simulate a legacy event without the intent field
        let json = r#"{
            "from": "aleph",
            "to": "luban",
            "subject": "Test",
            "content": "Content",
            "thread_id": null,
            "priority": "normal"
        }"#;

        let msg: MessageEvent = serde_json::from_str(json)
            .expect("Failed to deserialize MessageEvent without intent");
        assert_eq!(msg.intent, MessageIntent::Inform);
    }

    #[test]
    fn test_event_payload_message_variant() {
        // Test EventPayload::Message round-trip
        let payload = EventPayload::Message(MessageEvent {
            from: "aleph".to_string(),
            to: "luban".to_string(),
            subject: "Test".to_string(),
            content: "Content".to_string(),
            thread_id: None,
            priority: Priority::Normal,
            intent: MessageIntent::default(),
        });

        let json = serde_json::to_string(&payload).expect("Failed to serialize");
        let deserialized: EventPayload = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        match deserialized {
            EventPayload::Message(msg) => {
                assert_eq!(msg.from, "aleph");
                assert_eq!(msg.to, "luban");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_event_payload_decision_variant() {
        // Test EventPayload::Decision round-trip
        let payload = EventPayload::Decision(DecisionEvent {
            title: "Test Decision".to_string(),
            context: "Test context".to_string(),
            options: vec![],
            chosen: 0,
            rationale: "Test rationale".to_string(),
        });

        let json = serde_json::to_string(&payload).expect("Failed to serialize");
        let deserialized: EventPayload = serde_json::from_str(&json)
            .expect("Failed to deserialize");

        match deserialized {
            EventPayload::Decision(dec) => {
                assert_eq!(dec.title, "Test Decision");
                assert_eq!(dec.rationale, "Test rationale");
            }
            _ => panic!("Wrong variant"),
        }
    }
}
