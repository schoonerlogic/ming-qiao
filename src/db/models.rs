// Database models for Ming-Qiao
//
// These models represent materialized views of the event log in SurrealDB.
// They are the queryable state derived by replaying events from the append-only log.
//
// Models are pure data containers - validation happens in the processing/indexer layer.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Re-use types from events (re-exported from schema)
use crate::events::{AgentStatus, DecisionOption, ExpectedResponse, MessageIntent, Priority, ProvenanceLevel};

// ============================================================================
// NEW ENUMS
// ============================================================================

/// The current status of a conversation thread
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// Thread is active and accepting new messages
    Active,
    /// Thread has been paused by Merlin
    Paused,
    /// Thread has been resolved (no further action needed)
    Resolved,
    /// Thread has been archived (historical reference only)
    Archived,
}

/// The approval status of a recorded decision
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    /// Awaiting Merlin's approval (gated mode)
    Pending,
    /// Decision has been approved and is active
    Approved,
    /// Decision was rejected
    Rejected,
    /// Decision has been superseded by a newer decision
    Superseded,
}

/// The type of target an annotation refers to
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationTarget {
    /// Annotation on a Thread
    Thread,
    /// Annotation on a Decision
    Decision,
    /// Annotation on a Message
    Message,
}

// ============================================================================
// DATABASE MODELS
// ============================================================================

/// A conversation thread between agents
///
/// Materialized from MessageEvents. Represents a single conversation topic
/// that may involve multiple participants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Unique identifier (UUID v7)
    pub id: String,

    /// Thread subject/title
    pub subject: String,

    /// Agent IDs participating in this thread
    pub participants: Vec<String>,

    /// When the thread was created
    pub created_at: DateTime<Utc>,

    /// When the thread was last updated (last message or status change)
    pub updated_at: DateTime<Utc>,

    /// Number of messages in this thread
    pub message_count: u32,

    /// Current thread status
    pub status: ThreadStatus,
}

/// A message within a thread
///
/// Materialized from MessageEvent. Represents a single message sent from
/// one agent to another (or to all agents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique identifier (UUID v7), same as the originating event ID
    pub id: String,

    /// ID of the thread this message belongs to
    pub thread_id: String,

    /// Agent ID who sent this message
    pub from: String,

    /// Agent ID who received this message (or "all" for broadcast)
    pub to: String,

    /// Message subject
    pub subject: String,

    /// Message content (markdown supported)
    pub content: String,

    /// Message priority level
    pub priority: Priority,

    /// Message intent (discuss, request, inform)
    pub intent: MessageIntent,

    /// What the sender expects the receiver to do next
    pub expected_response: ExpectedResponse,

    /// Whether the system should track receipt acknowledgment
    pub require_ack: bool,

    /// When this message was sent
    pub created_at: DateTime<Utc>,

    /// Agent IDs who have marked this message as read
    pub read_by: Vec<String>,

    // -- Provenance --
    pub claimed_source_model: Option<String>,
    pub claimed_source_runtime: Option<String>,
    pub claimed_source_mode: Option<String>,
    pub verified_source_model: Option<String>,
    pub verified_source_runtime: Option<String>,
    pub verified_source_mode: Option<String>,
    pub source_worktree: Option<String>,
    pub source_session_id: Option<String>,
    pub provenance_level: ProvenanceLevel,
    pub provenance_issuer: Option<String>,
}

/// A recorded decision
///
/// Materialized from DecisionEvent. Represents a decision made during
/// development, with options considered and the chosen path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// Unique identifier (UUID v7)
    pub id: String,

    /// Optional ID of the thread that spawned this decision
    pub thread_id: Option<String>,

    /// Decision title
    pub title: String,

    /// Context/background for this decision
    pub context: String,

    /// Options that were considered
    pub options: Vec<DecisionOption>,

    /// Index of the chosen option (validation happens in processing layer)
    pub chosen: usize,

    /// Rationale for the chosen option
    pub rationale: String,

    /// Current status of this decision
    pub status: DecisionStatus,

    /// When this decision was recorded
    pub created_at: DateTime<Utc>,

    /// Agent ID who recorded this decision
    pub recorded_by: String,
}

/// A shared artifact (file or document)
///
/// Materialized from ArtifactEvent. Represents a file that has been
/// shared between agents for reference or collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Unique identifier (UUID v7)
    pub id: String,

    /// File path or URL to the artifact
    pub path: String,

    /// Human-readable description of the artifact
    pub description: String,

    /// Checksum for integrity verification
    pub checksum: String,

    /// Agent ID who shared this artifact
    pub shared_by: String,

    /// When this artifact was shared
    pub shared_at: DateTime<Utc>,

    /// Optional ID of the thread providing context for this artifact
    pub thread_id: Option<String>,
}

/// Current state of an agent
///
/// Materialized from StatusEvents. Represents the latest known state
/// of an agent in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Agent identifier (e.g., "aleph", "luban", "thales")
    pub id: String,

    /// Human-readable display name
    pub display_name: String,

    /// Current agent status
    pub status: AgentStatus,

    /// When this agent was last seen (last activity)
    pub last_seen: DateTime<Utc>,

    /// Optional description of current task the agent is working on
    pub current_task: Option<String>,
}

/// Merlin's notes on threads, decisions, or messages
///
/// Annotations are meta-level comments from the human operator (Merlin/Proteus)
/// providing context, feedback, or guidance on the system's activities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    /// Unique identifier (UUID v7)
    pub id: String,

    /// Type of target this annotation refers to
    pub target_type: AnnotationTarget,

    /// ID of the target (thread_id, decision_id, or message_id)
    pub target_id: String,

    /// Annotation content
    pub content: String,

    /// When this annotation was created
    pub created_at: DateTime<Utc>,
}
