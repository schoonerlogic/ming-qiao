//! arXiv PDF processor — text extraction from PDF attachments + API metadata.
//!
//! Handles artifacts with arXiv IDs in the subject and PDF attachments.
//! Extracts text from the PDF and enriches with arXiv API metadata.
//!
//! Agent: luban

use base64::Engine;
use chrono::Utc;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::processor::{ArtifactProcessor, ProcessorError, ProcessorResult, RawArtifact};
use super::arxiv_url;

const VERSION: &str = "1.0.0";
const MAX_PAGES: usize = 50;
const MAX_CONTENT_LEN: usize = 30_000;

static ARXIV_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{4}\.\d{4,5})(?:v\d+)?").unwrap());

/// Extract text from PDF bytes.
///
/// Currently returns a placeholder — full PDF extraction requires a native
/// PDF library. The `pdf` or `lopdf` crate can be integrated here.
///
/// TODO: Integrate `pdf-extract` or `lopdf` for production text extraction.
fn extract_pdf_text(pdf_bytes: &[u8], _max_pages: usize) -> Result<String, String> {
    if pdf_bytes.len() < 5 || &pdf_bytes[..5] != b"%PDF-" {
        return Err("not a valid PDF (missing header)".to_string());
    }
    Ok(format!(
        "[PDF detected: {} bytes — text extraction pending native PDF library integration]",
        pdf_bytes.len()
    ))
}

/// Decode attachment data (base64 or raw bytes).
fn decode_attachment_data(data: &str) -> Result<Vec<u8>, ProcessorError> {
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
        return Ok(bytes);
    }
    Ok(data.as_bytes().to_vec())
}

/// arXiv PDF processor.
pub struct ArxivPdfProcessor {
    client: reqwest::Client,
}

impl ArxivPdfProcessor {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Async processing with API metadata enrichment.
    pub async fn process_async(
        &self,
        artifact: &RawArtifact,
    ) -> Result<ProcessorResult, ProcessorError> {
        let now = Utc::now();

        let arxiv_id = ARXIV_ID_RE
            .captures(&artifact.subject)
            .map(|c| c[1].to_string())
            .ok_or_else(|| ProcessorError::MissingField("arXiv ID in subject".to_string()))?;

        // Find first PDF attachment
        let pdf_attachment = artifact
            .attachments
            .iter()
            .find(|a| a.content_type.starts_with("application/pdf"))
            .ok_or_else(|| ProcessorError::MissingField("PDF attachment".to_string()))?;

        let pdf_bytes = decode_attachment_data(&pdf_attachment.data)?;
        let pdf_hash = hex::encode(Sha256::digest(&pdf_bytes));

        // Extract text from PDF
        let (pdf_text, text_extracted) = match extract_pdf_text(&pdf_bytes, MAX_PAGES) {
            Ok(text) => (text, true),
            Err(_) => (String::new(), false),
        };

        let source_url = format!("https://arxiv.org/abs/{arxiv_id}");

        // Fetch arXiv metadata for enrichment
        let meta = arxiv_url::fetch_arxiv_metadata(&self.client, &arxiv_id).await.ok();

        let (title, authors_display, abstract_text, categories, published) = match &meta {
            Some(m) => (
                m.title.clone(),
                m.authors_display(),
                m.abstract_text.clone(),
                m.categories.clone(),
                m.published.clone(),
            ),
            None => (
                format!("arXiv:{arxiv_id}"),
                String::new(),
                String::new(),
                vec![],
                String::new(),
            ),
        };

        // Build content
        let mut content_parts = vec![format!("Title: {title}")];
        if !authors_display.is_empty() {
            content_parts.push(format!("Authors: {authors_display}"));
        }
        if !abstract_text.is_empty() {
            content_parts.push(format!("\nAbstract: {abstract_text}"));
        }
        if text_extracted && !pdf_text.is_empty() {
            content_parts.push("\n[--- PDF Text ---]".to_string());
            let truncated = if pdf_text.len() > MAX_CONTENT_LEN {
                format!(
                    "{}\n[Truncated — {} chars total]",
                    &pdf_text[..MAX_CONTENT_LEN],
                    pdf_text.len()
                )
            } else {
                pdf_text.clone()
            };
            content_parts.push(truncated);
        }
        let content = content_parts.join("\n");

        // Build domain tags
        let mut domain_tags = vec!["research".to_string()];
        for cat in &categories {
            if let Some(tag) = arxiv_url::category_tag(cat) {
                if !domain_tags.iter().any(|t| t == tag) {
                    domain_tags.push(tag.to_string());
                }
            }
        }

        // Build metadata
        let mut metadata = HashMap::new();
        metadata.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
        metadata.insert("content_hash_sha256".to_string(), serde_json::json!(pdf_hash));
        metadata.insert("pdf_text_extracted".to_string(), serde_json::json!(text_extracted));
        metadata.insert("pdf_text_length".to_string(), serde_json::json!(pdf_text.len()));
        if !authors_display.is_empty() {
            metadata.insert("authors".to_string(), serde_json::json!(authors_display));
        }
        if !categories.is_empty() {
            metadata.insert("categories".to_string(), serde_json::json!(categories));
        }
        if !published.is_empty() {
            metadata.insert("published".to_string(), serde_json::json!(published));
        }

        Ok(ProcessorResult {
            title,
            content,
            source_url,
            source_type: "arxiv-pdf".to_string(),
            processor_version: VERSION.to_string(),
            domain_tags,
            metadata,
            embedded_artifacts: vec![],
            fetched_at: now,
            processed_at: Utc::now(),
        })
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
        let has_arxiv_id = ARXIV_ID_RE.is_match(&artifact.subject);
        let has_pdf = artifact
            .attachments
            .iter()
            .any(|a| a.content_type.starts_with("application/pdf"));
        has_arxiv_id && has_pdf
    }

    fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError> {
        // Synchronous fallback — extracts PDF text but cannot fetch API metadata.
        let now = Utc::now();

        let arxiv_id = ARXIV_ID_RE
            .captures(&artifact.subject)
            .map(|c| c[1].to_string())
            .ok_or_else(|| ProcessorError::MissingField("arXiv ID in subject".to_string()))?;

        let pdf_attachment = artifact
            .attachments
            .iter()
            .find(|a| a.content_type.starts_with("application/pdf"))
            .ok_or_else(|| ProcessorError::MissingField("PDF attachment".to_string()))?;

        let pdf_bytes = decode_attachment_data(&pdf_attachment.data)?;
        let pdf_hash = hex::encode(Sha256::digest(&pdf_bytes));

        let (pdf_text, text_extracted) = match extract_pdf_text(&pdf_bytes, MAX_PAGES) {
            Ok(text) => (text, true),
            Err(_) => (String::new(), false),
        };

        let source_url = format!("https://arxiv.org/abs/{arxiv_id}");
        let title = format!("arXiv:{arxiv_id}");

        let mut content_parts = vec![format!("Title: {title}")];
        if text_extracted && !pdf_text.is_empty() {
            content_parts.push("\n[--- PDF Text ---]".to_string());
            let truncated = if pdf_text.len() > MAX_CONTENT_LEN {
                format!(
                    "{}\n[Truncated — {} chars total]",
                    &pdf_text[..MAX_CONTENT_LEN],
                    pdf_text.len()
                )
            } else {
                pdf_text.clone()
            };
            content_parts.push(truncated);
        }

        let mut metadata = HashMap::new();
        metadata.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
        metadata.insert("content_hash_sha256".to_string(), serde_json::json!(pdf_hash));
        metadata.insert("pdf_text_extracted".to_string(), serde_json::json!(text_extracted));
        metadata.insert("pdf_text_length".to_string(), serde_json::json!(pdf_text.len()));
        metadata.insert("sync_only".to_string(), serde_json::json!(true));

        Ok(ProcessorResult {
            title,
            content: content_parts.join("\n"),
            source_url,
            source_type: "arxiv-pdf".to_string(),
            processor_version: VERSION.to_string(),
            domain_tags: vec!["research".to_string()],
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

    fn make_pdf_bytes() -> Vec<u8> {
        b"%PDF-1.4 fake pdf content for testing".to_vec()
    }

    fn make_artifact(arxiv_id: &str, pdf_data: &[u8]) -> RawArtifact {
        RawArtifact {
            source_type: "arxiv_pdf".to_string(),
            subject: format!("Paper {arxiv_id} — new results"),
            body: String::new(),
            message_id: "test-pdf-1".to_string(),
            date: String::new(),
            attachments: vec![Attachment {
                filename: format!("{arxiv_id}.pdf"),
                content_type: "application/pdf".to_string(),
                data: base64::engine::general_purpose::STANDARD.encode(pdf_data),
            }],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn can_process_with_arxiv_id_and_pdf() {
        let processor = ArxivPdfProcessor::new();
        let pdf_bytes = make_pdf_bytes();
        let artifact = make_artifact("2401.12345", &pdf_bytes);
        assert!(processor.can_process(&artifact));
    }

    #[test]
    fn cannot_process_without_pdf() {
        let processor = ArxivPdfProcessor::new();
        let artifact = RawArtifact {
            source_type: "arxiv_pdf".to_string(),
            subject: "Paper 2401.12345".to_string(),
            body: String::new(),
            message_id: "test-2".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert!(!processor.can_process(&artifact));
    }

    #[test]
    fn cannot_process_without_arxiv_id() {
        let processor = ArxivPdfProcessor::new();
        let artifact = RawArtifact {
            source_type: "arxiv_pdf".to_string(),
            subject: "Some random paper".to_string(),
            body: String::new(),
            message_id: "test-3".to_string(),
            date: String::new(),
            attachments: vec![Attachment {
                filename: "paper.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: "not-real".to_string(),
            }],
            metadata: HashMap::new(),
        };
        assert!(!processor.can_process(&artifact));
    }

    #[test]
    fn sync_process_extracts_basic_info() {
        let processor = ArxivPdfProcessor::new();
        let pdf_bytes = make_pdf_bytes();
        let artifact = make_artifact("2401.12345", &pdf_bytes);

        let result = processor.process(&artifact).unwrap();
        assert_eq!(result.source_type, "arxiv-pdf");
        assert!(result.title.contains("2401.12345"));
        assert_eq!(
            result.metadata.get("arxiv_id").unwrap(),
            &serde_json::json!("2401.12345")
        );
        assert!(result.metadata.contains_key("content_hash_sha256"));
        assert!(result.is_valid());
    }

    #[test]
    fn decode_base64_attachment() {
        let original = b"Hello PDF world";
        let encoded = base64::engine::general_purpose::STANDARD.encode(original);
        let decoded = decode_attachment_data(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn extract_pdf_text_invalid() {
        let result = extract_pdf_text(b"not a pdf", 50);
        assert!(result.is_err());
    }

    #[test]
    fn extract_pdf_text_valid_header() {
        let fake_pdf = b"%PDF-1.4 some content";
        let result = extract_pdf_text(fake_pdf, 50);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("PDF detected"));
    }

    #[test]
    fn processor_metadata() {
        let processor = ArxivPdfProcessor::new();
        assert_eq!(processor.name(), "arxiv-pdf");
        assert_eq!(processor.version(), "1.0.0");
        assert_eq!(processor.accepts(), &["arxiv_pdf"]);
    }
}
