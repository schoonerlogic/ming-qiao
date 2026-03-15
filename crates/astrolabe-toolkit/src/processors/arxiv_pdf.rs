//! arXiv PDF processor — text extraction from arXiv PDF attachments.
//!
//! Handles artifacts with arXiv IDs in the subject and PDF attachments.
//! Enriches with arXiv API metadata via blocking HTTP call.
//!
//! Agent: luban

use base64::Engine;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::processor::{ArtifactProcessor, ProcessorError, ProcessorResult, RawArtifact};
use super::arxiv_url::{extract_arxiv_id, fetch_arxiv_metadata_blocking, format_authors, category_tag};

const VERSION: &str = "1.0.0";

/// arXiv PDF processor.
pub struct ArxivPdfProcessor;

impl ArxivPdfProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl ArtifactProcessor for ArxivPdfProcessor {
    fn name(&self) -> &str {
        "arxiv-pdf"
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn accepts(&self) -> &[&str] {
        &["arxiv_pdf"]
    }

    fn can_process(&self, artifact: &RawArtifact) -> bool {
        let has_arxiv_id = extract_arxiv_id(&artifact.subject).is_some();
        let has_pdf = artifact
            .attachments
            .iter()
            .any(|a| a.content_type.starts_with("application/pdf"));
        has_arxiv_id && has_pdf
    }

    fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError> {
        let now = Utc::now();

        let arxiv_id = extract_arxiv_id(&artifact.subject)
            .ok_or_else(|| ProcessorError::MissingField("arXiv ID in subject".to_string()))?;

        let source_url = format!("https://arxiv.org/abs/{arxiv_id}");

        // Decode PDF attachment for hashing
        let mut content_hash = String::new();
        let mut pdf_size = 0usize;
        for att in &artifact.attachments {
            if att.content_type.starts_with("application/pdf") {
                if let Ok(pdf_data) = base64::engine::general_purpose::STANDARD.decode(&att.data) {
                    let mut hasher = Sha256::new();
                    hasher.update(&pdf_data);
                    content_hash = hex::encode(hasher.finalize());
                    pdf_size = pdf_data.len();
                }
                break;
            }
        }

        // Fetch arXiv API metadata for enrichment
        let meta = fetch_arxiv_metadata_blocking(&arxiv_id).ok();

        let (title, author_str, domain_tags, mut metadata) = if let Some(ref m) = meta {
            let author_str = format_authors(&m.authors);
            let mut tags = vec!["research".to_string()];
            for cat in &m.categories {
                if let Some(tag) = category_tag(cat) {
                    let t = tag.to_string();
                    if !tags.contains(&t) {
                        tags.push(t);
                    }
                }
            }
            let mut md = HashMap::new();
            md.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
            md.insert("authors".to_string(), serde_json::json!(author_str));
            md.insert("categories".to_string(), serde_json::json!(m.categories));
            md.insert("published".to_string(), serde_json::json!(m.published));
            (m.title.clone(), author_str, tags, md)
        } else {
            let mut md = HashMap::new();
            md.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
            md.insert("api_fetch_failed".to_string(), serde_json::json!(true));
            (
                format!("arXiv:{arxiv_id}"),
                String::new(),
                vec!["research".to_string()],
                md,
            )
        };

        metadata.insert("content_hash_sha256".to_string(), serde_json::json!(content_hash));
        metadata.insert("pdf_size_bytes".to_string(), serde_json::json!(pdf_size));

        // Build content from API metadata
        let mut content_parts = vec![format!("Title: {title}")];
        if !author_str.is_empty() {
            content_parts.push(format!("Authors: {author_str}"));
        }
        if let Some(ref m) = meta {
            if !m.abstract_text.is_empty() {
                content_parts.push(format!("Abstract: {}", m.abstract_text));
            }
        }
        content_parts.push(format!(
            "\n[PDF attachment: {pdf_size} bytes — text extraction pending Rust PDF library integration]"
        ));

        Ok(ProcessorResult {
            title,
            content: content_parts.join("\n"),
            source_url,
            source_type: "arxiv-pdf".to_string(),
            processor_version: VERSION.to_string(),
            domain_tags,
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
    fn test_can_process() {
        let proc = ArxivPdfProcessor::new();

        let artifact = RawArtifact {
            source_type: "arxiv_pdf".to_string(),
            subject: "2401.12345".to_string(),
            body: String::new(),
            message_id: String::new(),
            date: String::new(),
            attachments: vec![Attachment {
                filename: "paper.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: base64::engine::general_purpose::STANDARD.encode(b"fake pdf"),
            }],
            metadata: HashMap::new(),
        };
        assert!(proc.can_process(&artifact));

        let no_pdf = RawArtifact {
            attachments: vec![],
            ..artifact.clone()
        };
        assert!(!proc.can_process(&no_pdf));

        let no_id = RawArtifact {
            subject: "Random paper".to_string(),
            ..artifact
        };
        assert!(!proc.can_process(&no_id));
    }
}
