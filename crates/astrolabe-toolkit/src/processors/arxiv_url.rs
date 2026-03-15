//! arXiv URL processor — metadata extraction via arXiv API.
//!
//! Agent: luban

use chrono::Utc;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::processor::{ArtifactProcessor, ProcessorError, ProcessorResult, RawArtifact};

const VERSION: &str = "1.0.0";
const ARXIV_API: &str = "https://export.arxiv.org/api/query";

static ARXIV_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{4}\.\d{4,5})(?:v\d+)?").unwrap());
static ARXIV_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)arxiv\.org/abs/(\d{4}\.\d{4,5})").unwrap());

/// arXiv category to human-readable domain tag mapping.
pub fn category_tag(cat: &str) -> Option<&'static str> {
    match cat {
        "cs.AI" => Some("artificial-intelligence"),
        "cs.LG" | "stat.ML" => Some("machine-learning"),
        "cs.CL" => Some("nlp"),
        "cs.CV" => Some("computer-vision"),
        "cs.CR" => Some("security"),
        "cs.DC" => Some("distributed-computing"),
        "cs.SE" => Some("software-engineering"),
        "math.OC" => Some("optimization"),
        _ => None,
    }
}

/// Extract arXiv ID from text containing arXiv URLs or bare IDs.
pub fn extract_arxiv_id(text: &str) -> Option<String> {
    if let Some(caps) = ARXIV_URL_RE.captures(text) {
        return Some(caps[1].to_string());
    }
    if let Some(caps) = ARXIV_ID_RE.captures(text) {
        return Some(caps[1].to_string());
    }
    None
}

/// Parsed arXiv paper metadata.
#[derive(Debug, Clone)]
pub struct ArxivMetadata {
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub categories: Vec<String>,
    pub published: String,
}

impl ArxivMetadata {
    /// Format authors for display (with "et al." for long lists).
    pub fn authors_display(&self) -> String {
        format_authors(&self.authors)
    }
}

/// Fetch metadata asynchronously via the arXiv API.
pub async fn fetch_arxiv_metadata(
    client: &reqwest::Client,
    arxiv_id: &str,
) -> Result<ArxivMetadata, ProcessorError> {
    let url = format!("{ARXIV_API}?id_list={arxiv_id}&max_results=1");

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))?;

    let xml_data = resp
        .text()
        .await
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))?;

    parse_arxiv_response(&xml_data, arxiv_id)
}

/// Fetch metadata synchronously via the arXiv API (blocking).
pub fn fetch_arxiv_metadata_blocking(arxiv_id: &str) -> Result<ArxivMetadata, ProcessorError> {
    let url = format!("{ARXIV_API}?id_list={arxiv_id}&max_results=1");

    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))?
        .get(&url)
        .send()
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))?;

    let xml_data = resp
        .text()
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))?;

    parse_arxiv_response(&xml_data, arxiv_id)
}

/// Parse arXiv Atom XML response.
fn parse_arxiv_response(xml: &str, arxiv_id: &str) -> Result<ArxivMetadata, ProcessorError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);

    let mut in_entry = false;
    let mut in_author = false;
    let mut current_tag = String::new();
    let mut title = String::new();
    let mut summary = String::new();
    let mut authors = Vec::new();
    let mut categories = Vec::new();
    let mut published = String::new();
    let mut current_author_name = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "entry" => in_entry = true,
                    "author" if in_entry => in_author = true,
                    _ => {}
                }
                if in_entry {
                    current_tag = name;
                }
            }
            Ok(Event::Empty(e)) if in_entry => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "category" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"term" {
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            if !val.is_empty() {
                                categories.push(val);
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(e)) if in_entry => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_author && current_tag == "name" {
                    current_author_name = text.trim().to_string();
                } else {
                    match current_tag.as_str() {
                        "title" => title = text.split_whitespace().collect::<Vec<_>>().join(" "),
                        "summary" => {
                            summary = text.split_whitespace().collect::<Vec<_>>().join(" ")
                        }
                        "published" => published = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "entry" => break,
                    "author" if in_entry => {
                        if !current_author_name.is_empty() {
                            authors.push(std::mem::take(&mut current_author_name));
                        }
                        in_author = false;
                    }
                    _ => {}
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ProcessorError::ParseError(format!(
                    "XML parse error: {e}"
                )))
            }
            _ => {}
        }
    }

    if title.is_empty() || title.contains("Error") {
        return Err(ProcessorError::ExtractionFailed(format!(
            "No valid entry found for arXiv:{arxiv_id}"
        )));
    }

    Ok(ArxivMetadata {
        arxiv_id: arxiv_id.to_string(),
        title,
        authors,
        abstract_text: summary,
        categories,
        published,
    })
}

/// Format author list with "et al." for long lists.
pub fn format_authors(authors: &[String]) -> String {
    if authors.is_empty() {
        return String::new();
    }
    let display: Vec<&str> = authors.iter().take(5).map(|s| s.as_str()).collect();
    let mut s = display.join(", ");
    if authors.len() > 5 {
        s.push_str(&format!(" et al. ({} authors)", authors.len()));
    }
    s
}

