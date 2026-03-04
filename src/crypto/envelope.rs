//! Signed event envelopes per Thales architectural decision
//!
//! Envelope fields: event_id (UUID v7), from_agent, timestamp_utc (ISO8601),
//! nonce (32-byte random), payload_hash (SHA-256), signature (Ed25519).
//!
//! Signature covers: event_id || timestamp_utc || nonce || payload_hash
//! (newline-separated for unambiguous parsing).
//!
//! Replay defense: reject events with timestamp_utc older than 60s
//! OR with a seen nonce (nonce registry with 120s TTL).

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::keyring::Keyring;
use super::nonce::NonceRegistry;
use super::signing;

/// Maximum age of a signed event (60 seconds per Thales spec).
const MAX_EVENT_AGE_SECS: i64 = 60;

/// A signed event envelope wrapping an arbitrary JSON payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEnvelope {
    /// UUID v7 event identifier
    pub event_id: String,
    /// Agent that created and signed this event
    pub from_agent: String,
    /// ISO8601 timestamp (UTC)
    pub timestamp_utc: String,
    /// 32-byte random nonce (hex-encoded)
    pub nonce: String,
    /// SHA-256 hash of canonical JSON payload (hex-encoded)
    pub payload_hash: String,
    /// Ed25519 signature over signing_message (hex-encoded)
    pub signature: String,
    /// The actual event payload
    pub payload: serde_json::Value,
}

impl SignedEnvelope {
    /// Create and sign a new envelope.
    pub fn create(
        event_id: &str,
        from_agent: &str,
        payload: &serde_json::Value,
        signing_key: &SigningKey,
    ) -> Self {
        let timestamp_utc = Utc::now().to_rfc3339();
        let nonce = generate_nonce();
        let payload_hash = hash_payload(payload);

        let signing_message = build_signing_message(event_id, &timestamp_utc, &nonce, &payload_hash);
        let signature = signing::sign(signing_key, signing_message.as_bytes());

        Self {
            event_id: event_id.to_string(),
            from_agent: from_agent.to_string(),
            timestamp_utc,
            nonce,
            payload_hash,
            signature: hex::encode(signature.to_bytes()),
            payload: payload.clone(),
        }
    }

    /// Verify this envelope's signature, timestamp freshness, and nonce uniqueness.
    ///
    /// Returns the verified agent ID on success.
    pub fn verify(
        &self,
        keyring: &Keyring,
        nonce_registry: &NonceRegistry,
    ) -> Result<String, EnvelopeError> {
        // 1. Check timestamp freshness (reject > 60s old)
        let event_time = DateTime::parse_from_rfc3339(&self.timestamp_utc)
            .map_err(|_| EnvelopeError::InvalidTimestamp)?
            .with_timezone(&Utc);

        let age = Utc::now().signed_duration_since(event_time);
        if age.num_seconds() > MAX_EVENT_AGE_SECS {
            return Err(EnvelopeError::Expired {
                age_secs: age.num_seconds(),
            });
        }
        // Also reject future timestamps (> 5s into the future)
        if age.num_seconds() < -5 {
            return Err(EnvelopeError::FutureTimestamp);
        }

        // 2. Check nonce uniqueness (reject seen nonces)
        if !nonce_registry.check_and_insert(&self.nonce) {
            return Err(EnvelopeError::ReplayedNonce);
        }

        // 3. Verify payload hash
        let expected_hash = hash_payload(&self.payload);
        if self.payload_hash != expected_hash {
            return Err(EnvelopeError::PayloadHashMismatch);
        }

        // 4. Look up agent's public key in keyring
        let public_key = keyring
            .get_public_key(&self.from_agent)
            .ok_or_else(|| EnvelopeError::UnknownAgent(self.from_agent.clone()))?;

        // 5. Verify Ed25519 signature
        let signing_message = build_signing_message(
            &self.event_id,
            &self.timestamp_utc,
            &self.nonce,
            &self.payload_hash,
        );

        let sig_bytes =
            hex::decode(&self.signature).map_err(|_| EnvelopeError::InvalidSignatureFormat)?;
        let signature = Signature::from_bytes(
            sig_bytes
                .as_slice()
                .try_into()
                .map_err(|_| EnvelopeError::InvalidSignatureFormat)?,
        );

        signing::verify(&public_key, signing_message.as_bytes(), &signature)
            .map_err(|_| EnvelopeError::InvalidSignature)?;

        Ok(self.from_agent.clone())
    }
}

