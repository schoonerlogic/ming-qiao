// src/db/indexer.rs
// Database Indexer - Materializes Events into Queryable In-Memory Views

use crate::db::error::IndexerError;
use crate::db::models::{Agent, Artifact, Decision, Message, Thread};
use crate::events::{EventEnvelope, EventPayload};
use std::collections::{HashMap, HashSet};

/// Materializes events into in-memory queryable views.
///
/// Events are pushed via `process_event()` (called by the write path
/// after `Persistence::store_event()`). The Indexer is a pure in-memory
/// engine — it does not know about storage.
pub struct Indexer {
    /// In-memory materialized views
    threads: HashMap<String, Thread>,
    messages: HashMap<String, Message>,
    decisions: HashMap<String, Decision>,
    artifacts: HashMap<String, Artifact>,
    agents: HashMap<String, Agent>,

    /// Count of events processed (for diagnostics)
    events_processed: u64,

    /// Event IDs already processed (dedup guard for NATS echo + hydration overlap)
    seen_ids: HashSet<String>,
}

impl Indexer {
    /// Create a new empty indexer.
    pub fn new() -> Self {
        Self {
            threads: HashMap::new(),
            messages: HashMap::new(),
            decisions: HashMap::new(),
            artifacts: HashMap::new(),
            agents: HashMap::new(),
            events_processed: 0,
            seen_ids: HashSet::new(),
        }
    }

    /// Number of events processed so far.
    pub fn events_processed(&self) -> u64 {
        self.events_processed
    }

    /// Process a single event and update materialized views.
    ///
    /// Called by the write path after storing the event in SurrealDB.
    pub fn process_event(&mut self, event: &EventEnvelope) -> Result<(), IndexerError> {
        let event_id = event.id.to_string();

        // Dedup: skip events already processed (NATS echo, hydration overlap)
        if !self.seen_ids.insert(event_id.clone()) {
            return Ok(());
        }

        self.events_processed += 1;

        match &event.payload {
            EventPayload::Message(msg) => {
                self.process_message(&event_id, event, msg)?;
            }
            EventPayload::Artifact(artifact) => {
                self.process_artifact(&event_id, event, artifact)?;
            }
            EventPayload::Decision(decision) => {
                self.process_decision(&event_id, event, decision)?;
            }
            EventPayload::Task(task) => {
                if event.agent_id == task.assigned_by {
                    self.process_task_assigned(event, task)?;
                } else {
                    self.process_task_completed(event, task)?;
                }
            }
            EventPayload::Status(status) => {
                self.process_status_changed(event, status)?;
            }
        }

        Ok(())
    }

    // --- Event processors ---

    fn process_message(
        &mut self,
        event_id: &str,
        event: &EventEnvelope,
        msg: &crate::events::MessageEvent,
    ) -> Result<(), IndexerError> {
        let thread_id = msg
            .thread_id
            .as_ref()
            .unwrap_or(&event_id.to_string())
            .to_string();

        let thread = self.threads.entry(thread_id.clone()).or_insert_with(|| Thread {
            id: thread_id.clone(),
            subject: msg.subject.clone(),
            status: crate::db::models::ThreadStatus::Active,
            participants: vec![msg.from.clone()],
            created_at: event.timestamp,
            updated_at: event.timestamp,
            message_count: 0,
        });

        if !thread.participants.contains(&msg.from) {
            thread.participants.push(msg.from.clone());
        }
        if !thread.participants.contains(&msg.to) {
            thread.participants.push(msg.to.clone());
        }

        thread.updated_at = event.timestamp;
        thread.message_count += 1;

        let message = Message {
            id: event_id.to_string(),
            thread_id: thread_id.clone(),
            from: msg.from.clone(),
            to: msg.to.clone(),
            subject: msg.subject.clone(),
            content: msg.content.clone(),
            priority: msg.priority.clone(),
            intent: msg.intent.clone(),
            expected_response: msg.expected_response.clone(),
            require_ack: msg.require_ack,
            created_at: event.timestamp,
            read_by: vec![],
            claimed_source_model: msg.claimed_source_model.clone(),
            claimed_source_runtime: msg.claimed_source_runtime.clone(),
            claimed_source_mode: msg.claimed_source_mode.clone(),
            verified_source_model: msg.verified_source_model.clone(),
            verified_source_runtime: msg.verified_source_runtime.clone(),
            verified_source_mode: msg.verified_source_mode.clone(),
            source_worktree: msg.source_worktree.clone(),
            source_session_id: msg.source_session_id.clone(),
            provenance_level: msg.provenance_level.clone(),
            provenance_issuer: msg.provenance_issuer.clone(),
        };
        self.messages.insert(event_id.to_string(), message);

        self.ensure_agent_exists(&msg.from, event.timestamp)?;
        self.ensure_agent_exists(&msg.to, event.timestamp)?;

        Ok(())
    }

