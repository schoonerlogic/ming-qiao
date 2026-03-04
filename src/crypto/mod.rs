//! Cryptographic primitives for Security P0
//!
//! Implements Ed25519 signed-event envelopes per Thales architectural decision:
//! - Agent keypairs for event signing
//! - Council keyring for public key distribution
//! - Nonce registry with TTL for replay defense
//! - Signed envelope creation and verification

pub mod envelope;
pub mod keyring;
pub mod nonce;
pub mod signing;