/// Build the message that gets signed: event_id\ntimestamp\nnonce\npayload_hash
fn build_signing_message(
    event_id: &str,
    timestamp_utc: &str,
    nonce: &str,
    payload_hash: &str,
) -> String {
    format!("{}\n{}\n{}\n{}", event_id, timestamp_utc, nonce, payload_hash)
}

/// SHA-256 hash of canonical (sorted-keys) JSON payload, hex-encoded.
fn hash_payload(payload: &serde_json::Value) -> String {
    // Canonical JSON: serde_json serializes with sorted keys when using to_string
    // on a Value (keys are sorted in BTreeMap after round-trip). For true canonical
    // form, we serialize to string and hash that.
    let canonical = serde_json::to_string(payload).unwrap_or_default();
    let hash = Sha256::digest(canonical.as_bytes());
    hex::encode(hash)
}

/// Generate a 32-byte random nonce, hex-encoded.
fn generate_nonce() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("Event expired (age: {age_secs}s, max: {}s)", MAX_EVENT_AGE_SECS)]
    Expired { age_secs: i64 },

    #[error("Event timestamp is in the future")]
    FutureTimestamp,

    #[error("Invalid timestamp format")]
    InvalidTimestamp,

    #[error("Replayed nonce detected")]
    ReplayedNonce,

    #[error("Payload hash mismatch")]
    PayloadHashMismatch,

    #[error("Unknown agent: {0}")]
    UnknownAgent(String),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid signature format")]
    InvalidSignatureFormat,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signing::generate_keypair;

    #[test]
    fn test_create_and_verify() {
        let key = generate_keypair();
        let mut keyring = Keyring::empty();
        keyring.register("aleph", &key.verifying_key());
        let nonce_registry = NonceRegistry::new(120);

        let payload = serde_json::json!({"from": "aleph", "content": "hello"});
        let envelope = SignedEnvelope::create("event-001", "aleph", &payload, &key);

        let result = envelope.verify(&keyring, &nonce_registry);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "aleph");
    }

    #[test]
    fn test_replay_rejected() {
        let key = generate_keypair();
        let mut keyring = Keyring::empty();
        keyring.register("aleph", &key.verifying_key());
        let nonce_registry = NonceRegistry::new(120);

        let payload = serde_json::json!({"test": true});
        let envelope = SignedEnvelope::create("event-002", "aleph", &payload, &key);

        // First verify succeeds
        assert!(envelope.verify(&keyring, &nonce_registry).is_ok());
        // Second verify with same nonce fails
        assert!(matches!(
            envelope.verify(&keyring, &nonce_registry),
            Err(EnvelopeError::ReplayedNonce)
        ));
    }

    #[test]
    fn test_tampered_payload_rejected() {
        let key = generate_keypair();
        let mut keyring = Keyring::empty();
        keyring.register("aleph", &key.verifying_key());
        let nonce_registry = NonceRegistry::new(120);

        let payload = serde_json::json!({"content": "original"});
        let mut envelope = SignedEnvelope::create("event-003", "aleph", &payload, &key);

        // Tamper with payload
        envelope.payload = serde_json::json!({"content": "tampered"});

        assert!(matches!(
            envelope.verify(&keyring, &nonce_registry),
            Err(EnvelopeError::PayloadHashMismatch)
        ));
    }

    #[test]
    fn test_unknown_agent_rejected() {
        let key = generate_keypair();
        let keyring = Keyring::empty(); // No agents registered
        let nonce_registry = NonceRegistry::new(120);

        let payload = serde_json::json!({"test": true});
        let envelope = SignedEnvelope::create("event-004", "unknown", &payload, &key);

        assert!(matches!(
            envelope.verify(&keyring, &nonce_registry),
            Err(EnvelopeError::UnknownAgent(_))
        ));
    }

    #[test]
    fn test_wrong_key_rejected() {
        let key1 = generate_keypair();
        let key2 = generate_keypair();
        let mut keyring = Keyring::empty();
        // Register key2's public key for "aleph" but sign with key1
        keyring.register("aleph", &key2.verifying_key());
        let nonce_registry = NonceRegistry::new(120);

        let payload = serde_json::json!({"test": true});
        let envelope = SignedEnvelope::create("event-005", "aleph", &payload, &key1);

        assert!(matches!(
            envelope.verify(&keyring, &nonce_registry),
            Err(EnvelopeError::InvalidSignature)
        ));
    }
}
