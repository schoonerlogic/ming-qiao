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

/// Runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
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
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
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
}
