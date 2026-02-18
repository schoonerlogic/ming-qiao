//! Core NatsBridge type for connecting agents via NATS JetStream
//!
//! The bridge publishes local events to `am.agent.{agent_id}.task.mingqiao.events`
//! and subscribes to `am.agent.*.task.mingqiao.events` to receive events from
//! other agents. Echo suppression filters by subject prefix matching own agent_id.

use async_nats::jetstream;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::events::{EventEnvelope, EventWriter};
use crate::state::NatsConfig;

/// NATS messaging bridge for inter-agent communication
pub struct NatsBridge {
    /// JetStream context for publish and durable subscriptions
    jetstream: jetstream::Context,
    /// Resolved publish subject (e.g., `am.agent.aleph.task.mingqiao.events`)
    publish_subject: String,
    /// Subscribe subject pattern (e.g., `am.agent.*.task.mingqiao.events`)
    subscribe_subject: String,
    /// This agent's ID
    agent_id: String,
    /// Background subscription task handle
    subscription_handle: Option<JoinHandle<()>>,
}

impl NatsBridge {
    /// Connect to NATS and ensure the JetStream stream exists.
    ///
    /// Returns `None` if NATS is disabled in config or the server is unreachable.
    /// This makes NATS integration fully optional — callers just get `None` and
    /// continue without it.
    pub async fn connect(config: &NatsConfig, agent_id: &str) -> Option<Self> {
        if !config.enabled {
            info!("NATS integration disabled");
            return None;
        }

        info!("Connecting to NATS at {}", config.url);

        let client = match async_nats::connect(&config.url).await {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to connect to NATS at {}: {}. Running local-only.", config.url, e);
                return None;
            }
        };

        let jetstream = jetstream::new(client);

        // Resolve the publish subject by replacing {agent_id} placeholder
        let publish_subject = config
            .publish_subject
            .replace("{agent_id}", agent_id);
        let subscribe_subject = config.subscribe_subject.clone();

        // Ensure the JetStream stream exists
        let stream_config = jetstream::stream::Config {
            name: config.stream_name.clone(),
            subjects: vec!["am.agent.*.task.mingqiao.>".to_string()],
            max_age: std::time::Duration::from_secs(7 * 24 * 3600), // 7 days
            storage: jetstream::stream::StorageType::File,
            ..Default::default()
        };

        match jetstream.get_or_create_stream(stream_config).await {
            Ok(stream) => {
                info!(
                    "JetStream stream '{}' ready (state: {:?})",
                    config.stream_name,
                    stream.cached_info().state.messages
                );
            }
            Err(e) => {
                warn!("Failed to create/get JetStream stream: {}. Running local-only.", e);
                return None;
            }
        }

        info!(
            "NATS bridge connected for agent '{}' (publish: {}, subscribe: {})",
            agent_id, publish_subject, subscribe_subject
        );

        Some(Self {
            jetstream,
            publish_subject,
            subscribe_subject,
            agent_id: agent_id.to_string(),
            subscription_handle: None,
        })
    }

    /// Publish an event to this agent's NATS subject via JetStream.
    pub async fn publish(&self, event: &EventEnvelope) -> Result<(), NatsBridgeError> {
        let json = serde_json::to_vec(event)?;

        self.jetstream
            .publish(self.publish_subject.clone(), json.into())
            .await
            .map_err(|e| NatsBridgeError::Publish(e.to_string()))?
            .await
            .map_err(|e| NatsBridgeError::Publish(e.to_string()))?;

        Ok(())
    }

    /// Start a background subscription that receives events from other agents.
    ///
    /// Received events are:
    /// 1. Written to the local JSONL event log (so the log stays complete)
    /// 2. Broadcast to local WebSocket clients (for real-time UI updates)
    ///
    /// Echo suppression: messages published on this agent's own subject are skipped.
    pub async fn start_subscription(
        &mut self,
        event_tx: broadcast::Sender<EventEnvelope>,
        event_writer: Option<Arc<EventWriter>>,
    ) -> Result<(), NatsBridgeError> {
        let agent_id = self.agent_id.clone();
        let own_subject_prefix = format!("am.agent.{}.", agent_id);

        // Get the stream and create a durable pull consumer
        let stream = self
            .jetstream
            .get_stream("AM_MINGQIAO")
            .await
            .map_err(|e| NatsBridgeError::Subscribe(e.to_string()))?;

        let consumer_name = format!("mingqiao-{}", agent_id);
        let consumer = stream
            .get_or_create_consumer(
                &consumer_name,
                jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.clone()),
                    filter_subject: self.subscribe_subject.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| NatsBridgeError::Subscribe(e.to_string()))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| NatsBridgeError::Subscribe(e.to_string()))?;

        info!(
            "NATS subscription started (consumer: {}, filter: {})",
            consumer_name, self.subscribe_subject
        );

        let handle = tokio::spawn(async move {
            use futures_util::StreamExt;

            let mut messages = messages;
            while let Some(msg_result) = messages.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("NATS message receive error: {}", e);
                        continue;
                    }
                };

                // Echo suppression: skip messages from our own subject
                if msg.subject.starts_with(&own_subject_prefix) {
                    if let Err(e) = msg.ack().await {
                        warn!("Failed to ack own message: {}", e);
                    }
                    continue;
                }

                // Deserialize the event
                let event: EventEnvelope = match serde_json::from_slice(&msg.payload) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!("Failed to deserialize NATS event: {}", e);
                        if let Err(e) = msg.ack().await {
                            warn!("Failed to ack bad message: {}", e);
                        }
                        continue;
                    }
                };

                info!(
                    "Received remote event from '{}': {} ({})",
                    event.agent_id, event.event_type, event.id
                );

                // Write to local JSONL log
                if let Some(ref writer) = event_writer {
                    if let Err(e) = writer.append(&event) {
                        error!("Failed to write remote event to local log: {}", e);
                    }
                }

                // Broadcast to local WebSocket clients
                let _ = event_tx.send(event);

                // Acknowledge the message
                if let Err(e) = msg.ack().await {
                    warn!("Failed to ack NATS message: {}", e);
                }
            }

            info!("NATS subscription stream ended");
        });

        self.subscription_handle = Some(handle);
        Ok(())
    }

    /// Shut down the subscription background task.
    pub fn shutdown(&mut self) {
        if let Some(handle) = self.subscription_handle.take() {
            handle.abort();
            info!("NATS subscription shut down");
        }
    }
}

impl Drop for NatsBridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Errors from NATS bridge operations
#[derive(Debug, thiserror::Error)]
pub enum NatsBridgeError {
    #[error("NATS publish failed: {0}")]
    Publish(String),

    #[error("NATS subscribe failed: {0}")]
    Subscribe(String),

    #[error("Serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Display impl for EventType (used in logging)
impl std::fmt::Display for crate::events::EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MessageSent => write!(f, "message_sent"),
            Self::MessageReceived => write!(f, "message_received"),
            Self::ArtifactShared => write!(f, "artifact_shared"),
            Self::DecisionRecorded => write!(f, "decision_recorded"),
            Self::TaskAssigned => write!(f, "task_assigned"),
            Self::TaskCompleted => write!(f, "task_completed"),
            Self::StatusChanged => write!(f, "status_changed"),
        }
    }
}
