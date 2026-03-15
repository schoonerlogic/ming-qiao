//! Core processor traits and types.
//!
//! Every artifact processor implements [`ArtifactProcessor`] and returns
//! [`ProcessorResult`]. The envelope builder then wraps results into
//! ASTROLABE-compatible envelopes for quarantine and ingestion.
//!
//! Agent: luban

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Input to a processor — a raw artifact from a collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawArtifact {
    /// Classification label (e.g., "arxiv_url", "x_post")
    pub source_type: String,

    /// Email subject or equivalent
    pub subject: String,

    /// Email body or raw content
    pub body: String,

    /// Unique identifier from source (e.g., Gmail Message-ID)
    #[serde(default)]
    pub message_id: String,

    /// When the artifact was collected (ISO 8601)
    #[serde(default)]
    pub date: String,

    /// Attachments: [{filename, content_type, data}]
    #[serde(default)]
    pub attachments: Vec<Attachment>,

    /// Collector-provided extra data
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// An attachment on a raw artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: String,
    pub content_type: String,
    /// Base64-encoded attachment data
    pub data: String,
}

/// Output of any processor — input to the envelope builder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorResult {
    /// Human-readable title
    pub title: String,

    /// Extracted/cleaned text content
    pub content: String,

    /// Original URL or source identifier
    pub source_url: String,

    /// Processor type that produced this (e.g., "x-post", "arxiv-url")
    pub source_type: String,

    /// Semver of the processor (e.g., "1.0.0")
    pub processor_version: String,

    /// Topic classification tags
    #[serde(default)]
    pub domain_tags: Vec<String>,

    /// Processor-specific metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,

    /// Nested artifacts to process
    #[serde(default)]
    pub embedded_artifacts: Vec<serde_json::Value>,

    /// When the source content was fetched
    pub fetched_at: DateTime<Utc>,

    /// When processing completed
    pub processed_at: DateTime<Utc>,
}

impl ProcessorResult {
    /// Validate that required fields are non-empty.
    pub fn is_valid(&self) -> bool {
        !self.title.is_empty() && !self.content.is_empty() && !self.source_url.is_empty()
    }
}

/// Trait for all artifact processors.
///
/// Each processor handles one or more `source_type` values and transforms
/// a [`RawArtifact`] into a structured [`ProcessorResult`].
pub trait ArtifactProcessor {
    /// Processor identifier (e.g., "x-post", "arxiv-url").
    fn name(&self) -> &str;

    /// Semantic version string.
    fn version(&self) -> &str;

    /// List of source types this processor handles.
    fn accepts(&self) -> &[&str];

    /// Quick check: can this processor handle this artifact?
    fn can_process(&self, artifact: &RawArtifact) -> bool {
        self.accepts()
            .iter()
            .any(|&t| t == artifact.source_type)
    }

    /// Extract and structure the artifact content.
    ///
    /// # Errors
    ///
    /// Returns an error if the artifact cannot be processed.
    fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError>;

    /// Validate processor output. Default checks required fields.
    fn validate(&self, result: &ProcessorResult) -> bool {
        result.is_valid()
    }
}

/// Errors that can occur during artifact processing.
#[derive(Debug, thiserror::Error)]
pub enum ProcessorError {
    #[error("unsupported source type: {0}")]
    UnsupportedType(String),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("content extraction failed: {0}")]
    ExtractionFailed(String),

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_artifact_defaults() {
        let artifact = RawArtifact {
            source_type: "arxiv_url".to_string(),
            subject: "Test paper".to_string(),
            body: "https://arxiv.org/abs/2401.12345".to_string(),
            message_id: String::new(),
            date: String::new(),
            attachments: Vec::new(),
            metadata: HashMap::new(),
        };
        assert_eq!(artifact.source_type, "arxiv_url");
        assert!(artifact.attachments.is_empty());
    }

    #[test]
    fn processor_result_validation() {
        let now = Utc::now();
        let valid = ProcessorResult {
            title: "Test".to_string(),
            content: "Content".to_string(),
            source_url: "https://example.com".to_string(),
            source_type: "test".to_string(),
            processor_version: "1.0.0".to_string(),
            domain_tags: vec![],
            metadata: HashMap::new(),
            embedded_artifacts: vec![],
            fetched_at: now,
            processed_at: now,
        };
        assert!(valid.is_valid());

        let invalid = ProcessorResult {
            title: String::new(),
            ..valid
        };
        assert!(!invalid.is_valid());
    }

    #[test]
    fn processor_result_serialization() {
        let now = Utc::now();
        let result = ProcessorResult {
            title: "Test Paper".to_string(),
            content: "Abstract text".to_string(),
            source_url: "https://arxiv.org/abs/2401.12345".to_string(),
            source_type: "arxiv-url".to_string(),
            processor_version: "1.0.0".to_string(),
            domain_tags: vec!["ai".to_string()],
            metadata: HashMap::new(),
            embedded_artifacts: vec![],
            fetched_at: now,
            processed_at: now,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ProcessorResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "Test Paper");
        assert_eq!(deserialized.source_type, "arxiv-url");
    }
}