    fn process_artifact(
        &mut self,
        event_id: &str,
        event: &EventEnvelope,
        artifact: &crate::events::ArtifactEvent,
    ) -> Result<(), IndexerError> {
        let art = Artifact {
            id: event_id.to_string(),
            path: artifact.path.clone(),
            description: artifact.description.clone(),
            checksum: artifact.checksum.clone(),
            shared_by: event.agent_id.clone(),
            shared_at: event.timestamp,
            thread_id: None,
            source_url: artifact.source_url.clone(),
            fetch_timestamp: artifact.fetch_timestamp,
            content_hash_sha256: artifact.content_hash_sha256.clone(),
            processor_version: artifact.processor_version.clone(),
        };
        self.artifacts.insert(event_id.to_string(), art);
        Ok(())
    }

    fn process_decision(
        &mut self,
        event_id: &str,
        event: &EventEnvelope,
        decision: &crate::events::DecisionEvent,
    ) -> Result<(), IndexerError> {
        let dec = Decision {
            id: event_id.to_string(),
            thread_id: None,
            title: decision.title.clone(),
            context: decision.context.clone(),
            options: decision.options.clone(),
            chosen: decision.chosen,
            rationale: decision.rationale.clone(),
            status: crate::db::models::DecisionStatus::Pending,
            created_at: event.timestamp,
            recorded_by: event.agent_id.clone(),
        };
        self.decisions.insert(event_id.to_string(), dec);
        Ok(())
    }

    fn process_task_assigned(
        &mut self,
        event: &EventEnvelope,
        task: &crate::events::TaskEvent,
    ) -> Result<(), IndexerError> {
        let agent = self.get_or_create_agent(&task.assigned_to, event.timestamp)?;
        agent.current_task = Some(task.title.clone());
        agent.last_seen = event.timestamp;
        Ok(())
    }

    fn process_task_completed(
        &mut self,
        event: &EventEnvelope,
        task: &crate::events::TaskEvent,
    ) -> Result<(), IndexerError> {
        let agent = self.get_or_create_agent(&task.assigned_to, event.timestamp)?;
        agent.current_task = None;
        agent.last_seen = event.timestamp;
        Ok(())
    }

    fn process_status_changed(
        &mut self,
        event: &EventEnvelope,
        status: &crate::events::StatusEvent,
    ) -> Result<(), IndexerError> {
        let agent = self.get_or_create_agent(&status.agent_id, event.timestamp)?;
        agent.status = status.current.clone();
        agent.last_seen = event.timestamp;
        Ok(())
    }

    fn ensure_agent_exists(
        &mut self,
        agent_id: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), IndexerError> {
        if !self.agents.contains_key(agent_id) {
            self.agents.insert(
                agent_id.to_string(),
                Agent {
                    id: agent_id.to_string(),
                    display_name: agent_id.to_string(),
                    status: crate::events::AgentStatus::Available,
                    current_task: None,
                    last_seen: timestamp,
                },
            );
        }
        Ok(())
    }

    fn get_or_create_agent(
        &mut self,
        agent_id: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<&mut Agent, IndexerError> {
        if !self.agents.contains_key(agent_id) {
            self.agents.insert(
                agent_id.to_string(),
                Agent {
                    id: agent_id.to_string(),
                    display_name: agent_id.to_string(),
                    status: crate::events::AgentStatus::Available,
                    current_task: None,
                    last_seen: timestamp,
                },
            );
        }
        Ok(self.agents.get_mut(agent_id).expect("just inserted"))
    }

    // --- Query Methods ---

    pub fn get_thread(&self, id: &str) -> Option<&Thread> {
        self.threads.get(id)
    }

    pub fn get_messages_for_thread(&self, thread_id: &str) -> Vec<&Message> {
        self.messages
            .values()
            .filter(|m| &m.thread_id == thread_id)
            .collect()
    }

    pub fn get_messages_for_agent(&self, agent_id: &str) -> Vec<&Message> {
        self.messages
            .values()
            .filter(|m| &m.from == agent_id)
            .collect()
    }

    pub fn get_messages_to_agent(&self, agent_id: &str) -> Vec<&Message> {
        self.messages
            .values()
            .filter(|m| m.to == agent_id || m.to == "all" || m.to == "council")
            .collect()
    }

    /// Get all unique agent IDs that have messages addressed to them.
    pub fn all_recipient_agents(&self) -> Vec<String> {
        let mut agents: HashSet<String> = HashSet::new();
        for msg in self.messages.values() {
            if msg.to != "all" && msg.to != "council" {
                agents.insert(msg.to.clone());
            }
        }
        agents.into_iter().collect()
    }

    pub fn get_decisions(&self) -> Vec<&Decision> {
        self.decisions.values().collect()
    }

    pub fn get_artifacts(&self) -> Vec<&Artifact> {
        self.artifacts.values().collect()
    }

    pub fn get_agent(&self, id: &str) -> Option<&Agent> {
        self.agents.get(id)
    }

    pub fn get_all_threads(&self) -> Vec<&Thread> {
        self.threads.values().collect()
    }

    pub fn get_message(&self, id: &str) -> Option<&Message> {
        self.messages.get(id)
    }

    pub fn get_decision(&self, id: &str) -> Option<&Decision> {
        self.decisions.get(id)
    }

    pub fn get_artifact(&self, id: &str) -> Option<&Artifact> {
        self.artifacts.get(id)
    }

    pub fn get_all_artifacts(&self) -> Vec<&Artifact> {
        self.artifacts.values().collect()
    }
}

impl Default for Indexer {
    fn default() -> Self {
        Self::new()
    }
}
