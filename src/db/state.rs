// src/db/state.rs
// Indexer State Persistence

use crate::db::error::IndexerError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Tracks the progress of the event log indexer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerState {
    /// Last processed event ID
    pub last_event_id: Option<String>,

    /// Last processed timestamp
    pub last_timestamp: Option<DateTime<Utc>>,

    /// Count of events processed
    pub events_processed: u64,
}

impl Default for IndexerState {
    fn default() -> Self {
        Self {
            last_event_id: None,
            last_timestamp: None,
            events_processed: 0,
        }
    }
}

impl IndexerState {
    /// Load indexer state from a JSON file.
    ///
    /// # Arguments
    /// * `path` - Path to the state file (e.g., `data/indexer_state.json`)
    ///
    /// # Returns
    /// * `Ok(IndexerState)` - Loaded state, or default state if file doesn't exist
    /// * `Err(IndexerError)` - Failed to read or parse the file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, IndexerError> {
        let path = path.as_ref();

        // If file doesn't exist, return default state
        if !path.exists() {
            return Ok(Self::default());
        }

        // Read and parse the JSON file
        let json = fs::read_to_string(path).map_err(|e| {
            IndexerError::StatePersistence(format!("Failed to read state file: {}", e))
        })?;

        serde_json::from_str(&json).map_err(|e| {
            IndexerError::StatePersistence(format!("Failed to parse state file: {}", e))
        })
    }

    /// Save indexer state to a JSON file.
    ///
    /// # Arguments
    /// * `path` - Path to the state file (e.g., `data/indexer_state.json`)
    ///
    /// # Returns
    /// * `Ok(())` - State saved successfully
    /// * `Err(IndexerError)` - Failed to write the file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), IndexerError> {
        let path = path.as_ref();

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                IndexerError::StatePersistence(format!(
                    "Failed to create state directory: {}",
                    e
                ))
            })?;
        }

        // Serialize state to JSON
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            IndexerError::StatePersistence(format!("Failed to serialize state: {}", e))
        })?;

        // Write to file
        fs::write(path, json).map_err(|e| {
            IndexerError::StatePersistence(format!("Failed to write state file: {}", e))
        })?;

        Ok(())
    }
}
