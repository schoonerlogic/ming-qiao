//! Envelope builder — wraps ProcessorResult into ASTROLABE ingest format.
//!
//! Produces JSON envelope files for quarantine review, and provides
//! conversion to ASTROLABE MCP `add_memory` arguments for ingestion.
//!
//! Agent: luban

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::processor::ProcessorResult;

/// Schema version for envelopes produced by this module.
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Default ASTROLABE group ID for ingestion.
pub const DEFAULT_GROUP_ID: &str = "astrolabe_main";

/// An ASTROLABE envelope — the canonical format for quarantine storage
/// and eventual ingestion into the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub schema_version: String,
    pub envelope_id: String,
    pub created_at: String,
    pub enrichment_status: String,

    // Source provenance (Ogma S-1 requirements)
    pub source_type: String,
    pub source_url: String,
    pub processor_version: String,
    pub content_hash_sha256: String,

    // Content
    pub title: String,
    pub episode_body: String,
    pub domain_tags: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,

    // Dedup
    pub post_sanitization_hash: String,

    // Timestamps
    pub fetched_at: String,
    pub processed_at: String,

    // Embedded artifacts for recursive processing
    #[serde(default)]
    pub embedded_artifacts: Vec<serde_json::Value>,
}

/// Arguments for the ASTROLABE MCP `add_memory` tool.
#[derive(Debug, Clone, Serialize)]
pub struct IngestArgs {
    pub name: String,
    pub episode_body: String,
    pub group_id: String,
    pub source: String,
    pub source_description: String,
}

/// Build an ASTROLABE envelope from a [`ProcessorResult`].
pub fn build_envelope(result: &ProcessorResult) -> Envelope {
    // Build episode body as structured text for graphiti ingestion
    let mut body_parts = vec![format!("Title: {}", result.title)];

    if let Some(authors) = result.metadata.get("authors") {
        if let Some(authors_str) = authors.as_str() {
            body_parts.push(format!("Authors: {authors_str}"));
        }
    }

    body_parts.push(format!("\n{}", result.content));

    if !result.domain_tags.is_empty() {
        body_parts.push(format!("\nDomain: {}", result.domain_tags.join(", ")));
    }

    let episode_body = body_parts.join("\n");

    // Compute dedup hash over sanitized content
    let hash_input = format!(
        "{}|{}|{}",
        result.source_url,
        result.title,
        &result.content[..result.content.len().min(500)]
    );
    let content_hash_full = hex_sha256(hash_input.as_bytes());
    let post_sanitization_hash = content_hash_full[..16].to_string();

    Envelope {
        schema_version: SCHEMA_VERSION.to_string(),
        envelope_id: format!("{}-{post_sanitization_hash}", result.source_type),
        created_at: Utc::now().to_rfc3339(),
        enrichment_status: "quarantined".to_string(),
        source_type: result.source_type.clone(),
        source_url: result.source_url.clone(),
        processor_version: result.processor_version.clone(),
        content_hash_sha256: content_hash_full,
        title: result.title.clone(),
        episode_body,
        domain_tags: result.domain_tags.clone(),
        metadata: result.metadata.clone(),
        post_sanitization_hash,
        fetched_at: result.fetched_at.to_rfc3339(),
        processed_at: result.processed_at.to_rfc3339(),
        embedded_artifacts: result.embedded_artifacts.clone(),
    }
}

/// Write an envelope to the quarantine/pending directory.
///
/// Returns the path to the written file.
pub fn write_to_quarantine(
    envelope: &Envelope,
    quarantine_dir: &Path,
) -> Result<PathBuf, EnvelopeError> {
    fs::create_dir_all(quarantine_dir).map_err(|e| EnvelopeError::Io(e.to_string()))?;

    let filename = format!("{}.json", envelope.envelope_id);
    let filepath = quarantine_dir.join(filename);

    let json =
        serde_json::to_string_pretty(envelope).map_err(|e| EnvelopeError::Serialize(e.to_string()))?;

    fs::write(&filepath, json).map_err(|e| EnvelopeError::Io(e.to_string()))?;

    Ok(filepath)
}

/// Convenience struct for building and writing envelopes.
pub struct EnvelopeBuilder {
    quarantine_dir: PathBuf,
}

