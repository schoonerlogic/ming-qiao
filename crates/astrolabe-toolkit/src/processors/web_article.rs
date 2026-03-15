//! Web article processor — content extraction from blog posts and articles.
//!
//! Fetches web pages and extracts readable content. Uses a simple HTML
//! tag-stripping approach as a baseline, with extensible hooks for
//! more sophisticated extraction (e.g., readability algorithms).
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
const USER_AGENT: &str = "Mozilla/5.0 (compatible; AstrolabeBot/1.0; research)";

/// URL pattern for extracting URLs from email bodies.
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"']+"#).unwrap());

/// HTML tag pattern for basic stripping.
static HTML_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

/// HTML title tag extraction.
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());

/// Consecutive newlines for cleanup.
static MULTI_NEWLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// Skip URLs for images and social media.
const SKIP_PATTERNS: &[&str] = &[
    "x.com/",
    "twitter.com/",
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".mp4",
];

/// Domain-specific tag mapping.
fn domain_tags(url: &str) -> Vec<String> {
    let mut tags = vec!["web-article".to_string()];
    let lower = url.to_lowercase();

    if lower.contains("medium.com") {
        tags.extend(["blog".to_string(), "tech-writing".to_string()]);
    } else if lower.contains("huggingface.co/blog") {
        tags.extend([
            "blog".to_string(),
            "machine-learning".to_string(),
            "huggingface".to_string(),
        ]);
    } else if lower.contains("substack.com") {
        tags.push("newsletter".to_string());
    }

    tags
}

/// Extract the first usable URL from an artifact.
fn extract_url(artifact: &RawArtifact) -> Option<String> {
    // Check metadata first
    if let Some(url) = artifact
        .metadata
        .get("source_url")
        .and_then(|v| v.as_str())
    {
        if !url.is_empty() {
            return Some(url.to_string());
        }
    }

    // Scan body for URLs
    for m in URL_RE.find_iter(&artifact.body) {
        let url = m.as_str();
        let lower = url.to_lowercase();
        if SKIP_PATTERNS.iter().any(|p| lower.contains(p)) {
            continue;
        }
        // Strip trailing punctuation
        let url = url.trim_end_matches(|c: char| matches!(c, '.' | ',' | ')' | ']' | ';'));
        return Some(url.to_string());
    }

    None
}

/// Fetch a web page via HTTP GET.
pub async fn fetch_page(
    client: &reqwest::Client,
    url: &str,
) -> Result<String, ProcessorError> {
    let resp = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| ProcessorError::NetworkError(format!("fetch failed for {url}: {e}")))?;

    resp.text().await.map_err(|e| {
        ProcessorError::NetworkError(format!("failed to read response from {url}: {e}"))
    })
}

/// Extract readable content from HTML using basic tag stripping.
///
/// Returns (text, title, author).
fn extract_content(html: &str) -> (String, Option<String>, Option<String>) {
    // Extract title
    let title = TITLE_RE.captures(html).map(|c| {
        HTML_TAG_RE
            .replace_all(&c[1], "")
            .trim()
            .to_string()
    });

    // Strip HTML tags
    let text = HTML_TAG_RE.replace_all(html, "\n").to_string();

    // Decode common HTML entities
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    // Collapse excessive newlines
    let text = MULTI_NEWLINE_RE
        .replace_all(&text, "\n\n")
        .trim()
        .to_string();

    // Author extraction placeholder
    let author = None;

    (text, title, author)
}

/// Web article processor.
pub struct WebArticleProcessor {
    client: reqwest::Client,
}

