// Error types for event persistence
//
// Defines all errors that can occur during event reading and writing.

use std::io;

use serde_json;
use thiserror::Error;

/// Errors that can occur during event persistence
#[derive(Debug, Error)]
pub enum EventError {
    /// IO error from file operations
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// JSON serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Event not found (e.g., when looking for a specific ID)
    #[error("Event not found: {0}")]
    NotFound(String),

    /// Invalid event log format at a specific line
    #[error("Invalid event log format at line {line}: {message}")]
    InvalidFormat { line: usize, message: String },
}
