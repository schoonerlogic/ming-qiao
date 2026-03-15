//! X/Twitter post processor v1.0 — subject-only extraction.
//!
//! Extracts author handle, engagement metrics, and tweet URL from
//! self-sent Gmail emails containing X/Twitter links.
//!
//! Agent: luban

use chrono::Utc;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::processor::{ArtifactProcessor, ProcessorError, ProcessorResult, RawArtifact};

const VERSION: &str = "1.0.0";

static TWEET_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)https?://(?:x\.com|twitter\.com)/(\w+)/status/(\d+)").unwrap()
});
static SUBJECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?)\s+\(@(\w+)\)\s+(.*?)$").unwrap());
static ENGAGEMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)([\d,.]+[KkMm]?)\s+(likes?|replies?|reposts?|retweets?|bookmarks?)").unwrap()
});
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"']+"#).unwrap());
static ARXIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)arxiv\.org/abs/(\d{4}\.\d{4,5})").unwrap());
static GITHUB_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)github\.com/([\w.-]+/[\w.-]+)").unwrap());

/// Parse engagement metrics from subject line text.
fn parse_engagement(text: &str) -> HashMap<String, i64> {
    let mut metrics = HashMap::new();

    for caps in ENGAGEMENT_RE.captures_iter(text) {
        let raw_num = &caps[1];
        let metric_type = caps[2].to_lowercase();

        let key = if metric_type.starts_with("like") {
            "likes"
        } else if metric_type.starts_with("repl") {
            "replies"
        } else if metric_type.starts_with("repo") || metric_type.starts_with("retw") {
            "reposts"
        } else if metric_type.starts_with("book") {
            "bookmarks"
        } else {
            continue;
        };

        let num_str = raw_num.replace(',', "");
        let (num_str, multiplier) = if num_str.ends_with('K') || num_str.ends_with('k') {
            (&num_str[..num_str.len() - 1], 1000.0)
        } else if num_str.ends_with('M') || num_str.ends_with('m') {
            (&num_str[..num_str.len() - 1], 1_000_000.0)
        } else {
            (num_str.as_str(), 1.0)
        };

        if let Ok(n) = num_str.parse::<f64>() {
            metrics.insert(key.to_string(), (n * multiplier) as i64);
        }
    }

    metrics
}

/// Classify an embedded URL by type hint.
fn classify_url(url: &str) -> &'static str {
    if ARXIV_RE.is_match(url) {
        "arxiv_url"
    } else if GITHUB_RE.is_match(url) {
        "reference"
    } else if ["medium.com", "huggingface.co/blog", "substack.com"]
        .iter()
        .any(|d| url.contains(d))
    {
        "web_article"
    } else if ["youtube.com", "youtu.be"].iter().any(|d| url.contains(d)) {
        "video"
    } else if [".png", ".jpg", ".jpeg", ".gif", ".mp4"]
        .iter()
        .any(|ext| url.to_lowercase().contains(ext))
    {
        "media"
    } else {
        "web_url"
    }
}

/// X/Twitter post processor.
pub struct XPostProcessor;

impl XPostProcessor {
    pub fn new() -> Self {
        Self
    }
}

