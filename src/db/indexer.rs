// src/db/indexer.rs
// Database Indexer - Materializes Event Log into Queryable State

use crate::db::error::IndexerError;
use crate::db::models::{Agent, Artifact, Decision, Message, Thread};
use crate::db::state::IndexerState;
use crate::events::{EventEnvelope, EventPayload, EventReader};
use std::collections::HashMap;
use std::path::Path;

/// Indexes events from the event log into materialized views.
pub struct Indexer {
    /// Event reader for the log
    reader: EventReader,

    /// Current indexer state
    state: IndexerState,

    /// In-memory materialized views
    threads: HashMap<String, Thread>,
    messages: HashMap<String, Message>,
    decisions: HashMap<String, Decision>,
    artifacts: HashMap<String, Artifact>,
    agents: HashMap<String, Agent>,
}

impl Indexer {
    /// Create a new indexer.
    ///
    /// # Arguments
    /// * `events_path` - Path to the event log (e.g., `data/events.jsonl`)
    ///
    /// # Returns
    /// * `Ok(Indexer)` - Indexer ready to process events
    /// * `Err(IndexerError)` - Failed to create event reader
    pub fn new<P: AsRef<Path>>(events_path: P) -> Result<Self, IndexerError> {
        let reader = EventReader::open(events_path)?;
        Ok(Self {
            reader,
            state: IndexerState::default(),
            threads: HashMap::new(),
            messages: HashMap::new(),
            decisions: HashMap::new(),
            artifacts: HashMap::new(),
            agents: HashMap::new(),
        })
    }

    /// Create indexer with loaded state.
    pub fn with_state<P: AsRef<Path>>(
        events_path: P,
        state: IndexerState,
    ) -> Result<Self, IndexerError> {
        let reader = EventReader::open(events_path)?;
        Ok(Self {
            reader,
            state,
            threads: HashMap::new(),
            messages: HashMap::new(),
            decisions: HashMap::new(),
            artifacts: HashMap::new(),
            agents: HashMap::new(),
        })
    }

    /// Get current indexer state.
    pub fn state(&self) -> &IndexerState {
        &self.state
    }

    /// Process all new events from the log.
    ///
    /// Starts from the last processed position and processes all new events.
    ///
    /// # Returns
    /// * `Ok(count)` - Number of events processed
    /// * `Err(IndexerError)` - Failed to read or process events
    pub fn catch_up(&mut self) -> Result<usize, IndexerError> {
        let mut count = 0;

        // Start from last position, or beginning if first run
        if let Some(last_id) = &self.state.last_event_id {
            // Resume from after last event
            for event_result in self.reader.after(last_id)? {
                let event = event_result?;
                self.process_event(&event)?;
                count += 1;
            }
        } else {
            // Process all events from beginning
            for event_result in self.reader.replay()? {
                let event = event_result?;
                self.process_event(&event)?;
                count += 1;
            }
        }

        Ok(count)
    }

