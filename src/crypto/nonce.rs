//! Nonce registry with TTL-based expiry for replay defense
//!
//! Per Thales spec: 120s TTL for nonce registry entries.
//! Events with previously-seen nonces are rejected.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Thread-safe nonce registry that tracks seen nonces with expiry.
pub struct NonceRegistry {
    inner: Mutex<NonceRegistryInner>,
    ttl: Duration,
}

struct NonceRegistryInner {
    nonces: HashMap<String, Instant>,
    last_cleanup: Instant,
}

impl NonceRegistry {
    /// Create a new registry with the given TTL in seconds.
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Mutex::new(NonceRegistryInner {
                nonces: HashMap::new(),
                last_cleanup: Instant::now(),
            }),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Check if a nonce is fresh (not seen before) and register it.
    ///
    /// Returns `true` if the nonce is new (valid), `false` if it's a replay.
    pub fn check_and_insert(&self, nonce: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();

        // Periodic cleanup of expired entries (every 30s)
        if now.duration_since(inner.last_cleanup) > Duration::from_secs(30) {
            inner
                .nonces
                .retain(|_, &mut inserted_at| now.duration_since(inserted_at) < self.ttl);
            inner.last_cleanup = now;
        }

        // Check if nonce exists and is still within TTL
        if let Some(&inserted_at) = inner.nonces.get(nonce) {
            if now.duration_since(inserted_at) < self.ttl {
                return false; // Replay detected
            }
            // Expired entry — treat as new
        }

        inner.nonces.insert(nonce.to_string(), now);
        true
    }

    /// Get the number of tracked nonces (for monitoring).
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().nonces.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_nonce_accepted() {
        let registry = NonceRegistry::new(120);
        assert!(registry.check_and_insert("nonce-001"));
    }

    #[test]
    fn test_duplicate_nonce_rejected() {
        let registry = NonceRegistry::new(120);
        assert!(registry.check_and_insert("nonce-002"));
        assert!(!registry.check_and_insert("nonce-002"));
    }

    #[test]
    fn test_different_nonces_accepted() {
        let registry = NonceRegistry::new(120);
        assert!(registry.check_and_insert("nonce-a"));
        assert!(registry.check_and_insert("nonce-b"));
        assert!(registry.check_and_insert("nonce-c"));
        assert_eq!(registry.len(), 3);
    }
}
