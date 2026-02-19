//! NATS messaging for agent coordination
//!
//! Provides typed pub/sub messaging between agents running in separate processes
//! or worktrees. Complements the local JSONL event log — NATS carries events
//! in real time, while the log remains the source of truth.
//!
//! ## Module structure
//!
//! - `subjects` — Subject hierarchy builder (`am.agent.{agent}.task.{project}.*`)
//! - `messages` — Typed message payloads (Presence, TaskAssignment, etc.)
//! - `streams` — JetStream stream and consumer configurations
//! - `client` — NatsAgentClient: connect, publish, subscribe

mod client;
pub mod messages;
pub mod streams;
pub mod subjects;

pub use client::{ClientError, NatsAgentClient};
pub use messages::{NatsMessage, Presence, SessionNote, TaskAssignment, TaskStatusUpdate};
pub use subjects::AgentSubjects;