impl EnvelopeBuilder {
    /// Create a new builder with the given quarantine directory.
    pub fn new(quarantine_dir: impl Into<PathBuf>) -> Self {
        Self {
            quarantine_dir: quarantine_dir.into(),
        }
    }

    /// Build an envelope from a ProcessorResult.
    pub fn build(&self, result: &ProcessorResult) -> Envelope {
        build_envelope(result)
    }

    /// Build envelope and write to quarantine. Returns file path.
    pub fn quarantine(&self, result: &ProcessorResult) -> Result<PathBuf, EnvelopeError> {
        let envelope = build_envelope(result);
        write_to_quarantine(&envelope, &self.quarantine_dir)
    }

    /// Convert envelope to `add_memory` MCP tool arguments.
    pub fn ingest_args(envelope: &Envelope) -> IngestArgs {
        IngestArgs {
            name: envelope.title.clone(),
            episode_body: envelope.episode_body.clone(),
            group_id: DEFAULT_GROUP_ID.to_string(),
            source: "text".to_string(),
            source_description: format!(
                "{}:{} (processor v{})",
                envelope.source_type, envelope.source_url, envelope.processor_version
            ),
        }
    }
}

/// Errors from the envelope module.
#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("serialization error: {0}")]
    Serialize(String),
}

/// Compute hex-encoded SHA-256 digest.
fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_result() -> ProcessorResult {
        let now = Utc::now();
        ProcessorResult {
            title: "Attention Is All You Need".to_string(),
            content: "We propose a new architecture, the Transformer, based on attention mechanisms.".to_string(),
            source_url: "https://arxiv.org/abs/1706.03762".to_string(),
            source_type: "arxiv-url".to_string(),
            processor_version: "1.0.0".to_string(),
            domain_tags: vec!["machine-learning".to_string(), "transformers".to_string()],
            metadata: {
                let mut m = HashMap::new();
                m.insert("authors".to_string(), serde_json::json!("Vaswani et al."));
                m.insert("arxiv_id".to_string(), serde_json::json!("1706.03762"));
                m
            },
            embedded_artifacts: vec![],
            fetched_at: now,
            processed_at: now,
        }
    }

    #[test]
    fn build_envelope_structure() {
        let result = sample_result();
        let envelope = build_envelope(&result);

        assert_eq!(envelope.schema_version, "1.0.0");
        assert!(envelope.envelope_id.starts_with("arxiv-url-"));
        assert_eq!(envelope.enrichment_status, "quarantined");
        assert_eq!(envelope.source_type, "arxiv-url");
        assert_eq!(envelope.title, "Attention Is All You Need");
        assert!(envelope.episode_body.contains("Authors: Vaswani et al."));
        assert!(envelope.episode_body.contains("Domain: machine-learning, transformers"));
        assert_eq!(envelope.post_sanitization_hash.len(), 16);
        assert_eq!(envelope.content_hash_sha256.len(), 64);
    }

    #[test]
    fn build_envelope_deterministic_hash() {
        let result = sample_result();
        let e1 = build_envelope(&result);
        let e2 = build_envelope(&result);
        assert_eq!(e1.content_hash_sha256, e2.content_hash_sha256);
        assert_eq!(e1.post_sanitization_hash, e2.post_sanitization_hash);
    }

    #[test]
    fn write_quarantine_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let quarantine = dir.path().join("quarantine").join("pending");
        let result = sample_result();
        let envelope = build_envelope(&result);

        let path = write_to_quarantine(&envelope, &quarantine).unwrap();
        assert!(path.exists());

        let contents = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["title"], "Attention Is All You Need");
    }

    #[test]
    fn ingest_args_format() {
        let result = sample_result();
        let envelope = build_envelope(&result);
        let args = EnvelopeBuilder::ingest_args(&envelope);

        assert_eq!(args.name, "Attention Is All You Need");
        assert_eq!(args.group_id, "astrolabe_main");
        assert_eq!(args.source, "text");
        assert!(args.source_description.contains("arxiv-url"));
        assert!(args.source_description.contains("processor v1.0.0"));
    }

    #[test]
    fn envelope_serialization_roundtrip() {
        let result = sample_result();
        let envelope = build_envelope(&result);

        let json = serde_json::to_string(&envelope).unwrap();
        let deserialized: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, envelope.title);
        assert_eq!(deserialized.content_hash_sha256, envelope.content_hash_sha256);
    }
}
