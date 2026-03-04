//! Ed25519 key management and signing operations
//!
//! Per Thales spec: Ed25519 via ed25519-dalek, per-agent keypairs,
//! public keys registered in council keyring.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use std::path::Path;

/// Generate a new Ed25519 signing keypair.
pub fn generate_keypair() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

/// Sign a message with the given signing key.
pub fn sign(key: &SigningKey, message: &[u8]) -> Signature {
    key.sign(message)
}

/// Verify a signature against a public key.
pub fn verify(
    public_key: &VerifyingKey,
    message: &[u8],
    signature: &Signature,
) -> Result<(), SigningError> {
    public_key
        .verify(message, signature)
        .map_err(|_| SigningError::InvalidSignature)
}

/// Load a signing key from a 32-byte seed file (hex-encoded).
pub fn load_signing_key(path: impl AsRef<Path>) -> Result<SigningKey, SigningError> {
    let contents = std::fs::read_to_string(path)?;
    let seed_bytes = hex::decode(contents.trim()).map_err(|_| SigningError::InvalidKeyFormat)?;
    if seed_bytes.len() != 32 {
        return Err(SigningError::InvalidKeyFormat);
    }
    let seed: [u8; 32] = seed_bytes
        .try_into()
        .map_err(|_| SigningError::InvalidKeyFormat)?;
    Ok(SigningKey::from_bytes(&seed))
}

/// Save a signing key as hex-encoded seed.
pub fn save_signing_key(key: &SigningKey, path: impl AsRef<Path>) -> Result<(), SigningError> {
    let hex_seed = hex::encode(key.to_bytes());
    std::fs::write(path, hex_seed)?;
    Ok(())
}

/// Export the public key as hex-encoded bytes.
pub fn public_key_hex(key: &SigningKey) -> String {
    hex::encode(key.verifying_key().to_bytes())
}

/// Parse a hex-encoded public key.
pub fn parse_public_key(hex_str: &str) -> Result<VerifyingKey, SigningError> {
    let bytes = hex::decode(hex_str).map_err(|_| SigningError::InvalidKeyFormat)?;
    if bytes.len() != 32 {
        return Err(SigningError::InvalidKeyFormat);
    }
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| SigningError::InvalidKeyFormat)?;
    VerifyingKey::from_bytes(&arr).map_err(|_| SigningError::InvalidKeyFormat)
}

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid key format")]
    InvalidKeyFormat,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_sign_verify() {
        let key = generate_keypair();
        let message = b"test message";
        let sig = sign(&key, message);
        assert!(verify(&key.verifying_key(), message, &sig).is_ok());
    }

    #[test]
    fn test_wrong_message_fails() {
        let key = generate_keypair();
        let sig = sign(&key, b"correct");
        assert!(verify(&key.verifying_key(), b"wrong", &sig).is_err());
    }

    #[test]
    fn test_public_key_roundtrip() {
        let key = generate_keypair();
        let hex_str = public_key_hex(&key);
        let parsed = parse_public_key(&hex_str).unwrap();
        assert_eq!(key.verifying_key(), parsed);
    }
}
