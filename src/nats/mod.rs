//! NATS messaging bridge for agent coordination
//!
//! Provides pub/sub messaging between agents running in separate processes
//! or worktrees. Complements the local JSONL event log — NATS carries events
//! in real time, while the log remains the source of truth.

mod bridge;

pub use bridge::NatsBridge;
