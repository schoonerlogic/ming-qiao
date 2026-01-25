// Event reader for append-only event log
//
// Provides streaming replay and tail capabilities for reading JSONL event logs.

use super::error::EventError;
use super::schema::EventEnvelope;
use serde_json::from_str;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Event log reader with replay and tail capabilities
///
/// Reads events from a JSONL file with efficient streaming.
pub struct EventReader {
    /// Path to the event log file
    path: PathBuf,
}

impl EventReader {
    /// Open an event log for reading
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, EventError> {
        let path = path.as_ref().to_path_buf();

        // Verify file exists
        if !path.exists() {
            return Err(EventError::NotFound(format!(
                "Event log not found: {}",
                path.display()
            )));
        }

        Ok(Self { path })
    }

    /// Replay all events from the beginning
    ///
    /// Returns an iterator that streams events without loading the entire file into memory.
    pub fn replay(&self) -> Result<ReplayIterator, EventError> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);

        Ok(ReplayIterator {
            reader,
            line_number: 0,
        })
    }

    /// Read events after a given event ID
    ///
    /// Scans linearly from the start and returns events that come after the specified ID.
    /// Useful for incremental sync or tailing the log.
    pub fn after(&self, event_id: &str) -> Result<TailIterator, EventError> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);

        Ok(TailIterator {
            reader,
            line_number: 0,
            target_id: event_id.to_string(),
            found_target: false,
        })
    }

    /// Get count of events in the log
    pub fn count(&self) -> Result<usize, EventError> {
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);

        Ok(reader.lines().count())
    }

    /// Check if log file exists
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Get the path to the event log file
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Streaming iterator for replaying all events
///
/// Yields events one at a time without loading the entire file into memory.
pub struct ReplayIterator {
    reader: BufReader<File>,
    line_number: usize,
}

impl Iterator for ReplayIterator {
    type Item = Result<EventEnvelope, EventError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.line_number += 1;

        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => None, // EOF
            Ok(_) => {
                // Parse JSON
                match from_str::<EventEnvelope>(&line) {
                    Ok(event) => Some(Ok(event)),
                    Err(e) => Some(Err(EventError::InvalidFormat {
                        line: self.line_number,
                        message: e.to_string(),
                    })),
                }
            }
            Err(e) => Some(Err(EventError::InvalidFormat {
                line: self.line_number,
                message: e.to_string(),
            })),
        }
    }
}

/// Streaming iterator for reading events after a given ID
///
/// Scans linearly and yields events after finding the target ID.
pub struct TailIterator {
    reader: BufReader<File>,
    line_number: usize,
    target_id: String,
    found_target: bool,
}

impl Iterator for TailIterator {
    type Item = Result<EventEnvelope, EventError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.line_number += 1;

            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => return None, // EOF
                Ok(_) => {
                    // Parse JSON
                    let event: Result<EventEnvelope, _> = from_str(&line);
                    match event {
                        Ok(ev) => {
                            // Check if we found the target ID
                            if !self.found_target {
                                if ev.id.to_string() == self.target_id {
                                    self.found_target = true;
                                }
                                // Skip events before finding the target
                                continue;
                            } else {
                                // Yield events after the target
                                return Some(Ok(ev));
                            }
                        }
                        Err(e) => {
                            return Some(Err(EventError::InvalidFormat {
                                line: self.line_number,
                                message: e.to_string(),
                            }))
                        }
                    }
                }
                Err(e) => {
                    return Some(Err(EventError::InvalidFormat {
                        line: self.line_number,
                        message: e.to_string(),
                    }))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::events::schema::{EventPayload, EventType, MessageEvent, Priority};

    fn create_test_event(id: &str, agent: &str) -> EventEnvelope {
        EventEnvelope {
            id: Uuid::parse_str(id).unwrap(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: agent.to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: agent.to_string(),
                to: "all".to_string(),
                subject: "Test".to_string(),
                content: "Test message".to_string(),
                thread_id: None,
                priority: Priority::Normal,
            }),
        }
    }

    fn create_test_log() -> (tempfile::NamedTempFile, Vec<String>) {
        use std::io::Write;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let mut event_ids = Vec::new();

        for i in 0..3 {
            // UUID format: 8-4-4-4-12 hex digits
            let id = format!("01234567-89ab-cdef-0123-{:012x}", i);
            event_ids.push(id.clone());

            let event = create_test_event(&id, &format!("agent-{}", i));
            let json = serde_json::to_string(&event).unwrap();
            writeln!(temp_file.as_file(), "{}", json).unwrap();
        }

        (temp_file, event_ids)
    }

    #[test]
    fn test_event_reader_open_nonexistent() {
        let reader = EventReader::open("/nonexistent/path/events.jsonl");
        assert!(reader.is_err());
    }

    #[test]
    fn test_event_reader_replay_all() {
        let (temp_file, event_ids) = create_test_log();
        let reader = EventReader::open(temp_file.path()).unwrap();

        let events: Vec<Result<EventEnvelope, _>> = reader.replay().unwrap().collect();
        assert_eq!(events.len(), 3);

        // Verify event IDs match
        for (i, event_result) in events.iter().enumerate() {
            let event = event_result.as_ref().unwrap();
            assert_eq!(event.id.to_string(), event_ids[i]);
        }
    }

    #[test]
    fn test_event_reader_count() {
        let (temp_file, _) = create_test_log();
        let reader = EventReader::open(temp_file.path()).unwrap();

        let count = reader.count().unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_event_reader_exists() {
        let (temp_file, _) = create_test_log();
        let reader = EventReader::open(temp_file.path()).unwrap();

        assert!(reader.exists());
    }

    #[test]
    fn test_event_reader_after() {
        let (temp_file, event_ids) = create_test_log();
        let reader = EventReader::open(temp_file.path()).unwrap();

        // Request events after the first one
        let events: Vec<Result<EventEnvelope, _>> = reader.after(&event_ids[0]).unwrap().collect();

        // Should have 2 events (after the first)
        assert_eq!(events.len(), 2);

        // Verify they're the correct events
        assert_eq!(events[0].as_ref().unwrap().id.to_string(), event_ids[1]);
        assert_eq!(events[1].as_ref().unwrap().id.to_string(), event_ids[2]);
    }

    #[test]
    fn test_event_reader_after_nonexistent_id() {
        let (temp_file, _) = create_test_log();
        let reader = EventReader::open(temp_file.path()).unwrap();

        // Request events after a non-existent ID
        let events: Vec<Result<EventEnvelope, _>> = reader
            .after("01234567-89ab-cdef-ffffffffffff")
            .unwrap()
            .collect();

        // Should return 0 events (target never found, so no "after" events)
        assert_eq!(events.len(), 0);
    }
}