    /// Process a single event and update materialized views.
    fn process_event(&mut self, event: &EventEnvelope) -> Result<(), IndexerError> {
        let event_id = event.id.to_string();

        // Skip if we've already processed this event (shouldn't happen with append-only log)
        if let Some(last_id) = &self.state.last_event_id {
            if &event_id == last_id {
                return Ok(());
            }
        }

        // Update state
        self.state.last_event_id = Some(event_id.clone());
        self.state.last_timestamp = Some(event.timestamp);
        self.state.events_processed += 1;

        // Process based on event type
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
                // Task events can be TaskAssigned or TaskCompleted
                // Check event.agent_id to determine who created the event
                if event.agent_id == task.assigned_by {
                    // Task was just assigned
                    self.process_task_assigned(event, task)?;
                } else {
                    // Task was completed
                    self.process_task_completed(event, task)?;
                }
            }
            EventPayload::Status(status) => {
                self.process_status_changed(event, status)?;
            }
        }

        Ok(())
    }

    /// Process Message event: Create/update Thread, create Message.
    fn process_message(
        &mut self,
        event_id: &str,
        event: &EventEnvelope,
        msg: &crate::events::MessageEvent,
    ) -> Result<(), IndexerError> {
        // Determine thread ID: use message.thread_id if present, otherwise use event ID
        let thread_id = msg
            .thread_id
            .as_ref()
            .unwrap_or(&event_id.to_string())
            .to_string();

        // Create or update thread
        let thread = self.threads.entry(thread_id.clone()).or_insert_with(|| Thread {
            id: thread_id.clone(),
            subject: msg.subject.clone(),
            status: crate::db::models::ThreadStatus::Active,
            participants: vec![msg.from.clone()],
            created_at: event.timestamp,
            updated_at: event.timestamp,
            message_count: 0,
        });

        // Accumulate participants (from both from and to fields)
        if !thread.participants.contains(&msg.from) {
            thread.participants.push(msg.from.clone());
        }
        if !thread.participants.contains(&msg.to) {
            thread.participants.push(msg.to.clone());
        }

        // Update thread metadata
        thread.updated_at = event.timestamp;
        thread.message_count += 1;

        // Create message
        let message = Message {
            id: event_id.to_string(),
            thread_id: thread_id.clone(),
            from: msg.from.clone(),
            to: msg.to.clone(),
            subject: msg.subject.clone(),
            content: msg.content.clone(),
            priority: msg.priority.clone(),
            created_at: event.timestamp,
            read_by: vec![],
        };
        self.messages.insert(event_id.to_string(), message);

        // Ensure agents exist for sender and recipient
        self.ensure_agent_exists(&msg.from, event.timestamp)?;
        self.ensure_agent_exists(&msg.to, event.timestamp)?;

        Ok(())
    }

    /// Process Artifact event: Create Artifact.
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
        };
        self.artifacts.insert(event_id.to_string(), art);
        Ok(())
    }

    /// Process Decision event: Create Decision.
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

    /// Process TaskAssigned event: Update Agent.current_task.
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

    /// Process TaskCompleted event: Set Agent.current_task to None.
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

    /// Process StatusChanged event: Update Agent.status.
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

    /// Ensure an agent record exists, create if not.
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
                    display_name: agent_id.to_string(), // Default to agent ID
                    status: crate::events::AgentStatus::Available,
                    current_task: None,
                    last_seen: timestamp,
                },
            );
        }
        Ok(())
    }

    /// Get or create an agent record.
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

    /// Save current state to disk.
    ///
    /// # Arguments
    /// * `state_path` - Path to the state file (e.g., `data/indexer_state.json`)
    pub fn save_state<P: AsRef<Path>>(&self, state_path: P) -> Result<(), IndexerError> {
        self.state.save(state_path)
    }

    // --- Query Methods ---

    /// Get a thread by ID.
    pub fn get_thread(&self, id: &str) -> Option<&Thread> {
        self.threads.get(id)
    }

    /// Get all messages for a thread.
    pub fn get_messages_for_thread(&self, thread_id: &str) -> Vec<&Message> {
        self.messages
            .values()
            .filter(|m| &m.thread_id == thread_id)
            .collect()
    }

    /// Get all messages sent by an agent.
    pub fn get_messages_for_agent(&self, agent_id: &str) -> Vec<&Message> {
        self.messages
            .values()
            .filter(|m| &m.from == agent_id)
            .collect()
    }

    /// Get all messages sent to an agent.
    pub fn get_messages_to_agent(&self, agent_id: &str) -> Vec<&Message> {
        self.messages
            .values()
            .filter(|m| &m.to == agent_id || m.to == "all")
            .collect()
    }

    /// Get all decisions.
    pub fn get_decisions(&self) -> Vec<&Decision> {
        self.decisions.values().collect()
    }

    /// Get all artifacts.
    pub fn get_artifacts(&self) -> Vec<&Artifact> {
        self.artifacts.values().collect()
    }

    /// Get an agent by ID.
    pub fn get_agent(&self, id: &str) -> Option<&Agent> {
        self.agents.get(id)
    }

    /// Get all threads.
    pub fn get_all_threads(&self) -> Vec<&Thread> {
        self.threads.values().collect()
    }

    /// Get a message by ID.
    pub fn get_message(&self, id: &str) -> Option<&Message> {
        self.messages.get(id)
    }

    /// Get a decision by ID.
    pub fn get_decision(&self, id: &str) -> Option<&Decision> {
        self.decisions.get(id)
    }

    /// Get an artifact by ID.
    pub fn get_artifact(&self, id: &str) -> Option<&Artifact> {
        self.artifacts.get(id)
    }

    /// Get all artifacts.
    pub fn get_all_artifacts(&self) -> Vec<&Artifact> {
        self.artifacts.values().collect()
    }
}
