//! NATS Agent Client for inter-agent coordination
//!
//! Replaces the original NatsBridge with a purpose-specific client that uses
//! the structured subject hierarchy, typed messages, and JetStream streams.
//!
//! Three communication channels:
//!
//! - **Presence** → core NATS (ephemeral heartbeats, no persistence)
//! - **Task coordination** → JetStream `AGENT_TASKS` stream (work queue, 7 days)
//! - **Session notes** → JetStream `AGENT_NOTES` stream (limits, 30 days)

use std::time::Duration;

use async_nats::jetstream;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::nats::messages::{MessageNotification, NatsMessage, Presence, SessionNote, TaskAssignment, TaskStatusUpdate};
use crate::nats::streams;
use crate::nats::subjects::AgentSubjects;
use crate::state::NatsConfig;

/// NATS agent client for typed inter-agent coordination.
///
/// Holds both a raw NATS client (for ephemeral presence) and a JetStream
/// context (for persistent task and notes messaging). Subject construction
/// is handled by [`AgentSubjects`].
pub struct NatsAgentClient {
    /// Raw NATS client for core NATS operations (presence)
    client: async_nats::Client,
    /// JetStream context for persistent messaging (tasks, notes)
    jetstream: jetstream::Context,
    /// Subject builder for this agent on this project
    subjects: AgentSubjects,
    /// Background task handles for subscriptions
    handles: Vec<JoinHandle<()>>,
}

