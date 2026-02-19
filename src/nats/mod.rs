//! NATS messaging bridge for agent coordination
//!
//! Provides pub/sub messaging between agents running in separate processes
//! or worktrees. Complements the local JSONL event log — NATS carries events
//! in real time, while the log remains the source of truth.
//!
//! ## Module structure
//!
//! - `subjects` — Subject hierarchy builder (`am.agent.{agent}.task.{project}.*`)
//! - `bridge` — Core NatsBridge connection and pub/sub (to be refactored into `client`)

mod bridge;
pub mod messages;
pub mod subjects;

pub use bridge::NatsBridge;
pub use messages::{NatsMessage, Presence, SessionNote, TaskAssignment, TaskStatusUpdate};
pub use subjects::AgentSubjects;
