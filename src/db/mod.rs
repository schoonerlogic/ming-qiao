// Database module for Ming-Qiao
//
// This module contains the SurrealDB persistence layer and
// in-memory materialized view models.

pub mod error;
pub mod indexer;
pub mod models;
pub mod persistence;
pub mod tests;

// Re-export all models for convenient use
pub use models::{
    Agent, Annotation, AnnotationTarget, Artifact, Decision, DecisionStatus, Message,
    Thread, ThreadStatus,
};

// Re-export indexer types
pub use error::{IndexerError, PersistenceError};
pub use indexer::Indexer;
pub use persistence::Persistence;
