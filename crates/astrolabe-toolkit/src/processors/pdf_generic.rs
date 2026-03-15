//! Generic PDF processor — text extraction + heuristic metadata inference.
//!
//! Handles non-arXiv PDF attachments. Extracts text, infers title from
//! multiple sources (PDF metadata, email subject, filename, content),
//! and searches for DOI identifiers.
//!
//! Agent: luban

use base64::Engine;
use chrono::Utc;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::processor::{ArtifactProcessor, ProcessorError, ProcessorResult, RawArtifact};

const VERSION: &str = "1.0.0";
const MAX_CONTENT_LEN: usize = 30_000;

/// DOI pattern: 10.XXXX/... (standard DOI format)
static DOI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"10\.\d{4,9}/[-._;()/:A-Za-z0-9]+").unwrap());

/// Extract text from PDF bytes.
///
/// Placeholder — same as arxiv_pdf. Production integration pending.
fn extract_pdf_text(pdf_bytes: &[u8]) -> Result<String, String> {
    if pdf_bytes.len() < 5 || &pdf_bytes[..5] != b"%PDF-" {
        return Err("not a valid PDF".to_string());
    }
    Ok(format!(
        "[PDF detected: {} bytes — text extraction pending native PDF library integration]",
        pdf_bytes.len()
    ))
}

