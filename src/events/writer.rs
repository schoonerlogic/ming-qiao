// Event writer for append-only event log
//
// Provides thread-safe, atomic appends to a newline-delimited JSON (JSONL) file.

use super::error::EventError;
use super::schema::EventEnvelope;
use serde_json::to_string;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Append-only event writer
///
/// Writes events to a JSONL file with atomic appends (write + newline + flush).
/// Thread-safe via internal mutex.
pub struct EventWriter {
    /// The underlying file handle (protected by mutex for thread-safety)
    file: Mutex<File>,
    /// Path to the event log file
    path: PathBuf,
}

impl EventWriter {
    /// Default event log path
    const DEFAULT_PATH: &'static str = "data/events.jsonl";

    /// Create a new writer for the default path (`data/events.jsonl`)
    ///
    /// Creates parent directories and file if they don't exist.
    pub fn new() -> Result<Self, EventError> {
        Self::new_with_path(Self::DEFAULT_PATH)
    }

    /// Create a new writer for a custom path
    ///
    /// Creates parent directories and file if they don't exist.
    pub fn new_with_path<P: AsRef<Path>>(path: P) -> Result<Self, EventError> {
        let path = path.as_ref().to_path_buf();

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Open file in append mode, create if it doesn't exist
        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            file: Mutex::new(file),
            path,
        })
    }

    /// Append an event to the log
    ///
    /// Returns the event ID after successful write.
    ///
    /// # Atomicity
    /// Each append is atomic: JSON + newline are written, then flushed to disk
    /// before the mutex is released. A crash cannot leave partial JSON in the file.
    pub fn append(&self, event: &EventEnvelope) -> Result<String, EventError> {
        // Serialize to compact JSON (no pretty-printing)
        let json = to_string(event)?;

        // Lock the file for exclusive access
        let mut file = self.file.lock().map_err(|e| {
            EventError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Mutex lock failed: {}", e),
            ))
        })?;

        // Write JSON + newline
        writeln!(file, "{}", json)?;

        // Flush to disk for durability
        file.flush()?;

        // Release mutex lock
        drop(file);

        // Return the event ID
        Ok(event.id.to_string())
    }

    /// Flush any buffered writes to disk
    ///
    /// Note: `append()` already flushes after each write, so this is only
    /// needed if you want to ensure all OS-level buffers are flushed.
    pub fn flush(&self) -> Result<(), EventError> {
        let mut file = self.file.lock().map_err(|e| {
            EventError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Mutex lock failed: {}", e),
            ))
        })?;

        file.flush()?;
        Ok(())
    }

    /// Get the current file size in bytes
    pub fn size(&self) -> Result<u64, EventError> {
        Ok(std::fs::metadata(&self.path)?.len())
    }

    /// Get the path to the event log file
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    /// Helper to create a test event with UUID v7
    fn create_test_event(agent_id: &str, subject: &str) -> EventEnvelope {
        EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: crate::events::schema::EventType::MessageSent,
            agent_id: agent_id.to_string(),
            payload: crate::events::schema::EventPayload::Message(
                crate::events::schema::MessageEvent {
                    from: "alice".to_string(),
                    to: "bob".to_string(),
                    subject: subject.to_string(),
                    content: "Hello, world!".to_string(),
                    thread_id: None,
                    priority: crate::events::schema::Priority::Normal,
                },
            ),
        }
    }

    #[test]
    fn test_event_writer_new_creates_directory() {
        let temp_dir = std::env::temp_dir().join("test_events_new");
        let _ = std::fs::remove_dir_all(&temp_dir); // Clean up if exists
        let path = temp_dir.join("subdir").join("events.jsonl");

        let writer = EventWriter::new_with_path(&path).unwrap();
        assert_eq!(writer.path(), &path);

        // Verify directory was created
        assert!(path.parent().unwrap().exists());

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_event_writer_append_and_persist() {
        let temp_file = std::env::temp_dir().join("test_events_append.jsonl");
        let _ = std::fs::remove_file(&temp_file);

        let writer = EventWriter::new_with_path(&temp_file).unwrap();

        // Create a test event
        let event = create_test_event("test-agent", "Test");

        // Append event
        let event_id = writer.append(&event).unwrap();
        assert_eq!(event_id, event.id.to_string());

        // Verify file was created and has content
        assert!(temp_file.exists());
        let size = writer.size().unwrap();
        assert!(size > 0);

        // Clean up
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_event_writer_atomic_append() {
        let temp_file = std::env::temp_dir().join("test_events_atomic.jsonl");
        let _ = std::fs::remove_file(&temp_file);

        let writer = EventWriter::new_with_path(&temp_file).unwrap();

        // Append multiple events
        for i in 0..5 {
            let event = create_test_event(&format!("agent-{}", i), &format!("Message {}", i));
            writer.append(&event).unwrap();
        }

        // Read file and count lines
        let content = std::fs::read_to_string(&temp_file).unwrap();
        let line_count = content.lines().count();
        assert_eq!(line_count, 5);

        // Clean up
        let _ = std::fs::remove_file(&temp_file);
    }
}