impl ArtifactProcessor for XPostProcessor {
    fn name(&self) -> &str {
        "x-post"
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn accepts(&self) -> &[&str] {
        &["x_post"]
    }

    fn can_process(&self, artifact: &RawArtifact) -> bool {
        let combined = format!("{} {}", artifact.subject, artifact.body);
        TWEET_URL_RE.is_match(&combined)
    }

    fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError> {
        let now = Utc::now();
        let combined = format!("{} {}", artifact.subject, artifact.body);

        let caps = TWEET_URL_RE
            .captures(&combined)
            .ok_or_else(|| ProcessorError::MissingField("tweet URL".to_string()))?;

        let handle_from_url = caps[1].to_string();
        let tweet_id = caps[2].to_string();
        let tweet_url = format!("https://x.com/{handle_from_url}/status/{tweet_id}");

        // Parse subject line
        let mut author_name = String::new();
        let mut author_handle = handle_from_url.clone();
        let mut engagement = HashMap::new();

        if let Some(subj_caps) = SUBJECT_RE.captures(&artifact.subject) {
            author_name = subj_caps[1].trim().to_string();
            author_handle = subj_caps[2].to_string();
            engagement = parse_engagement(&subj_caps[3]);
        }

        // Find embedded URLs in body (excluding the tweet URL itself)
        let mut embedded_artifacts = Vec::new();
        for mat in URL_RE.find_iter(&artifact.body) {
            let url = mat.as_str();
            if TWEET_URL_RE.is_match(url) {
                continue;
            }
            let url_type = classify_url(url);
            if url_type != "media" {
                embedded_artifacts.push(serde_json::json!({
                    "url": url,
                    "type_hint": url_type,
                    "discovered_in": "x-post",
                    "discovered_from": tweet_url,
                }));
            }
        }

        // Build title
        let topic_hint = if !embedded_artifacts.is_empty() {
            let types: std::collections::HashSet<&str> = embedded_artifacts
                .iter()
                .filter_map(|a| a.get("type_hint").and_then(|v| v.as_str()))
                .collect();
            if types.contains("arxiv_url") {
                " sharing paper"
            } else if types.contains("reference") {
                " sharing repo"
            } else {
                " sharing link"
            }
        } else {
            ""
        };

        let display = if !author_name.is_empty() {
            &author_name
        } else {
            &format!("@{author_handle}")
        };
        let title = format!("{display}{topic_hint}");

        // Build content
        let mut content_parts = vec![
            format!("X post by {display} (@{author_handle})"),
            format!("URL: {tweet_url}"),
        ];
        if !engagement.is_empty() {
            let eng_str: Vec<String> = engagement
                .iter()
                .map(|(k, v)| format!("{v} {k}"))
                .collect();
            content_parts.push(format!("Engagement: {}", eng_str.join(", ")));
        }
        if !embedded_artifacts.is_empty() {
            content_parts.push(format!("Embedded links: {}", embedded_artifacts.len()));
            for ea in &embedded_artifacts {
                let url = ea.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let hint = ea.get("type_hint").and_then(|v| v.as_str()).unwrap_or("");
                content_parts.push(format!("  - [{hint}] {url}"));
            }
        }
        content_parts.push(
            "\n[Tweet text not yet fetched — v1.0 subject-only extraction]".to_string(),
        );

        let mut metadata = HashMap::new();
        metadata.insert(
            "author_handle".to_string(),
            serde_json::json!(author_handle),
        );
        metadata.insert("author_name".to_string(), serde_json::json!(author_name));
        metadata.insert("tweet_id".to_string(), serde_json::json!(tweet_id));
        metadata.insert("engagement".to_string(), serde_json::json!(engagement));
        metadata.insert(
            "has_embedded_links".to_string(),
            serde_json::json!(!embedded_artifacts.is_empty()),
        );
        metadata.insert(
            "embedded_link_count".to_string(),
            serde_json::json!(embedded_artifacts.len()),
        );

        Ok(ProcessorResult {
            title,
            content: content_parts.join("\n"),
            source_url: tweet_url,
            source_type: "x-post".to_string(),
            processor_version: VERSION.to_string(),
            domain_tags: vec!["social-signal".to_string(), "curated".to_string()],
            metadata,
            embedded_artifacts,
            fetched_at: now,
            processed_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_engagement() {
        let metrics = parse_engagement("6K likes · 319 replies · 42 reposts");
        assert_eq!(metrics.get("likes"), Some(&6000));
        assert_eq!(metrics.get("replies"), Some(&319));
        assert_eq!(metrics.get("reposts"), Some(&42));
    }

    #[test]
    fn test_parse_engagement_decimal() {
        let metrics = parse_engagement("1.2K likes");
        assert_eq!(metrics.get("likes"), Some(&1200));
    }

    #[test]
    fn test_classify_url() {
        assert_eq!(classify_url("https://arxiv.org/abs/2401.12345"), "arxiv_url");
        assert_eq!(
            classify_url("https://github.com/user/repo"),
            "reference"
        );
        assert_eq!(
            classify_url("https://medium.com/article"),
            "web_article"
        );
        assert_eq!(classify_url("https://example.com/page"), "web_url");
        assert_eq!(classify_url("https://example.com/photo.png"), "media");
    }

    #[test]
    fn test_process_x_post() {
        let proc = XPostProcessor::new();
        let artifact = RawArtifact {
            source_type: "x_post".to_string(),
            subject: "Andrej Karpathy (@karpathy) 6K likes · 319 replies".to_string(),
            body: "https://x.com/karpathy/status/2031135152349524125?s=20".to_string(),
            message_id: String::new(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };

        let result = proc.process(&artifact).unwrap();
        assert_eq!(result.title, "Andrej Karpathy");
        assert_eq!(result.source_type, "x-post");
        assert!(result.content.contains("@karpathy"));
        assert!(result
            .metadata
            .get("engagement")
            .unwrap()
            .get("likes")
            .is_some());
    }

    #[test]
    fn test_process_with_embedded_links() {
        let proc = XPostProcessor::new();
        let artifact = RawArtifact {
            source_type: "x_post".to_string(),
            subject: "User (@user) 100 likes".to_string(),
            body: "https://x.com/user/status/123 https://arxiv.org/abs/2401.99999".to_string(),
            message_id: String::new(),
            date: String::new(),
            attachments: vec![],
            metadata: HashMap::new(),
        };

        let result = proc.process(&artifact).unwrap();
        assert!(result.title.contains("sharing paper"));
        assert_eq!(result.embedded_artifacts.len(), 1);
    }
}
