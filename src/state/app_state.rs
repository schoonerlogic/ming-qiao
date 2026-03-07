//! Application state shared across MCP and HTTP servers
//!
//! Provides thread-safe access to the persistence layer, configuration, and
//! in-memory caches for quick lookups.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::crypto::keyring::Keyring;
use crate::crypto::nonce::NonceRegistry;
use crate::db::{Indexer, Persistence};
use crate::events::EventEnvelope;
use crate::http::auth::AuthConfig;
use crate::merlin::MerlinNotifier;
use crate::nats::{NatsAgentClient, NatsMessage};
use crate::state::config::{Config, ObservationMode};

/// Broadcast channel capacity for event notifications
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Broadcast channel capacity for NATS messages from other agents
const NATS_CHANNEL_CAPACITY: usize = 256;

/// Shared application state
///
/// This is the central state object that both MCP and HTTP servers use.
/// It provides thread-safe access to configuration, the SurrealDB persistence
/// layer, and in-memory materialized views via the Indexer.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    /// Runtime configuration (can be updated)
    config: RwLock<Config>,

    /// Data directory path (for artifacts, etc.)
    data_dir: PathBuf,

    /// Agent ID for this instance (set from environment)
    agent_id: Option<String>,

    /// Broadcast channel for real-time event notifications
    event_tx: broadcast::Sender<EventEnvelope>,

    /// SurrealDB persistence layer (always available — in-memory)
    persistence: Persistence,

    /// Database indexer for O(1) queries (materialized views from events)
    indexer: RwLock<Indexer>,

    /// Merlin notification system
    merlin_notifier: Arc<MerlinNotifier>,

    /// NATS agent client (None if disabled or unreachable)
    nats_client: RwLock<Option<NatsAgentClient>>,

    /// Broadcast channel for NATS messages received from other agents
    nats_tx: broadcast::Sender<NatsMessage>,

    /// HTTP API auth config (bearer tokens per agent)
    auth_config: RwLock<AuthConfig>,

    /// Ed25519 council keyring for signed-envelope verification (RA-004)
    keyring: Keyring,

    /// Nonce registry for replay defense (120s TTL per Thales spec)
    nonce_registry: NonceRegistry,
}

impl AppState {
    /// Create application state with default configuration.
    ///
    /// Initializes SurrealDB in-memory persistence and an empty Indexer.
    pub async fn new() -> Self {
        Self::with_config(Config::default()).await
    }

    /// Create application state with custom configuration.
    ///
    /// Initializes SurrealDB persistence (with env-var credential override)
    /// and an empty Indexer. Loads HTTP auth config if enabled.
    pub async fn with_config(config: Config) -> Self {
        let data_dir = PathBuf::from(&config.data_dir);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (nats_tx, _) = broadcast::channel(NATS_CHANNEL_CAPACITY);

        // Resolve DB credentials (env vars override config file)
        let db_username = config.database.resolved_username();
        let db_password = config.database.resolved_password();

        // Retry SurrealDB connection with exponential backoff.
        // SurrealDB may not be ready immediately after a system reboot.
        let persistence = {
            let max_retries = 5;
            let mut delay = std::time::Duration::from_secs(1);
            let mut last_err = None;
            let mut connected = None;

            for attempt in 1..=max_retries {
                match Persistence::connect_with_auth(
                    &config.database.url,
                    db_username.as_deref(),
                    db_password.as_deref(),
                    config.database.auth_level,
                )
                .await
                {
                    Ok(p) => {
                        if attempt > 1 {
                            tracing::info!(
                                "Connected to SurrealDB on attempt {}/{}",
                                attempt,
                                max_retries
                            );
                        }
                        connected = Some(p);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "SurrealDB connection attempt {}/{} failed: {} (retrying in {:?})",
                            attempt,
                            max_retries,
                            e,
                            delay
                        );
                        last_err = Some(e);
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                    }
                }
            }

            connected.unwrap_or_else(|| {
                panic!(
                    "Failed to connect to SurrealDB after {} attempts: {}",
                    max_retries,
                    last_err.map(|e| e.to_string()).unwrap_or_default()
                )
            })
        };

        // Load HTTP auth config if enabled
        let auth_config = if config.auth.enabled {
            match config.auth.token_file.as_ref() {
                Some(path) => match AuthConfig::load(path) {
                    Ok(ac) => {
                        tracing::info!("Loaded auth config from {}", path);
                        ac
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load auth config from {}: {}, auth disabled", path, e);
                        AuthConfig::disabled()
                    }
                },
                None => {
                    tracing::warn!("Auth enabled but no token_file configured, auth disabled");
                    AuthConfig::disabled()
                }
            }
        } else {
            AuthConfig::disabled()
        };

        // Hydrate Indexer from SurrealDB (replay stored events for cold-start consistency)
        let mut indexer = Indexer::new();
        match persistence.get_all_events().await {
            Ok(events) => {
                let count = events.len();
                for event in &events {
                    if let Err(e) = indexer.process_event(event) {
                        tracing::warn!("Indexer hydration skipped event {}: {}", event.id, e);
                    }
                }
                if count > 0 {
                    tracing::info!("Indexer hydrated with {} events from SurrealDB", count);
                }
            }
            Err(e) => {
                tracing::warn!("Indexer hydration failed, starting empty: {}", e);
            }
        }

