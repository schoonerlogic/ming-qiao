// src/db/error.rs
// Database Error Types

use crate::events::error::EventError;

/// Errors that can occur during indexing operations.
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    /// Error from the events module
    #[error("Event error: {0}")]
    EventLog(#[from] EventError),

    /// Error persisting or loading indexer state
    #[error("State persistence error: {0}")]
    StatePersistence(String),

    /// Invalid event data (e.g., missing required fields)
    #[error("Invalid event {event_id}: {reason}")]
    InvalidEvent {
        event_id: String,
        reason: String,
    },
}

/// Errors from the SurrealDB persistence layer.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    /// SurrealDB engine error
    #[error("SurrealDB error: {0}")]
    Database(String),

    /// JSON serialization/deserialization failure
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Requested record does not exist
    #[error("Record not found: {0}")]
    NotFound(String),
}