/// arXiv URL processor.
pub struct ArxivUrlProcessor;

impl ArxivUrlProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl ArtifactProcessor for ArxivUrlProcessor {
    fn name(&self) -> &str {
        "arxiv-url"
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn accepts(&self) -> &[&str] {
        &["arxiv_url"]
    }

    fn can_process(&self, artifact: &RawArtifact) -> bool {
        let combined = format!("{} {}", artifact.subject, artifact.body);
        extract_arxiv_id(&combined).is_some()
    }

    fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError> {
        let now = Utc::now();
        let combined = format!("{} {}", artifact.subject, artifact.body);

        let arxiv_id = extract_arxiv_id(&combined)
            .ok_or_else(|| ProcessorError::MissingField("arXiv ID".to_string()))?;

        let source_url = format!("https://arxiv.org/abs/{arxiv_id}");

        match fetch_arxiv_metadata_blocking(&arxiv_id) {
            Ok(meta) => {
                let author_str = format_authors(&meta.authors);

                let mut domain_tags = vec!["research".to_string()];
                for cat in &meta.categories {
                    if let Some(tag) = category_tag(cat) {
                        let tag_s = tag.to_string();
                        if !domain_tags.contains(&tag_s) {
                            domain_tags.push(tag_s);
                        }
                    }
                }

                let content = format!(
                    "Title: {}\nAuthors: {}\nAbstract: {}",
                    meta.title, author_str, meta.abstract_text
                );

                let mut metadata = HashMap::new();
                metadata.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
                metadata.insert("authors".to_string(), serde_json::json!(author_str));
                metadata.insert(
                    "categories".to_string(),
                    serde_json::json!(meta.categories),
                );
                metadata.insert("published".to_string(), serde_json::json!(meta.published));

                Ok(ProcessorResult {
                    title: meta.title,
                    content,
                    source_url,
                    source_type: "arxiv-url".to_string(),
                    processor_version: VERSION.to_string(),
                    domain_tags,
                    metadata,
                    embedded_artifacts: vec![],
                    fetched_at: now,
                    processed_at: now,
                })
            }
            Err(_) => {
                // Fallback: minimal result
                let mut metadata = HashMap::new();
                metadata.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
                metadata.insert("api_fetch_failed".to_string(), serde_json::json!(true));

                Ok(ProcessorResult {
                    title: format!("arXiv:{arxiv_id}"),
                    content: format!("arXiv paper {arxiv_id} (metadata fetch failed)"),
                    source_url,
                    source_type: "arxiv-url".to_string(),
                    processor_version: VERSION.to_string(),
                    domain_tags: vec!["research".to_string()],
                    metadata,
                    embedded_artifacts: vec![],
                    fetched_at: now,
                    processed_at: now,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_arxiv_id() {
        assert_eq!(
            extract_arxiv_id("https://arxiv.org/abs/2401.12345"),
            Some("2401.12345".to_string())
        );
        assert_eq!(
            extract_arxiv_id("Check out 2401.12345v2"),
            Some("2401.12345".to_string())
        );
        assert_eq!(extract_arxiv_id("no id here"), None);
    }

    #[test]
    fn test_format_authors() {
        let short = vec!["Alice".to_string(), "Bob".to_string()];
        assert_eq!(format_authors(&short), "Alice, Bob");

        let long: Vec<String> = (1..=8).map(|i| format!("Author{i}")).collect();
        let formatted = format_authors(&long);
        assert!(formatted.contains("et al."));
        assert!(formatted.contains("8 authors"));
    }

    #[test]
    fn test_category_tag() {
        assert_eq!(category_tag("cs.AI"), Some("artificial-intelligence"));
        assert_eq!(category_tag("cs.LG"), Some("machine-learning"));
        assert_eq!(category_tag("unknown.XX"), None);
    }

    #[test]
    fn test_parse_arxiv_response() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <title>Test Paper Title</title>
    <summary>This is the abstract of the test paper.</summary>
    <author><name>Alice</name></author>
    <author><name>Bob</name></author>
    <category term="cs.AI"/>
    <category term="cs.LG"/>
    <published>2024-01-15T00:00:00Z</published>
  </entry>
</feed>"#;

        let meta = parse_arxiv_response(xml, "2401.12345").unwrap();
        assert_eq!(meta.title, "Test Paper Title");
        assert_eq!(meta.authors, vec!["Alice", "Bob"]);
        assert!(meta.abstract_text.contains("abstract"));
        assert_eq!(meta.categories, vec!["cs.AI", "cs.LG"]);
    }

    #[test]
    fn test_processor_can_process() {
        let proc = ArxivUrlProcessor::new();
        let artifact = RawArtifact {
            source_type: "arxiv_url".to_string(),
            subject: "Paper".to_string(),
            body: "https://arxiv.org/abs/2401.12345".to_string(),
            message_id: String::new(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert!(proc.can_process(&artifact));

        let no_arxiv = RawArtifact {
            body: "just some text".to_string(),
            ..artifact
        };
        assert!(!proc.can_process(&no_arxiv));
    }
}
