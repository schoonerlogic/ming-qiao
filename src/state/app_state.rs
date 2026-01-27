//! Application state shared across MCP and HTTP servers
//!
//! Provides thread-safe access to the event log, configuration, and in-memory
//! caches for quick lookups.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::db::Indexer;
use crate::events::{EventEnvelope, EventWriter};
use crate::merlin::MerlinNotifier;
use crate::state::config::{Config, ObservationMode};

/// Broadcast channel capacity for event notifications
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Shared application state
///
/// This is the central state object that both MCP and HTTP servers use.
/// It provides thread-safe access to configuration and will provide access
/// to the event log once Luban completes the persistence layer.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    /// Runtime configuration (can be updated)
    config: RwLock<Config>,

    /// Data directory path
    data_dir: PathBuf,

    /// Agent ID for this instance (set from environment)
    agent_id: Option<String>,

    /// Broadcast channel for real-time event notifications
    event_tx: broadcast::Sender<EventEnvelope>,

    /// Database indexer for O(1) queries (materialized views from event log)
    indexer: RwLock<Indexer>,

    /// Merlin notification system
    merlin_notifier: Arc<MerlinNotifier>,

    /// Event writer for append-only log
    event_writer: Arc<EventWriter>,
}

impl AppState {
    /// Create a new application state with default configuration
    pub fn new() -> Self {
        Self::with_config(Config::default())
    }

