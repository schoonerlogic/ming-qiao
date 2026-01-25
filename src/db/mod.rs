// Database module for Ming-Qiao
//
// This module contains materialized view models for SurrealDB,
// derived from the append-only event log.

pub mod models;
pub mod tests;

// Re-export all models for convenient use
pub use models::{
    Agent, Annotation, AnnotationTarget, Artifact, Decision, DecisionStatus, Message,
    Thread, ThreadStatus,
};
