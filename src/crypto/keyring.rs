//! Council keyring — per-agent public key registry
//!
//! Per Thales spec: public keys registered in a council keyring file
//! (simple JSON for now, SPIRE-backed in P1).

use std::collections::HashMap;
use std::path::Path;

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use super::signing;

/// In-memory keyring mapping agent IDs to their Ed25519 public keys.
pub struct Keyring {
    keys: HashMap<String, VerifyingKey>,
}

/// Serializable keyring format for JSON persistence.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeyringFile {
    pub agents: HashMap<String, AgentKey>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentKey {
    /// Hex-encoded Ed25519 public key (32 bytes)
    pub public_key: String,
}

impl Keyring {
    /// Create an empty keyring.
    pub fn empty() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    /// Load keyring from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, KeyringError> {
        let contents = std::fs::read_to_string(path)?;
        let file: KeyringFile = serde_json::from_str(&contents)?;

        let mut keys = HashMap::new();
        for (agent_id, agent_key) in file.agents {
            let verifying_key = signing::parse_public_key(&agent_key.public_key)
                .map_err(|_| KeyringError::InvalidKey(agent_id.clone()))?;
            keys.insert(agent_id, verifying_key);
        }

        Ok(Self { keys })
    }

    /// Register an agent's public key.
    pub fn register(&mut self, agent_id: &str, public_key: &VerifyingKey) {
        self.keys.insert(agent_id.to_string(), *public_key);
    }

    /// Look up an agent's public key.
    pub fn get_public_key(&self, agent_id: &str) -> Option<VerifyingKey> {
        self.keys.get(agent_id).copied()
    }

    /// Check if an agent is registered.
    pub fn contains(&self, agent_id: &str) -> bool {
        self.keys.contains_key(agent_id)
    }

    /// Get all registered agent IDs.
    pub fn agent_ids(&self) -> Vec<&str> {
        self.keys.keys().map(|s| s.as_str()).collect()
    }

    /// Save keyring to a JSON file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), KeyringError> {
        let agents: HashMap<String, AgentKey> = self
            .keys
            .iter()
            .map(|(id, key)| {
                (
                    id.clone(),
                    AgentKey {
                        public_key: hex::encode(key.to_bytes()),
                    },
                )
            })
            .collect();

        let file = KeyringFile { agents };
        let json = serde_json::to_string_pretty(&file)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeyringError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid key for agent: {0}")]
    InvalidKey(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signing::generate_keypair;

    #[test]
    fn test_register_and_lookup() {
        let key = generate_keypair();
        let mut keyring = Keyring::empty();
        keyring.register("aleph", &key.verifying_key());

        assert!(keyring.contains("aleph"));
        assert!(!keyring.contains("luban"));
        assert_eq!(keyring.get_public_key("aleph"), Some(key.verifying_key()));
    }

    #[test]
    fn test_save_and_load() {
        let key1 = generate_keypair();
        let key2 = generate_keypair();

        let mut keyring = Keyring::empty();
        keyring.register("aleph", &key1.verifying_key());
        keyring.register("luban", &key2.verifying_key());

        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("keyring.json");
        keyring.save(&path).unwrap();

        let loaded = Keyring::load(&path).unwrap();
        assert_eq!(
            loaded.get_public_key("aleph"),
            Some(key1.verifying_key())
        );
        assert_eq!(
            loaded.get_public_key("luban"),
            Some(key2.verifying_key())
        );
    }
}
