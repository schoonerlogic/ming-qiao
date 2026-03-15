//! Envelope builder — wraps ProcessorResult into ASTROLABE ingest format.
//!
//! Agent: luban

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::processor::ProcessorResult;

/// An ASTROLABE envelope — canonical format for quarantine and ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub schema_version: String,
    pub envelope_id: String,
    pub created_at: String,
    pub enrichment_status: String,
    pub source_type: String,
    pub source_url: String,
    pub processor_version: String,
    pub content_hash_sha256: String,
    pub title: String,
    pub episode_body: String,
    pub domain_tags: Vec<String>,
    pub metadata: serde_json::Value,
    pub post_sanitization_hash: String,
    pub fetched_at: String,
    pub processed_at: String,
    #[serde(default)]
    pub embedded_artifacts: Vec<serde_json::Value>,
}

/// Arguments compatible with the ASTROLABE MCP `add_memory` tool.
#[derive(Debug, Clone, Serialize)]
pub struct IngestArgs {
    pub name: String,
    pub episode_body: String,
    pub group_id: String,
    pub source: String,
    pub source_description: String,
}

/// Build an ASTROLABE envelope from a ProcessorResult.
pub fn build_envelope(result: &ProcessorResult) -> Envelope {
    let mut body_parts = vec![format!("Title: {}", result.title)];

    if let Some(authors) = result.metadata.get("authors").and_then(|v| v.as_str()) {
        body_parts.push(format!("Authors: {authors}"));
    }

    body_parts.push(format!("\n{}", result.content));

    if !result.domain_tags.is_empty() {
        body_parts.push(format!("\nDomain: {}", result.domain_tags.join(", ")));
    }

    let episode_body = body_parts.join("\n");

    let content_prefix: String = result.content.chars().take(500).collect();
    let hash_input = format!("{}|{}|{}", result.source_url, result.title, content_prefix);
    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    let content_hash_full = hex::encode(hasher.finalize());
    let post_sanitization_hash = content_hash_full[..16].to_string();

    Envelope {
        schema_version: "1.0.0".to_string(),
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
        metadata: serde_json::to_value(&result.metadata).unwrap_or_default(),
        post_sanitization_hash,
        fetched_at: result.fetched_at.to_rfc3339(),
        processed_at: result.processed_at.to_rfc3339(),
        embedded_artifacts: result.embedded_artifacts.clone(),
    }
}

/// Write an envelope to the quarantine/pending directory.
pub fn write_to_quarantine(envelope: &Envelope, quarantine_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(quarantine_dir)?;
    let filename = format!("{}.json", envelope.envelope_id);
    let filepath = quarantine_dir.join(filename);
    let json = serde_json::to_string_pretty(envelope)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&filepath, json)?;
    Ok(filepath)
}

/// Convenience struct for building and writing envelopes.
pub struct EnvelopeBuilder {
    quarantine_dir: PathBuf,
}

impl EnvelopeBuilder {
    pub fn new(quarantine_dir: PathBuf) -> Self {
        Self { quarantine_dir }
    }

    pub fn build(&self, result: &ProcessorResult) -> Envelope {
        build_envelope(result)
    }

    pub fn quarantine(&self, result: &ProcessorResult) -> std::io::Result<PathBuf> {
        let envelope = build_envelope(result);
        write_to_quarantine(&envelope, &self.quarantine_dir)
    }

    pub fn build_ingest_args(envelope: &Envelope) -> IngestArgs {
        IngestArgs {
            name: envelope.title.clone(),
            episode_body: envelope.episode_body.clone(),
            group_id: "astrolabe_main".to_string(),
            source: "text".to_string(),
            source_description: format!(
                "{}:{} (processor v{})",
                envelope.source_type, envelope.source_url, envelope.processor_version
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::ProcessorResult;
    use std::collections::HashMap;

    fn sample_result() -> ProcessorResult {
        let now = Utc::now();
        ProcessorResult {
            title: "Test Paper".to_string(),
            content: "This is test content about machine learning".to_string(),
            source_url: "https://arxiv.org/abs/2401.12345".to_string(),
            source_type: "arxiv-url".to_string(),
            processor_version: "1.0.0".to_string(),
            domain_tags: vec!["research".to_string(), "machine-learning".to_string()],
            metadata: {
                let mut m = HashMap::new();
                m.insert("arxiv_id".to_string(), serde_json::json!("2401.12345"));
                m.insert("authors".to_string(), serde_json::json!("Alice, Bob"));
                m
            },
            embedded_artifacts: vec![],
            fetched_at: now,
            processed_at: now,
        }
    }

    #[test]
    fn test_build_envelope() {
        let result = sample_result();
        let envelope = build_envelope(&result);
        assert_eq!(envelope.schema_version, "1.0.0");
        assert_eq!(envelope.enrichment_status, "quarantined");
        assert!(envelope.envelope_id.starts_with("arxiv-url-"));
        assert_eq!(envelope.post_sanitization_hash.len(), 16);
        assert_eq!(envelope.content_hash_sha256.len(), 64);
        assert!(envelope.episode_body.contains("Title: Test Paper"));
        assert!(envelope.episode_body.contains("Authors: Alice, Bob"));
    }

    #[test]
    fn test_write_to_quarantine() {
        let dir = tempfile::tempdir().unwrap();
        let quarantine = dir.path().join("pending");
        let result = sample_result();
        let envelope = build_envelope(&result);
        let path = write_to_quarantine(&envelope, &quarantine).unwrap();
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["title"], "Test Paper");
    }

    #[test]
    fn test_ingest_args() {
        let result = sample_result();
        let envelope = build_envelope(&result);
        let args = EnvelopeBuilder::build_ingest_args(&envelope);
        assert_eq!(args.name, "Test Paper");
        assert_eq!(args.group_id, "astrolabe_main");
        assert!(args.source_description.contains("arxiv-url"));
    }
}
