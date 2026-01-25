// src/db/error.rs
// Database Indexer Error Types

use crate::events::error::EventError;

/// Errors that can occur during indexing operations.
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    /// Error from the event log (EventReader)
    #[error("Event log error: {0}")]
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