impl NatsAgentClient {
    /// Connect to NATS, ensure JetStream streams exist, and create the client.
    ///
    /// Returns `None` if NATS is disabled in config or the server is unreachable.
    /// This makes NATS integration fully optional — callers get `None` and
    /// continue without it.
    ///
    /// Supports NKey authentication when configured (Security P0).
    pub async fn connect(config: &NatsConfig, agent_id: &str, project: &str) -> Option<Self> {
        use crate::state::NatsAuthMode;

        if !config.enabled {
            info!("NATS integration disabled");
            return None;
        }

        info!("Connecting to NATS at {}", config.url);

        let client = match config.auth_mode {
            NatsAuthMode::Nkey => {
                let seed = match config.resolved_nkey_seed() {
                    Some(s) => s,
                    None => {
                        warn!("NATS NKey auth configured but no seed available. Set MINGQIAO_NATS_NKEY_SEED env var or nkey_seed_file in config.");
                        return None;
                    }
                };
                match async_nats::ConnectOptions::with_nkey(seed)
                    .name(format!("mingqiao-{}", agent_id))
                    .connect(&config.url)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(
                            "Failed to connect to NATS with NKey at {}: {}. Running local-only.",
                            config.url, e
                        );
                        return None;
                    }
                }
            }
            NatsAuthMode::None => {
                match async_nats::connect(&config.url).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(
                            "Failed to connect to NATS at {}: {}. Running local-only.",
                            config.url, e
                        );
                        return None;
                    }
                }
            }
        };

        let jetstream = jetstream::new(client.clone());

        // Ensure JetStream streams exist (AGENT_TASKS + AGENT_NOTES)
        if let Err(e) = streams::ensure_streams(&jetstream).await {
            warn!(
                "Failed to set up JetStream streams: {}. Running local-only.",
                e
            );
            return None;
        }

        let subjects = AgentSubjects::new(agent_id, project);

        info!(
            "NATS agent client connected for '{}' on project '{}'",
            agent_id, project
        );

        Some(Self {
            client,
            jetstream,
            subjects,
            handles: Vec::new(),
        })
    }

    /// Get the subject builder for this client.
    pub fn subjects(&self) -> &AgentSubjects {
        &self.subjects
    }

    /// Get the raw NATS client for direct publishing (e.g. observation subjects).
    pub fn raw_client(&self) -> &async_nats::Client {
        &self.client
    }

    /// Get the JetStream context for durable publishing (e.g. observations).
    pub fn jetstream(&self) -> &jetstream::Context {
        &self.jetstream
    }

    /// Return the raw NATS client and events broadcast subject for the
    /// event sync bridge.  The client is cloned (cheap Arc bump), following
    /// the same pattern used by `start_presence_heartbeat`.
    pub fn event_sync_parts(&self) -> (async_nats::Client, String) {
        (self.client.clone(), self.subjects.events())
    }

    // ========================================================================
    // Publish methods
    // ========================================================================

    /// Publish a presence heartbeat via core NATS (ephemeral, no persistence).
    ///
    /// Subject: `am.agent.{agent}.presence`
    pub async fn publish_presence(&self, presence: &Presence) -> Result<(), ClientError> {
        let subject = self.subjects.presence();
        let payload = serde_json::to_vec(&NatsMessage::Presence(presence.clone()))?;

        self.client
            .publish(subject, payload.into())
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Publish a message event to JetStream for durable delivery.
    ///
    /// Used by the HTTP server to publish EventEnvelopes to the AGENT_MESSAGES
    /// stream after writing to SurrealDB. This enables cross-process sync via
    /// JetStream instead of ephemeral core NATS.
    ///
    /// Subject: `am.msg.{to_agent}`
    ///
    /// Returns the JetStream sequence number on success.
    pub async fn publish_message_event(
        &self,
        to_agent: &str,
        event: &crate::events::EventEnvelope,
    ) -> Result<u64, ClientError> {
        let subject = AgentSubjects::message_event(to_agent);
        let payload = serde_json::to_vec(event)?;

        // Set Nats-Msg-Id for server-side dedup (120s window)
        let mut headers = async_nats::HeaderMap::new();
        headers.insert(
            async_nats::header::NATS_MESSAGE_ID,
            event.id.to_string().as_str(),
        );

        let ack = self
            .jetstream
            .publish_with_headers(subject, headers, payload.into())
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?;

        Ok(ack.sequence)
    }

    /// Publish a message notification hint via core NATS (ephemeral, no JetStream).
    ///
    /// Published to the *recipient's* message subject — not our own.
    /// Subject: `am.agent.{to_agent}.message.{project}`
    pub async fn publish_message_notification(
        &self,
        to_agent: &str,
        notification: &MessageNotification,
    ) -> Result<(), ClientError> {
        let recipient_subjects =
            AgentSubjects::new(to_agent, self.subjects.project());
        let subject = recipient_subjects.message();

        let payload =
            serde_json::to_vec(&NatsMessage::MessageNotification(notification.clone()))?;

        self.client
            .publish(subject, payload.into())
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Publish a task assignment via JetStream.
    ///
    /// Publishes to the *assignee's* task subject — not our own.
    /// Subject: `am.agent.{assigned_to}.task.{project}.assigned`
    pub async fn publish_task_assignment(
        &self,
        assignment: &TaskAssignment,
    ) -> Result<(), ClientError> {
        let assignee_subjects =
            AgentSubjects::new(&assignment.assigned_to, self.subjects.project());
        let subject = assignee_subjects.task_assigned();

        let payload =
            serde_json::to_vec(&NatsMessage::TaskAssignment(assignment.clone()))?;

        self.jetstream
            .publish(subject, payload.into())
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Publish a task status update via JetStream.
    ///
    /// Uses [`TaskStatusUpdate::subject_suffix()`] to determine the subtopic.
    /// Subject: `am.agent.{agent}.task.{project}.{suffix}`
    pub async fn publish_task_status(
        &self,
        update: &TaskStatusUpdate,
    ) -> Result<(), ClientError> {
        let suffix = update.subject_suffix();
        let subject = format!(
            "am.agent.{}.task.{}.{}",
            self.subjects.agent(),
            self.subjects.project(),
            suffix
        );

        let payload =
            serde_json::to_vec(&NatsMessage::TaskStatusUpdate(update.clone()))?;

        self.jetstream
            .publish(subject, payload.into())
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Publish session notes via JetStream.
    ///
    /// Subject: `am.agent.{agent}.notes.{project}`
    pub async fn publish_session_note(&self, note: &SessionNote) -> Result<(), ClientError> {
        let subject = self.subjects.notes();

        let payload = serde_json::to_vec(&NatsMessage::SessionNote(note.clone()))?;

        self.jetstream
            .publish(subject, payload.into())
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?
            .await
            .map_err(|e| ClientError::Publish(e.to_string()))?;

        Ok(())
    }

    // ========================================================================
    // Subscription methods
    // ========================================================================

    /// Start a subscription for task messages assigned to this agent.
    ///
    /// Creates a durable pull consumer that receives task messages addressed
    /// to this agent on this project. Messages are forwarded to the broadcast
    /// channel as [`NatsMessage`] variants.
    pub async fn subscribe_own_tasks(
        &mut self,
        tx: broadcast::Sender<NatsMessage>,
    ) -> Result<(), ClientError> {
        let stream = self
            .jetstream
            .get_stream(streams::STREAM_AGENT_TASKS)
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let (consumer_name, config) =
            streams::task_consumer_config(self.subjects.agent(), self.subjects.project());

        let consumer = stream
            .get_or_create_consumer(&consumer_name, config)
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        info!("Subscribed to own tasks (consumer: {})", consumer_name);

        let handle = tokio::spawn(consume_jetstream(messages, tx));
        self.handles.push(handle);
        Ok(())
    }

    /// Start an observer subscription for all task activity on this project.
    ///
    /// Creates a durable pull consumer that receives task messages from ALL agents.
    /// Echo suppression: messages from this agent's own subject are skipped.
    pub async fn subscribe_all_tasks(
        &mut self,
        tx: broadcast::Sender<NatsMessage>,
    ) -> Result<(), ClientError> {
        let stream = self
            .jetstream
            .get_stream(streams::STREAM_AGENT_TASKS)
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let (consumer_name, config) = streams::task_observer_consumer_config(
            self.subjects.agent(),
            self.subjects.project(),
        );

        let consumer = stream
            .get_or_create_consumer(&consumer_name, config)
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let own_prefix = self.subjects.own_prefix();
        info!(
            "Subscribed to all tasks (consumer: {}, echo suppression prefix: {})",
            consumer_name, own_prefix
        );

        let handle = tokio::spawn(consume_jetstream_filtered(messages, tx, own_prefix));
        self.handles.push(handle);
        Ok(())
    }

    /// Start a subscription for session notes on this project.
    ///
    /// Receives notes from all agents. Echo suppression skips this agent's own notes.
    pub async fn subscribe_notes(
        &mut self,
        tx: broadcast::Sender<NatsMessage>,
    ) -> Result<(), ClientError> {
        let stream = self
            .jetstream
            .get_stream(streams::STREAM_AGENT_NOTES)
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let (consumer_name, config) =
            streams::notes_consumer_config(self.subjects.agent(), self.subjects.project());

        let consumer = stream
            .get_or_create_consumer(&consumer_name, config)
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let own_prefix = self.subjects.own_prefix();
        info!(
            "Subscribed to notes (consumer: {}, echo suppression prefix: {})",
            consumer_name, own_prefix
        );

        let handle = tokio::spawn(consume_jetstream_filtered(messages, tx, own_prefix));
        self.handles.push(handle);
        Ok(())
    }

    /// Start a subscription for presence heartbeats from all agents.
    ///
    /// Uses core NATS (not JetStream) since presence is ephemeral.
    /// Echo suppression skips this agent's own heartbeats.
    pub async fn subscribe_presence(
        &mut self,
        tx: broadcast::Sender<NatsMessage>,
    ) -> Result<(), ClientError> {
        let subject = AgentSubjects::all_agents_presence();

        let mut subscription = self
            .client
            .subscribe(subject.clone())
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        let own_prefix = self.subjects.own_prefix();
        info!(
            "Subscribed to presence (subject: {}, echo suppression prefix: {})",
            subject, own_prefix
        );

        let handle = tokio::spawn(async move {
            use futures_util::StreamExt;
            while let Some(msg) = subscription.next().await {
                // Echo suppression
                if msg.subject.as_ref().starts_with(&own_prefix) {
                    continue;
                }

                match serde_json::from_slice::<NatsMessage>(&msg.payload) {
                    Ok(nats_msg) => {
                        let _ = tx.send(nats_msg);
                    }
                    Err(e) => {
                        warn!("Failed to deserialize presence message: {}", e);
                    }
                }
            }
            info!("Presence subscription ended");
        });

        self.handles.push(handle);
        Ok(())
    }

    /// Start a subscription for message notifications addressed to this agent.
    ///
    /// Uses core NATS (not JetStream) since notifications are ephemeral hints.
    /// No echo suppression — we only subscribe to our own message subject,
    /// and only other agents publish to it.
    ///
    /// Subject: `am.agent.{self}.message.{project}`
    pub async fn subscribe_own_messages(
        &mut self,
        tx: broadcast::Sender<NatsMessage>,
    ) -> Result<(), ClientError> {
        let subject = self.subjects.message();

        let subscription = self
            .client
            .subscribe(subject.clone())
            .await
            .map_err(|e| ClientError::Subscribe(e.to_string()))?;

        info!("Subscribed to message notifications (subject: {})", subject);
        eprintln!("[ming-qiao] Subscribed to message notifications (subject: {})", subject);

        let handle = tokio::spawn(async move {
            use futures_util::StreamExt;
            let mut subscription = subscription;
            while let Some(msg) = subscription.next().await {
                match serde_json::from_slice::<NatsMessage>(&msg.payload) {
                    Ok(nats_msg) => {
                        if let NatsMessage::MessageNotification(ref notif) = nats_msg {
                            // Use eprintln for MCP visibility (tracing not init'd in mcp-serve)
                            eprintln!(
                                "[ming-qiao] NEW MESSAGE from '{}': {}",
                                notif.from, notif.subject
                            );
                            info!(
                                "New message from '{}': {}",
                                notif.from, notif.subject
                            );
                        }
                        let _ = tx.send(nats_msg);
                    }
                    Err(e) => {
                        eprintln!("[ming-qiao] Failed to deserialize message notification: {}", e);
                        warn!("Failed to deserialize message notification: {}", e);
                    }
                }
            }
            info!("Message notification subscription ended");
        });

        self.handles.push(handle);
        Ok(())
    }

    // ========================================================================
    // Presence heartbeat
    // ========================================================================

    /// Start a background task that publishes presence heartbeats at a regular interval.
    ///
    /// Heartbeats announce this agent's availability to all other agents.
    /// Published every 30 seconds via core NATS (ephemeral, no persistence).
    ///
    /// The heartbeat includes the agent name, project, current branch, and a
    /// status string. For dynamic status updates, stop the heartbeat and start
    /// a new one with the updated values.
    pub fn start_presence_heartbeat(&mut self, branch: String, status: String) {
        let client = self.client.clone();
        let subject = self.subjects.presence();
        let agent = self.subjects.agent().to_string();
        let project = self.subjects.project().to_string();

        info!(
            "Starting presence heartbeat for '{}' (every 30s on {})",
            agent, subject
        );

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;

                let presence = Presence::new(&agent, &project, &branch, &status);
                let msg = NatsMessage::Presence(presence);

                match serde_json::to_vec(&msg) {
                    Ok(payload) => {
                        if let Err(e) = client.publish(subject.clone(), payload.into()).await {
                            warn!("Presence heartbeat publish failed: {}", e);
                        } else {
                            debug!("Presence heartbeat published for '{}'", agent);
                        }
                    }
                    Err(e) => {
                        warn!("Presence heartbeat serialization failed: {}", e);
                    }
                }
            }
        });

        self.handles.push(handle);
    }

    /// Shut down all subscription background tasks and heartbeat.
    pub fn shutdown(&mut self) {
        for handle in self.handles.drain(..) {
            handle.abort();
        }
        if !self.handles.is_empty() {
            info!("NATS agent client shut down");
        }
    }
}