    /// Create application state with custom configuration
    pub fn with_config(config: Config) -> Self {
        let data_dir = PathBuf::from(&config.data_dir);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        // Create indexer with events path
        let events_path = data_dir.join("events.jsonl");

        // Try to create indexer - if event log doesn't exist, that's OK
        // The indexer will be empty until refresh_indexer() is called
        let indexer = if events_path.exists() {
            // Event log exists - create indexer
            match Indexer::new(&events_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to create indexer from {}: {}. Indexer will start empty.",
                        events_path.display(),
                        e
                    );
                    // Create empty indexer anyway
                    Indexer::new(&events_path).unwrap_or_else(|_| {
                        // This should work now since the file exists
                        panic!("Failed to create indexer for existing event log")
                    })
                }
            }
        } else {
            // Event log doesn't exist yet - indexer will be empty
            eprintln!(
                "Info: Event log not found at {}. Indexer will be empty until events are written.",
                events_path.display()
            );
            // Create empty indexer - EventReader will fail but that's OK
            // We'll catch up when refresh_indexer() is called
            Indexer::new(&events_path).unwrap_or_else(|_| {
                // File doesn't exist, create a default empty indexer
                // This is a workaround - we create the file first
                if let Some(parent) = events_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                // Create empty file
                let _ = std::fs::File::create(&events_path);
                // Now try again
                Indexer::new(&events_path).expect("Failed to create indexer after creating file")
            })
        };

        // Create event writer
        let event_writer = match EventWriter::new_with_path(&events_path) {
            Ok(writer) => writer,
            Err(e) => {
                eprintln!(
                    "Warning: Failed to create event writer for {}: {}. Using default path.",
                    events_path.display(),
                    e
                );
                // Fall back to default writer
                match EventWriter::new() {
                    Ok(w) => w,
                    Err(e2) => {
                        panic!("Failed to create event writer: {}", e2);
                    }
                }
            }
        };

        Self {
            inner: Arc::new(AppStateInner {
                config: RwLock::new(config),
                data_dir,
                agent_id: std::env::var("MING_QIAO_AGENT_ID").ok(),
                event_tx,
                indexer: RwLock::new(indexer),
                merlin_notifier: Arc::new(MerlinNotifier::new()),
                event_writer: Arc::new(event_writer),
            }),
        }
    }

    /// Load configuration from file and create state
    pub fn load(
        config_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, crate::state::config::ConfigError> {
        let config = Config::load(config_path)?;
        Ok(Self::with_config(config))
    }

    /// Get the current observation mode
    pub async fn mode(&self) -> ObservationMode {
        self.inner.config.read().await.mode
    }

    /// Set the observation mode
    pub async fn set_mode(&self, mode: ObservationMode) {
        self.inner.config.write().await.mode = mode;
    }

    /// Get a clone of the current configuration
    pub async fn config(&self) -> Config {
        self.inner.config.read().await.clone()
    }

    /// Update configuration
    pub async fn update_config<F>(&self, f: F)
    where
        F: FnOnce(&mut Config),
    {
        let mut config = self.inner.config.write().await;
        f(&mut config);
    }

    /// Get the data directory path
    pub fn data_dir(&self) -> &PathBuf {
        &self.inner.data_dir
    }

    /// Get the events file path
    pub fn events_path(&self) -> PathBuf {
        self.inner.data_dir.join("events.jsonl")
    }

    /// Get the artifacts directory path
    pub fn artifacts_path(&self) -> PathBuf {
        self.inner.data_dir.join("artifacts")
    }

    /// Get the agent ID (if set)
    pub fn agent_id(&self) -> Option<&str> {
        self.inner.agent_id.as_deref()
    }

    /// Ensure data directories exist
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.inner.data_dir)?;
        std::fs::create_dir_all(self.artifacts_path())?;
        Ok(())
    }

    /// Broadcast an event to all connected WebSocket clients
    ///
    /// Returns the number of receivers that received the event.
    /// Returns 0 if no receivers are connected (this is not an error).
    pub fn broadcast_event(&self, event: EventEnvelope) -> usize {
        // send() returns Err if there are no receivers, which is fine
        self.inner.event_tx.send(event).unwrap_or(0)
    }

    /// Get the Merlin notifier
    ///
    /// Used for sending notifications to the human operator.
    pub fn merlin_notifier(&self) -> &Arc<MerlinNotifier> {
        &self.inner.merlin_notifier
    }

    /// Get the event writer
    ///
    /// Used for writing events to the append-only log.
    pub fn event_writer(&self) -> &Arc<EventWriter> {
        &self.inner.event_writer
    }

    /// Subscribe to the event broadcast channel
    ///
    /// Returns a receiver that will receive all events broadcast after subscription.
    /// Note: Events broadcast before subscription are not received.
    pub fn subscribe_events(&self) -> broadcast::Receiver<EventEnvelope> {
        self.inner.event_tx.subscribe()
    }

    /// Get read access to the indexer
    ///
    /// Returns a read lock guard for the indexer. Use this to query the materialized views.
    pub async fn indexer(&self) -> tokio::sync::RwLockReadGuard<'_, Indexer> {
        self.inner.indexer.read().await
    }

    /// Get write access to the indexer
    ///
    /// Returns a write lock guard for the indexer. Use this to refresh or modify the indexer.
    pub async fn indexer_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, Indexer> {
        self.inner.indexer.write().await
    }

    /// Refresh the indexer by catching up with new events from the log
    ///
    /// This will process all new events since the last refresh and update the materialized views.
    /// Returns the number of events processed, or an error if refresh fails.
    pub async fn refresh_indexer(&self) -> Result<usize, crate::db::IndexerError> {
        let mut indexer = self.inner.indexer.write().await;
        indexer.catch_up()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_state() {
        let state = AppState::new();
        assert_eq!(state.mode().await, ObservationMode::Passive);
    }

    #[tokio::test]
    async fn test_set_mode() {
        let state = AppState::new();
        assert_eq!(state.mode().await, ObservationMode::Passive);

        state.set_mode(ObservationMode::Gated).await;
        assert_eq!(state.mode().await, ObservationMode::Gated);
    }

    #[tokio::test]
    async fn test_update_config() {
        let state = AppState::new();

        state
            .update_config(|config| {
                config.port = 9999;
            })
            .await;

        let config = state.config().await;
        assert_eq!(config.port, 9999);
    }

    #[test]
    fn test_paths() {
        let state = AppState::new();
        assert_eq!(state.events_path(), PathBuf::from("data/events.jsonl"));
        assert_eq!(state.artifacts_path(), PathBuf::from("data/artifacts"));
    }

    #[test]
    fn test_custom_data_dir() {
        let mut config = Config::default();
        config.data_dir = "/tmp/ming-qiao-test".to_string();

        let state = AppState::with_config(config);
        assert_eq!(
            state.events_path(),
            PathBuf::from("/tmp/ming-qiao-test/events.jsonl")
        );
    }
}