/// Heuristic title inference from multiple sources.
///
/// Priority:
/// 1. Email subject (if > 5 chars, not "Fwd:")
/// 2. Filename stem, titlecased
/// 3. First substantial line from extracted text
/// 4. Fallback: "Untitled PDF"
fn infer_title(subject: &str, filename: &str, pdf_text: &str) -> String {
    // Try email subject
    let trimmed_subject = subject.trim();
    if trimmed_subject.len() > 5
        && !trimmed_subject.starts_with("Fwd:")
        && !trimmed_subject.starts_with("FW:")
    {
        return trimmed_subject.to_string();
    }

    // Try filename stem
    if !filename.is_empty() {
        let stem = filename
            .trim_end_matches(".pdf")
            .trim_end_matches(".PDF")
            .replace(['_', '-'], " ");
        if stem.len() > 3 {
            let titled: String = stem
                .split_whitespace()
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        Some(c) => {
                            c.to_uppercase().to_string() + &chars.as_str().to_lowercase()
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            return titled;
        }
    }

    // Try first substantial line from text
    for line in pdf_text.lines() {
        let line = line.trim();
        if line.len() >= 10
            && line.len() <= 120
            && !line.starts_with("http")
            && !line.starts_with("[PDF")
        {
            return line.to_string();
        }
    }

    "Untitled PDF".to_string()
}

/// Search for DOI in extracted text (first 5000 chars).
fn find_doi(text: &str) -> Option<String> {
    let search_window = if text.len() > 5000 {
        &text[..5000]
    } else {
        text
    };
    DOI_RE.find(search_window).map(|m| m.as_str().to_string())
}

/// Decode attachment data from base64 or raw.
fn decode_attachment_data(data: &str) -> Result<Vec<u8>, ProcessorError> {
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
        return Ok(bytes);
    }
    Ok(data.as_bytes().to_vec())
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

        let pdf_attachment = artifact
            .attachments
            .iter()
            .find(|a| a.content_type.starts_with("application/pdf"))
            .ok_or_else(|| ProcessorError::MissingField("PDF attachment".to_string()))?;

        let pdf_bytes = decode_attachment_data(&pdf_attachment.data)?;
        let pdf_hash = hex::encode(Sha256::digest(&pdf_bytes));

        let (pdf_text, text_extracted) = match extract_pdf_text(&pdf_bytes) {
            Ok(text) => (text, true),
            Err(_) => (String::new(), false),
        };

        let title = infer_title(&artifact.subject, &pdf_attachment.filename, &pdf_text);

        // Search for DOI
        let doi = find_doi(&pdf_text);
        let source_url = if let Some(ref doi_str) = doi {
            format!("https://doi.org/{doi_str}")
        } else if let Some(url) = artifact.metadata.get("source_url").and_then(|v| v.as_str()) {
            url.to_string()
        } else if !artifact.message_id.is_empty() {
            format!("email:{}", artifact.message_id)
        } else {
            "unknown".to_string()
        };

        // Build content
        let mut content_parts = vec![format!("Title: {title}")];
        if let Some(ref doi_str) = doi {
            content_parts.push(format!("DOI: {doi_str}"));
        }
        if text_extracted && !pdf_text.is_empty() {
            content_parts.push(String::new());
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
        metadata.insert(
            "filename".to_string(),
            serde_json::json!(pdf_attachment.filename),
        );
        metadata.insert(
            "content_hash_sha256".to_string(),
            serde_json::json!(pdf_hash),
        );
        metadata.insert(
            "pdf_text_extracted".to_string(),
            serde_json::json!(text_extracted),
        );
        metadata.insert(
            "pdf_text_length".to_string(),
            serde_json::json!(pdf_text.len()),
        );
        if let Some(doi_str) = doi {
            metadata.insert("doi".to_string(), serde_json::json!(doi_str));
        }

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

    fn make_pdf_bytes() -> Vec<u8> {
        b"%PDF-1.4 fake pdf content for testing".to_vec()
    }

    #[test]
    fn infer_title_from_subject() {
        assert_eq!(
            infer_title("My Research Paper", "paper.pdf", ""),
            "My Research Paper"
        );
    }

    #[test]
    fn infer_title_skips_fwd() {
        let title = infer_title("Fwd: something", "deep_learning_survey.pdf", "");
        assert_eq!(title, "Deep Learning Survey");
    }

    #[test]
    fn infer_title_from_filename() {
        let title = infer_title("", "attention_is_all_you_need.pdf", "");
        assert_eq!(title, "Attention Is All You Need");
    }

    #[test]
    fn infer_title_from_text() {
        let text = "Short\nThis is a long enough title line from the PDF content\nMore content";
        let title = infer_title("", "", text);
        assert_eq!(
            title,
            "This is a long enough title line from the PDF content"
        );
    }

    #[test]
    fn infer_title_fallback() {
        assert_eq!(infer_title("", "", ""), "Untitled PDF");
    }

    #[test]
    fn find_doi_present() {
        let text = "See reference 10.1234/some-paper.v2 for details";
        assert_eq!(find_doi(text), Some("10.1234/some-paper.v2".to_string()));
    }

    #[test]
    fn find_doi_absent() {
        assert_eq!(find_doi("no doi here"), None);
    }

    #[test]
    fn can_process_with_pdf() {
        let processor = PdfGenericProcessor::new();
        let artifact = RawArtifact {
            source_type: "pdf_attachment".to_string(),
            subject: "Report Q4".to_string(),
            body: String::new(),
            message_id: "test-1".to_string(),
            date: String::new(),
            attachments: vec![Attachment {
                filename: "report.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: "data".to_string(),
            }],
            metadata: HashMap::new(),
        };
        assert!(processor.can_process(&artifact));
    }

    #[test]
    fn cannot_process_without_pdf() {
        let processor = PdfGenericProcessor::new();
        let artifact = RawArtifact {
            source_type: "pdf_attachment".to_string(),
            subject: "Report".to_string(),
            body: String::new(),
            message_id: "test-2".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert!(!processor.can_process(&artifact));
    }

    #[test]
    fn process_basic_pdf() {
        let processor = PdfGenericProcessor::new();
        let pdf_bytes = make_pdf_bytes();
        let artifact = RawArtifact {
            source_type: "pdf_attachment".to_string(),
            subject: "Quarterly Report 2026".to_string(),
            body: String::new(),
            message_id: "test-3".to_string(),
            date: String::new(),
            attachments: vec![Attachment {
                filename: "report_q4.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                data: base64::engine::general_purpose::STANDARD.encode(&pdf_bytes),
            }],
            metadata: HashMap::new(),
        };

        let result = processor.process(&artifact).unwrap();
        assert_eq!(result.source_type, "pdf-generic");
        assert_eq!(result.title, "Quarterly Report 2026");
        assert!(result.domain_tags.contains(&"document".to_string()));
        assert!(result.metadata.contains_key("content_hash_sha256"));
        assert!(result.is_valid());
    }

    #[test]
    fn processor_metadata() {
        let processor = PdfGenericProcessor::new();
        assert_eq!(processor.name(), "pdf-generic");
        assert_eq!(processor.version(), "1.0.0");
        assert_eq!(processor.accepts(), &["pdf_attachment"]);
    }
}