impl Drop for NatsAgentClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ============================================================================
// Free-standing consumer functions (avoids complex generic bounds on methods)
// ============================================================================

/// Consume JetStream messages and forward all to broadcast channel.
async fn consume_jetstream(
    messages: async_nats::jetstream::consumer::pull::Stream,
    tx: broadcast::Sender<NatsMessage>,
) {
    use futures_util::StreamExt;

    let mut messages = messages;
    while let Some(msg_result) = messages.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                warn!("JetStream message error: {}", e);
                continue;
            }
        };

        match serde_json::from_slice::<NatsMessage>(&msg.payload) {
            Ok(nats_msg) => {
                info!("Received NATS message on {}", msg.subject);
                let _ = tx.send(nats_msg);
            }
            Err(e) => {
                warn!(
                    "Failed to deserialize JetStream message on {}: {}",
                    msg.subject, e
                );
            }
        }

        if let Err(e) = msg.ack().await {
            warn!("Failed to ack JetStream message: {}", e);
        }
    }

    info!("JetStream consumer ended");
}

/// Consume JetStream messages with echo suppression by subject prefix.
async fn consume_jetstream_filtered(
    messages: async_nats::jetstream::consumer::pull::Stream,
    tx: broadcast::Sender<NatsMessage>,
    own_prefix: String,
) {
    use futures_util::StreamExt;

    let mut messages = messages;
    while let Some(msg_result) = messages.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                warn!("JetStream message error: {}", e);
                continue;
            }
        };

        // Echo suppression: skip messages from our own subject prefix
        if msg.subject.starts_with(&own_prefix) {
            if let Err(e) = msg.ack().await {
                warn!("Failed to ack own message: {}", e);
            }
            continue;
        }

        match serde_json::from_slice::<NatsMessage>(&msg.payload) {
            Ok(nats_msg) => {
                info!("Received NATS message on {}", msg.subject);
                let _ = tx.send(nats_msg);
            }
            Err(e) => {
                warn!(
                    "Failed to deserialize JetStream message on {}: {}",
                    msg.subject, e
                );
            }
        }

        if let Err(e) = msg.ack().await {
            warn!("Failed to ack JetStream message: {}", e);
        }
    }

    info!("JetStream consumer ended");
}

// ============================================================================
// Error type
// ============================================================================

/// Errors from NATS agent client operations
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("NATS publish failed: {0}")]
    Publish(String),

    #[error("NATS subscribe failed: {0}")]
    Subscribe(String),

    #[error("Serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