impl WebArticleProcessor {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Async processing with web page fetch.
    pub async fn process_async(
        &self,
        artifact: &RawArtifact,
    ) -> Result<ProcessorResult, ProcessorError> {
        let now = Utc::now();

        let url = extract_url(artifact).ok_or_else(|| {
            ProcessorError::MissingField("no URL found in artifact".to_string())
        })?;

        let tags = domain_tags(&url);

        match fetch_page(&self.client, &url).await {
            Ok(html) => {
                let html_hash = hex::encode(Sha256::digest(html.as_bytes()));
                let (text, html_title, author) = extract_content(&html);

                let title = html_title
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| artifact.subject.clone());

                // Truncate content
                let truncated = if text.len() > MAX_CONTENT_LEN {
                    format!(
                        "{}\n[Truncated — {} chars total]",
                        &text[..MAX_CONTENT_LEN],
                        text.len()
                    )
                } else {
                    text.clone()
                };

                let mut content_parts = vec![format!("Title: {title}")];
                if let Some(ref auth) = author {
                    content_parts.push(format!("Author: {auth}"));
                }
                content_parts.push(format!("URL: {url}"));
                content_parts.push(String::new());
                content_parts.push(truncated);

                let mut metadata = HashMap::new();
                metadata.insert(
                    "content_hash_sha256".to_string(),
                    serde_json::json!(html_hash),
                );
                metadata.insert(
                    "text_extracted".to_string(),
                    serde_json::json!(!text.is_empty()),
                );
                metadata.insert(
                    "text_length".to_string(),
                    serde_json::json!(text.len()),
                );
                if let Some(auth) = author {
                    metadata.insert("authors".to_string(), serde_json::json!(auth));
                }

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
                    processed_at: Utc::now(),
                })
            }
            Err(_) => {
                // Fallback: minimal result when fetch fails
                let mut metadata = HashMap::new();
                metadata.insert("fetch_failed".to_string(), serde_json::json!(true));

                Ok(ProcessorResult {
                    title: artifact.subject.clone(),
                    content: format!("Web article at {url} — fetch failed"),
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
        // Synchronous fallback — cannot fetch the page.
        let now = Utc::now();

        let url = extract_url(artifact).ok_or_else(|| {
            ProcessorError::MissingField("no URL found in artifact".to_string())
        })?;

        let tags = domain_tags(&url);

        let mut metadata = HashMap::new();
        metadata.insert("sync_only".to_string(), serde_json::json!(true));

        Ok(ProcessorResult {
            title: artifact.subject.clone(),
            content: format!("Web article at {url} (use process_async for full content)"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_url_from_body() {
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "Interesting blog".to_string(),
            body: "Check this out: https://medium.com/@user/great-post-abc123".to_string(),
            message_id: "test-1".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert_eq!(
            extract_url(&artifact),
            Some("https://medium.com/@user/great-post-abc123".to_string())
        );
    }

    #[test]
    fn extract_url_from_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "source_url".to_string(),
            serde_json::json!("https://example.com/article"),
        );
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "Article".to_string(),
            body: String::new(),
            message_id: "test-2".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata,
        };
        assert_eq!(
            extract_url(&artifact),
            Some("https://example.com/article".to_string())
        );
    }

    #[test]
    fn extract_url_skips_twitter() {
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "Tweet".to_string(),
            body: "https://x.com/user/status/123 and https://example.com/real-article"
                .to_string(),
            message_id: "test-3".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert_eq!(
            extract_url(&artifact),
            Some("https://example.com/real-article".to_string())
        );
    }

    #[test]
    fn extract_url_strips_trailing_punctuation() {
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "Link".to_string(),
            body: "See https://example.com/page.".to_string(),
            message_id: "test-4".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert_eq!(
            extract_url(&artifact),
            Some("https://example.com/page".to_string())
        );
    }

    #[test]
    fn extract_url_none() {
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "No link".to_string(),
            body: "Nothing here".to_string(),
            message_id: "test-5".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert_eq!(extract_url(&artifact), None);
    }

    #[test]
    fn domain_tags_medium() {
        let tags = domain_tags("https://medium.com/@user/post");
        assert!(tags.contains(&"web-article".to_string()));
        assert!(tags.contains(&"blog".to_string()));
        assert!(tags.contains(&"tech-writing".to_string()));
    }

    #[test]
    fn domain_tags_huggingface() {
        let tags = domain_tags("https://huggingface.co/blog/new-model");
        assert!(tags.contains(&"machine-learning".to_string()));
        assert!(tags.contains(&"huggingface".to_string()));
    }

    #[test]
    fn domain_tags_generic() {
        let tags = domain_tags("https://example.com/article");
        assert_eq!(tags, vec!["web-article".to_string()]);
    }

    #[test]
    fn extract_content_basic_html() {
        let html =
            "<html><head><title>My Article</title></head><body><p>Hello world</p></body></html>";
        let (text, title, _) = extract_content(html);
        assert_eq!(title, Some("My Article".to_string()));
        assert!(text.contains("Hello world"));
    }

    #[test]
    fn extract_content_entities() {
        let html = "<p>A &amp; B &lt; C</p>";
        let (text, _, _) = extract_content(html);
        assert!(text.contains("A & B < C"));
    }

    #[test]
    fn can_process_with_url() {
        let processor = WebArticleProcessor::new();
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "Blog post".to_string(),
            body: "https://example.com/article".to_string(),
            message_id: "test-1".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert!(processor.can_process(&artifact));
    }

    #[test]
    fn cannot_process_without_url() {
        let processor = WebArticleProcessor::new();
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "No link".to_string(),
            body: "Just some text".to_string(),
            message_id: "test-2".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        assert!(!processor.can_process(&artifact));
    }

    #[test]
    fn sync_process_creates_placeholder() {
        let processor = WebArticleProcessor::new();
        let artifact = RawArtifact {
            source_type: "web_url".to_string(),
            subject: "Great Article".to_string(),
            body: "https://example.com/great-article".to_string(),
            message_id: "test-3".to_string(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };

        let result = processor.process(&artifact).unwrap();
        assert_eq!(result.source_type, "web-article");
        assert_eq!(result.title, "Great Article");
        assert!(result.source_url.contains("example.com"));
        assert!(result.is_valid());
    }

    #[test]
    fn processor_metadata() {
        let processor = WebArticleProcessor::new();
        assert_eq!(processor.name(), "web-article");
        assert_eq!(processor.version(), "1.0.0");
        assert!(processor.accepts().contains(&"medium_article"));
        assert!(processor.accepts().contains(&"huggingface_blog"));
        assert!(processor.accepts().contains(&"web_url"));
    }
}
