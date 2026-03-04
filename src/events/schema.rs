//! Event schema definitions for ming-qiao
//!
//! This module defines the core event types that form the foundation of
//! ming-qiao's append-only event log. All events share a common envelope
//! structure and are serializable for persistence and transmission.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Supporting Enums - Must be declared first (used by structs below)
// ============================================================================

/// Priority levels for messages and tasks
///
/// Indicates urgency and importance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Low priority - can be deferred
    Low,
    
    /// Normal priority - standard processing
    Normal,
    
    /// High priority - expedite handling
    High,
    
    /// Critical priority - immediate attention required
    Critical,
}

/// Status of a task in its lifecycle
///
/// Tracks progression from assignment to completion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task has been assigned but not yet started
    Assigned,
    
    /// Task is currently being worked on
    #[serde(rename = "in_progress")]
    InProgress,
    
    /// Task cannot proceed due to dependencies or blockers
    Blocked,
    
    /// Task is complete and ready for review
    Ready,
    
    /// Task has been completed successfully
    Completed,
    
    /// Task was cancelled before completion
    Cancelled,
}

/// Intent behind a message — signals how the recipient should treat it
///
/// Used by the hint system to categorize inbox messages by urgency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageIntent {
    /// Team discussion — respond when ready
    Discuss,

    /// Needs a response — action required
    Request,

    /// FYI / status update — no response needed
    Inform,
}

impl Default for MessageIntent {
    fn default() -> Self {
        MessageIntent::Inform
    }
}

/// What the sender expects the receiver to do next.
///
/// Maps to military radio prowords: reply=OVER, ack=ROGER, comply=WILCO,
/// none=OUT, standby=STANDBY. Carried as structured metadata so the system
/// can prioritize notifications and detect stalled conversations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedResponse {
    /// I need your input — send a response (OVER)
    Reply,

    /// Confirm you received this — no action needed (ROGER)
    Ack,

    /// Do what this says — confirm when done (WILCO)
    Comply,

    /// FYI only — no response expected (OUT)
    None,

    /// I'm working on it — hold for my follow-up (STANDBY)
    Standby,
}

impl Default for ExpectedResponse {
    fn default() -> Self {
        ExpectedResponse::None
    }
}

fn is_default_expected_response(er: &ExpectedResponse) -> bool {
    *er == ExpectedResponse::None
}

fn is_false(b: &bool) -> bool {
    !b
}

/// Availability and working state of an agent
///
/// Indicates whether an agent can accept new tasks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agent is available and can accept new tasks
    Available,
    
    /// Agent is actively working on assigned tasks
    Working,
    
    /// Agent is blocked and waiting for something
    Blocked,
    
    /// Agent is offline or not responding
    Offline,
}

// ============================================================================
// Event Types
// ============================================================================

/// All events share this envelope structure
///
/// The envelope provides metadata that applies to all events:
/// - A UUID v7 for time-sortable unique identification
/// - A UTC timestamp for when the event occurred
/// - The agent that produced this event
/// - The event type and its specific payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// UUID v7 (time-sortable) - uniquely identifies this event
    pub id: Uuid,
    
    /// Timestamp when this event occurred
    pub timestamp: DateTime<Utc>,
    
    /// The type of event (determines which payload variant to use)
    pub event_type: EventType,
    
    /// Which agent produced this event (e.g., "aleph", "luban", "thales")
    pub agent_id: String,
    
    /// The event-specific data (variant depends on event_type)
    pub payload: EventPayload,
}

/// The type of event
///
/// Each variant corresponds to a specific payload type in `EventPayload`.
/// Serialized as strings for human readability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// A message was sent from one agent to another
    MessageSent,
    
    /// A message was received by an agent
    MessageReceived,
    
    /// An artifact (file, documentation, etc.) was shared
    ArtifactShared,
    
    /// A decision was recorded with rationale
    DecisionRecorded,
    
    /// A task was assigned to an agent
    TaskAssigned,
    
    /// A task was completed (successfully or otherwise)
    TaskCompleted,
    
    /// An agent's status changed
    StatusChanged,
}

