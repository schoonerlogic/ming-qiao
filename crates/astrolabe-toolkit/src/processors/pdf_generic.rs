//! Generic PDF processor — handles non-arXiv PDF attachments.
//!
//! Agent: luban

use base64::Engine;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::processor::{ArtifactProcessor, ProcessorError, ProcessorResult, RawArtifact};

const VERSION: &str = "1.0.0";

/// Infer the best title from available sources.
fn infer_title(subject: &str, filename: &str) -> String {
    // Use email subject if descriptive enough
    if subject.len() > 5 && !subject.starts_with("Fwd:") {
        return subject.trim().to_string();
    }

    // Use filename without extension
    if !filename.is_empty() {
        let stem = std::path::Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename);
        let name = stem.replace(['_', '-'], " ");
        if name.len() > 3 {
            // Title case
            return name
                .split_whitespace()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => {
                            f.to_uppercase().to_string() + &c.as_str().to_lowercase()
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    }

    "Untitled PDF".to_string()
}

/// Generic PDF processor.
pub struct PdfGenericProcessor;

impl PdfGenericProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl ArtifactProcessor for PdfGenericProcessor {
    fn name(&self) -> &str {
        "pdf-generic"
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn accepts(&self) -> &[&str] {
        &["pdf_attachment"]
    }

    fn can_process(&self, artifact: &RawArtifact) -> bool {
        artifact
            .attachments
            .iter()
            .any(|a| a.content_type.starts_with("application/pdf"))
    }

    fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError> {
        let now = Utc::now();

        // Find first PDF attachment
        let att = artifact
            .attachments
            .iter()
            .find(|a| a.content_type.starts_with("application/pdf"))
            .ok_or_else(|| ProcessorError::MissingField("PDF attachment".to_string()))?;

        let pdf_data = base64::engine::general_purpose::STANDARD
            .decode(&att.data)
            .map_err(|e| ProcessorError::ExtractionFailed(format!("base64 decode: {e}")))?;

        if pdf_data.is_empty() {
            return Err(ProcessorError::MissingField("PDF data".to_string()));
        }

        let mut hasher = Sha256::new();
        hasher.update(&pdf_data);
        let content_hash = hex::encode(hasher.finalize());

        let title = infer_title(&artifact.subject, &att.filename);

        // Build source URL from metadata or message ID
        let source_url = artifact
            .metadata
            .get("source_url")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("email:{}", artifact.message_id));

        let mut content_parts = vec![format!("Title: {title}")];
        content_parts.push(format!(
            "\n[PDF attachment: {} bytes — text extraction pending Rust PDF library integration]",
            pdf_data.len()
        ));

        let mut metadata = HashMap::new();
        metadata.insert("filename".to_string(), serde_json::json!(att.filename));
        metadata.insert(
            "content_hash_sha256".to_string(),
            serde_json::json!(content_hash),
        );
        metadata.insert(
            "pdf_size_bytes".to_string(),
            serde_json::json!(pdf_data.len()),
        );

        Ok(ProcessorResult {
            title,
            content: content_parts.join("\n"),
            source_url,
            source_type: "pdf-generic".to_string(),
            processor_version: VERSION.to_string(),
            domain_tags: vec!["document".to_string()],
            metadata,
            embedded_artifacts: vec![],
            fetched_at: now,
            processed_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::Attachment;

    #[test]
    fn test_infer_title() {
        assert_eq!(infer_title("My Research Paper", ""), "My Research Paper");
        assert_eq!(infer_title("Fwd: paper", "my_paper.pdf"), "My Paper");
        assert_eq!(infer_title("", ""), "Untitled PDF");
    }

    #[test]
    fn test_can_process() {
        let proc = PdfGenericProcessor::new();
        let artifact = RawArtifact {
            source_type: "pdf_attachment".to_string(),
            subject: "Test".to_string(),
            body: String::new(),
            message_id: "msg-1".to_string(),
            date: String::new(),
            attachments: vec![Attachment {
                filename: "test.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: base64::engine::general_purpose::STANDARD.encode(b"fake pdf"),
            }],
            metadata: HashMap::new(),
        };
        assert!(proc.can_process(&artifact));
    }

    #[test]
    fn test_process() {
        let proc = PdfGenericProcessor::new();
        let artifact = RawArtifact {
            source_type: "pdf_attachment".to_string(),
            subject: "Important Document".to_string(),
            body: String::new(),
            message_id: "msg-1".to_string(),
            date: String::new(),
            attachments: vec![Attachment {
                filename: "report.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: base64::engine::general_purpose::STANDARD.encode(b"fake pdf content"),
            }],
            metadata: HashMap::new(),
        };

        let result = proc.process(&artifact).unwrap();
        assert_eq!(result.title, "Important Document");
        assert_eq!(result.source_type, "pdf-generic");
        assert!(result.domain_tags.contains(&"document".to_string()));
        assert!(result.metadata.contains_key("content_hash_sha256"));
    }
}