        // Load Ed25519 council keyring for signed-envelope verification (RA-004)
        let keyring = if let Some(ref keyring_path) = config.auth.keyring_file {
            match Keyring::load(keyring_path) {
                Ok(kr) => {
                    tracing::info!("Loaded council keyring from {} ({} agents)", keyring_path, kr.agent_ids().len());
                    kr
                }
                Err(e) => {
                    tracing::warn!("Failed to load keyring from {}: {}, signed-envelope verification unavailable", keyring_path, e);
                    Keyring::empty()
                }
            }
        } else {
            tracing::info!("No keyring_file configured, signed-envelope verification unavailable");
            Keyring::empty()
        };

        // Nonce registry with 120s TTL per Thales spec
        let nonce_registry = NonceRegistry::new(120);

        Self {
            inner: Arc::new(AppStateInner {
                config: RwLock::new(config),
                data_dir,
                agent_id: std::env::var("MING_QIAO_AGENT_ID").ok(),
                event_tx,
                persistence,
                indexer: RwLock::new(indexer),
                merlin_notifier: Arc::new(MerlinNotifier::new()),
                nats_client: RwLock::new(None),
                nats_tx,
                auth_config: RwLock::new(auth_config),
                keyring,
                nonce_registry,
            }),
        }
    }

    /// Load configuration from file and create state.
    pub async fn load(
        config_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, crate::state::config::ConfigError> {
        let config = Config::load(config_path)?;
        Ok(Self::with_config(config).await)
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

    /// Get a reference to the persistence layer.
    pub fn persistence(&self) -> &Persistence {
        &self.inner.persistence
    }

    /// Broadcast an event to all connected WebSocket clients
    pub fn broadcast_event(&self, event: EventEnvelope) -> usize {
        self.inner.event_tx.send(event).unwrap_or(0)
    }

    /// Get the Merlin notifier
    pub fn merlin_notifier(&self) -> &Arc<MerlinNotifier> {
        &self.inner.merlin_notifier
    }

    /// Subscribe to the event broadcast channel
    pub fn subscribe_events(&self) -> broadcast::Receiver<EventEnvelope> {
        self.inner.event_tx.subscribe()
    }

    /// Get read access to the indexer
    pub async fn indexer(&self) -> tokio::sync::RwLockReadGuard<'_, Indexer> {
        self.inner.indexer.read().await
    }

    /// Get write access to the indexer
    pub async fn indexer_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, Indexer> {
        self.inner.indexer.write().await
    }

    /// Store a connected NATS agent client
    pub async fn set_nats_client(&self, client: NatsAgentClient) {
        *self.inner.nats_client.write().await = Some(client);
    }

    /// Get write access to the NATS client
    pub async fn nats_client_mut(
        &self,
    ) -> tokio::sync::RwLockWriteGuard<'_, Option<NatsAgentClient>> {
        self.inner.nats_client.write().await
    }

    /// Check if NATS is connected
    pub async fn nats_connected(&self) -> bool {
        self.inner.nats_client.read().await.is_some()
    }

    /// Get the broadcast sender for injecting events into the local bus
    pub fn event_sender(&self) -> broadcast::Sender<EventEnvelope> {
        self.inner.event_tx.clone()
    }

    /// Get the broadcast sender for NATS messages from other agents.
    pub fn nats_message_sender(&self) -> broadcast::Sender<NatsMessage> {
        self.inner.nats_tx.clone()
    }

    /// Subscribe to NATS messages from other agents.
    pub fn subscribe_nats_messages(&self) -> broadcast::Receiver<NatsMessage> {
        self.inner.nats_tx.subscribe()
    }

    /// Get a clone of the current auth config.
    pub async fn auth_config(&self) -> AuthConfig {
        self.inner.auth_config.read().await.clone()
    }

    /// Get a reference to the council keyring (RA-004).
    pub fn keyring(&self) -> &Keyring {
        &self.inner.keyring
    }

    /// Get a reference to the nonce registry (RA-004).
    pub fn nonce_registry(&self) -> &NonceRegistry {
        &self.inner.nonce_registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_state() {
        let state = AppState::new().await;
        assert_eq!(state.mode().await, ObservationMode::Passive);
    }

    #[tokio::test]
    async fn test_set_mode() {
        let state = AppState::new().await;
        assert_eq!(state.mode().await, ObservationMode::Passive);

        state.set_mode(ObservationMode::Gated).await;
        assert_eq!(state.mode().await, ObservationMode::Gated);
    }

    #[tokio::test]
    async fn test_update_config() {
        let state = AppState::new().await;

        state
            .update_config(|config| {
                config.port = 9999;
            })
            .await;

        let config = state.config().await;
        assert_eq!(config.port, 9999);
    }

    #[tokio::test]
    async fn test_paths() {
        let state = AppState::new().await;
        assert_eq!(state.artifacts_path(), PathBuf::from("data/artifacts"));
    }

    #[tokio::test]
    async fn test_custom_data_dir() {
        let mut config = Config::default();
        config.data_dir = "/tmp/ming-qiao-test".to_string();

        let state = AppState::with_config(config).await;
        assert_eq!(
            state.artifacts_path(),
            PathBuf::from("/tmp/ming-qiao-test/artifacts")
        );
    }

    #[tokio::test]
    async fn test_persistence_accessible() {
        let state = AppState::new().await;
        // Persistence should be available and working
        let _persistence = state.persistence();
    }
}