impl std::fmt::Display for EventType {
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

/// Event payloads contain the specific data for each event type
///
/// The payload variant must match the event_type in the envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventPayload {
    /// Data for MessageSent and MessageReceived events
    Message(MessageEvent),
    
    /// Data for ArtifactShared events
    Artifact(ArtifactEvent),
    
    /// Data for DecisionRecorded events
    Decision(DecisionEvent),
    
    /// Data for TaskAssigned and TaskCompleted events
    Task(TaskEvent),
    
    /// Data for StatusChanged events
    Status(StatusEvent),
}

// ============================================================================
// Event Payload Structs
// ============================================================================

/// Data for message events
///
/// Represents a message sent from one agent to another,
/// with optional threading for conversation tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    /// The agent ID that sent this message
    pub from: String,
    
    /// The agent ID that received this message (or "all" for broadcast)
    pub to: String,
    
    /// Subject line summarizing the message content
    pub subject: String,
    
    /// The full message content (supports markdown)
    pub content: String,
    
    /// Optional thread ID for grouping related messages (UUID v7)
    /// If None, this starts a new conversation thread
    pub thread_id: Option<String>,
    
    /// Message priority level
    #[serde(default)]
    pub priority: Priority,

    /// Message intent — signals how the recipient should treat this message
    #[serde(default)]
    pub intent: MessageIntent,

    /// What the sender expects the receiver to do next
    #[serde(default, skip_serializing_if = "is_default_expected_response")]
    pub expected_response: ExpectedResponse,

    /// Whether the system should track receipt acknowledgment
    #[serde(default, skip_serializing_if = "is_false")]
    pub require_ack: bool,
}

/// Data for artifact sharing events
///
/// Represents a file, document, or other artifact being shared
/// between agents for review or collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEvent {
    /// File path or URL to the artifact
    pub path: String,
    
    /// Human-readable description of what this artifact is
    pub description: String,
    
    /// Checksum for integrity verification (e.g., SHA-256)
    pub checksum: String,
}

/// Data for decision recording events
///
/// Records architectural or implementation decisions with their
/// rationale and the options that were considered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEvent {
    /// Short title summarizing the decision
    pub title: String,
    
    /// Context and background for why this decision was needed
    pub context: String,
    
    /// All options that were considered
    pub options: Vec<DecisionOption>,
    
    /// Index into options array indicating which was chosen
    pub chosen: usize,
    
    /// Explanation for why this option was selected
    pub rationale: String,
}

/// A single option that was considered for a decision
///
/// Captures the trade-offs that were evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionOption {
    /// Brief description of this option
    pub description: String,
    
    /// Advantages of this option
    #[serde(default)]
    pub pros: Vec<String>,
    
    /// Disadvantages of this option
    #[serde(default)]
    pub cons: Vec<String>,
}

/// Data for task assignment and completion events
///
/// Tracks task lifecycle events across the council.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    /// Unique identifier for this task (UUID v7)
    pub task_id: String,
    
    /// Human-readable task title
    pub title: String,
    
    /// Agent ID that this task is assigned to
    pub assigned_to: String,
    
    /// Agent ID that assigned this task
    pub assigned_by: String,
    
    /// Current status of the task
    #[serde(default)]
    pub status: TaskStatus,
}

/// Data for agent status change events
///
/// Records when an agent changes its availability or working state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEvent {
    /// Agent ID whose status changed
    pub agent_id: String,
    
    /// Previous status before this change
    pub previous: AgentStatus,
    
    /// New current status
    pub current: AgentStatus,
    
    /// Optional explanation for the status change
    pub reason: Option<String>,
}

// ============================================================================
// Default Implementations
// ============================================================================

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Assigned
    }
}
