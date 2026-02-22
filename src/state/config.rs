//! Configuration management for ming-qiao
//!
//! Handles loading and managing runtime configuration including observation modes,
//! notification triggers, and gating rules.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Observation mode for Merlin oversight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ObservationMode {
    /// All messages flow freely. Events logged. Merlin reviews async.
    #[default]
    Passive,

    /// Merlin notified on triggers (keywords, priority, decision type). No blocking.
    Advisory,

    /// Certain actions require Merlin approval before proceeding.
    Gated,
}

/// Notification triggers for advisory mode
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotifyOn {
    /// Priority levels that trigger notification
    #[serde(default)]
    pub priority: Vec<String>,

    /// Keywords in messages that trigger notification
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Decision types that trigger notification
    #[serde(default)]
    pub decision_type: Vec<String>,
}

/// Gating rules for gated mode
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GateRules {
    /// Decision types that require approval
    #[serde(default)]
    pub decision_type: Vec<String>,
}

/// NATS messaging configuration
///
/// Subject hierarchy and stream topology are defined in code
/// (`nats::subjects` and `nats::streams`), not in config. Only the
/// connection parameters live here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    /// Whether NATS integration is enabled
    #[serde(default)]
    pub enabled: bool,

    /// NATS server URL
    #[serde(default = "default_nats_url")]
    pub url: String,
}

fn default_nats_url() -> String {
    "nats://localhost:4222".to_string()
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: default_nats_url(),
        }
    }
}

/// Database connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// SurrealDB connection URL.
    /// Use `mem://` for in-memory (tests/default), `ws://host:port` for shared server.
    #[serde(default = "default_database_url")]
    pub url: String,

    /// Optional username for authentication (required for `ws://` connections)
    #[serde(default)]
    pub username: Option<String>,

    /// Optional password for authentication (required for `ws://` connections)
    #[serde(default)]
    pub password: Option<String>,
}

fn default_database_url() -> String {
    "mem://".to_string()
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
            username: None,
            password: None,
        }
    }
}

/// Runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Project identifier for NATS subject routing.
    ///
    /// Used as the `{project}` token in NATS subjects like
    /// `am.agent.{agent}.task.{project}.*` and `am.events.{project}`.
    /// Different ming-qiao instances serving different AstralMaris projects
    /// should use distinct project tokens.
    #[serde(default = "default_project")]
    pub project: String,

    /// Current observation mode
    #[serde(default)]
    pub mode: ObservationMode,

    /// Notification triggers (for advisory mode)
    #[serde(default)]
    pub notify_on: NotifyOn,

    /// Gating rules (for gated mode)
    #[serde(default)]
    pub gate: GateRules,

    /// Data directory path
    #[serde(default = "default_data_dir")]
    pub data_dir: String,

    /// HTTP server port
    #[serde(default = "default_port")]
    pub port: u16,

    /// NATS messaging configuration
    #[serde(default)]
    pub nats: NatsConfig,

    /// Database connection configuration
    #[serde(default)]
    pub database: DatabaseConfig,

    /// Watcher configurations for real-time event observers
    #[serde(default)]
    pub watchers: Vec<crate::watcher::WatcherConfig>,
}

fn default_project() -> String {
    "mingqiao".to_string()
}

fn default_data_dir() -> String {
    "data".to_string()
}

fn default_port() -> u16 {
    7777
}

impl Default for Config {
    fn default() -> Self {
        Self {
            project: default_project(),
            mode: ObservationMode::default(),
            notify_on: NotifyOn {
                priority: vec!["high".to_string(), "critical".to_string()],
                keywords: vec![
                    "breaking change".to_string(),
                    "security".to_string(),
                    "blocked".to_string(),
                ],
                decision_type: vec!["architectural".to_string()],
            },
            gate: GateRules {
                decision_type: vec!["architectural".to_string(), "external".to_string()],
            },
            data_dir: default_data_dir(),
            port: default_port(),
            nats: NatsConfig::default(),
            database: DatabaseConfig::default(),
            watchers: Vec::new(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file.
    ///
    /// Relative `data_dir` paths are resolved against the config file's parent
    /// directory, not the process CWD. This is essential for MCP mode where
    /// Claude Desktop spawns the process from an arbitrary working directory.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;

        // Resolve relative data_dir against config file's parent directory
        let data_path = Path::new(&config.data_dir);
        if data_path.is_relative() {
            if let Some(config_dir) = path.parent() {
                let absolute = config_dir.join(data_path);
                config.data_dir = absolute.to_string_lossy().into_owned();
            }
        }

        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the events file path
    pub fn events_path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.data_dir).join("events.jsonl")
    }

    /// Get the artifacts directory path
    pub fn artifacts_path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.data_dir).join("artifacts")
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.project, "mingqiao");
        assert_eq!(config.mode, ObservationMode::Passive);
        assert_eq!(config.port, 7777);
        assert_eq!(config.data_dir, "data");
    }

    #[test]
    fn test_observation_mode_serialization() {
        let modes = vec![
            (ObservationMode::Passive, "\"passive\""),
            (ObservationMode::Advisory, "\"advisory\""),
            (ObservationMode::Gated, "\"gated\""),
        ];

        for (mode, expected) in modes {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, expected);

            let deserialized: ObservationMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_events_path() {
        let config = Config::default();
        assert_eq!(
            config.events_path(),
            std::path::PathBuf::from("data/events.jsonl")
        );
    }

    #[test]
    fn test_config_with_custom_data_dir() {
        let mut config = Config::default();
        config.data_dir = "/custom/path".to_string();
        assert_eq!(
            config.events_path(),
            std::path::PathBuf::from("/custom/path/events.jsonl")
        );
    }

    #[test]
    fn test_nats_config_defaults() {
        let nats = NatsConfig::default();
        assert!(!nats.enabled);
        assert_eq!(nats.url, "nats://localhost:4222");
    }

    #[test]
    fn test_config_missing_nats_section_uses_defaults() {
        let toml_str = r#"
            mode = "passive"
            data_dir = "data"
            port = 7777
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.project, "mingqiao");
        assert!(!config.nats.enabled);
        assert_eq!(config.nats.url, "nats://localhost:4222");
    }

    #[test]
    fn test_config_with_custom_project() {
        let toml_str = r#"
            project = "buildermoon"
            mode = "passive"
            data_dir = "data"
            port = 7777
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.project, "buildermoon");
    }

    #[test]
    fn test_database_config_defaults() {
        let db = DatabaseConfig::default();
        assert_eq!(db.url, "mem://");
        assert!(db.username.is_none());
        assert!(db.password.is_none());
    }

    #[test]
    fn test_config_missing_database_section_uses_defaults() {
        let toml_str = r#"
            mode = "passive"
            data_dir = "data"
            port = 7777
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.database.url, "mem://");
        assert!(config.database.username.is_none());
    }

    #[test]
    fn test_config_with_database_ws() {
        let toml_str = r#"
            mode = "passive"
            data_dir = "data"
            port = 7777

            [database]
            url = "ws://localhost:8000"
            username = "root"
            password = "root"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.database.url, "ws://localhost:8000");
        assert_eq!(config.database.username.as_deref(), Some("root"));
        assert_eq!(config.database.password.as_deref(), Some("root"));
    }

    #[test]
    fn test_config_with_nats_enabled() {
        let toml_str = r#"
            mode = "passive"
            data_dir = "data"
            port = 7777

            [nats]
            enabled = true
            url = "nats://custom:4222"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.nats.enabled);
        assert_eq!(config.nats.url, "nats://custom:4222");
    }
}
