//! Web article processor — content extraction from blog posts and articles.
//!
//! Handles Medium articles, Hugging Face blog posts, and general web URLs.
//! Uses basic HTML tag stripping for content extraction.
//!
//! Agent: luban

use chrono::Utc;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::processor::{ArtifactProcessor, ProcessorError, ProcessorResult, RawArtifact};

const VERSION: &str = "1.0.0";
const MAX_CONTENT_LEN: usize = 30_000;
const FETCH_TIMEOUT_SECS: u64 = 30;

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"']+"#).unwrap());
static HTML_TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<title[^>]*>([^<]+)</title>").unwrap());

/// Domain tag mappings.
fn domain_tags(url: &str) -> Vec<String> {
    let mut tags = vec!["web-article".to_string()];
    if url.contains("medium.com") {
        tags.extend(["blog".to_string(), "tech-writing".to_string()]);
    } else if url.contains("huggingface.co") {
        tags.extend([
            "blog".to_string(),
            "machine-learning".to_string(),
            "huggingface".to_string(),
        ]);
    } else if url.contains("substack.com") {
        tags.push("newsletter".to_string());
    }
    tags
}

/// Extract the primary URL from artifact body or metadata.
fn extract_url(artifact: &RawArtifact) -> Option<String> {
    if let Some(url) = artifact.metadata.get("source_url").and_then(|v| v.as_str()) {
        return Some(url.to_string());
    }

    for mat in URL_RE.find_iter(&artifact.body) {
        let url = mat.as_str();
        if ["x.com/", "twitter.com/", ".png", ".jpg", ".gif"]
            .iter()
            .any(|skip| url.contains(skip))
        {
            continue;
        }
        return Some(url.trim_end_matches(['.', ',', ';', ')']).to_string());
    }

    None
}

/// Fetch a web page.
fn fetch_page(url: &str) -> Result<String, ProcessorError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent("Mozilla/5.0 (compatible; AstrolabeBot/1.0; research)")
        .build()
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))?;

    let resp = client
        .get(url)
        .header("Accept", "text/html,application/xhtml+xml")
        .send()
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))?;

    resp.text()
        .map_err(|e| ProcessorError::NetworkError(e.to_string()))
}

/// Extract content from HTML using basic tag stripping.
fn extract_content(html: &str) -> (String, String) {
    // Extract title from <title> tag
    let title = TITLE_RE
        .captures(html)
        .map(|c| c[1].trim().to_string())
        .unwrap_or_default();

    // Strip HTML tags
    let text = HTML_TAG_RE.replace_all(html, "");
    let text = WHITESPACE_RE.replace_all(&text, "\n\n");
    let text: String = text.chars().take(MAX_CONTENT_LEN).collect();

    (title, text.trim().to_string())
}

/// Web article processor.
pub struct WebArticleProcessor;

impl WebArticleProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl ArtifactProcessor for WebArticleProcessor {
    fn name(&self) -> &str {
        "web-article"
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn accepts(&self) -> &[&str] {
        &["medium_article", "huggingface_blog", "web_url"]
    }

    fn can_process(&self, artifact: &RawArtifact) -> bool {
        extract_url(artifact).is_some()
    }

    fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError> {
        let now = Utc::now();

        let url = extract_url(artifact)
            .ok_or_else(|| ProcessorError::MissingField("URL".to_string()))?;

        let tags = domain_tags(&url);

        match fetch_page(&url) {
            Ok(html) => {
                let mut hasher = Sha256::new();
                hasher.update(html.as_bytes());
                let content_hash = hex::encode(hasher.finalize());

                let (html_title, text) = extract_content(&html);
                let title = if !html_title.is_empty() {
                    html_title
                } else if !artifact.subject.is_empty() {
                    artifact.subject.clone()
                } else {
                    format!("Web article: {url}")
                };

                let mut content_parts = vec![
                    format!("Title: {title}"),
                    format!("URL: {url}"),
                ];

                if !text.is_empty() {
                    content_parts.push(format!("\n{text}"));
                } else {
                    content_parts.push("\n[Content extraction failed]".to_string());
                }

                let mut metadata = HashMap::new();
                metadata.insert(
                    "content_hash_sha256".to_string(),
                    serde_json::json!(content_hash),
                );
                metadata.insert(
                    "text_extracted".to_string(),
                    serde_json::json!(!text.is_empty()),
                );
                metadata.insert(
                    "text_length".to_string(),
                    serde_json::json!(text.len()),
                );

                Ok(ProcessorResult {
                    title,
                    content: content_parts.join("\n"),
                    source_url: url,
                    source_type: "web-article".to_string(),
                    processor_version: VERSION.to_string(),
                    domain_tags: tags,
                    metadata,
                    embedded_artifacts: vec![],
                    fetched_at: now,
                    processed_at: now,
                })
            }
            Err(_) => {
                // Fallback: return what we have
                let title = if !artifact.subject.is_empty() {
                    artifact.subject.clone()
                } else {
                    format!("Web article: {url}")
                };

                let mut metadata = HashMap::new();
                metadata.insert("fetch_failed".to_string(), serde_json::json!(true));

                Ok(ProcessorResult {
                    title,
                    content: format!(
                        "URL: {url}\n\n[Fetch failed — content unavailable]\n\n{}",
                        artifact.body
                    ),
                    source_url: url,
                    source_type: "web-article".to_string(),
                    processor_version: VERSION.to_string(),
                    domain_tags: tags,
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
    fn test_extract_url() {
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "Blog post".to_string(),
            body: "Check out https://example.com/article today".to_string(),
            message_id: String::new(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert_eq!(
            extract_url(&artifact),
            Some("https://example.com/article".to_string())
        );
    }

    #[test]
    fn test_extract_url_from_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "source_url".to_string(),
            serde_json::json!("https://meta.example.com"),
        );
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: String::new(),
            body: String::new(),
            message_id: String::new(),
            date: String::new(),
            attachments: vec![],
            metadata,
        };
        assert_eq!(
            extract_url(&artifact),
            Some("https://meta.example.com".to_string())
        );
    }

    #[test]
    fn test_extract_content() {
        let html = "<html><head><title>My Article</title></head><body><p>Hello world</p></body></html>";
        let (title, text) = extract_content(html);
        assert_eq!(title, "My Article");
        assert!(text.contains("Hello world"));
        assert!(!text.contains("<p>"));
    }

    #[test]
    fn test_domain_tags() {
        let tags = domain_tags("https://medium.com/some-article");
        assert!(tags.contains(&"blog".to_string()));
        assert!(tags.contains(&"tech-writing".to_string()));

        let tags = domain_tags("https://example.com/page");
        assert_eq!(tags, vec!["web-article".to_string()]);
    }
}
