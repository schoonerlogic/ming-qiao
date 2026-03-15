//! Dedup engine — prevents reprocessing of known artifacts.
//!
//! Uses a JSONL index to track processed artifacts by hash, arXiv ID, and URL.
//!
//! Agent: luban

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Entry in the dedup JSONL index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupEntry {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hash: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arxiv_id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub envelope_id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recorded_at: String,
}

/// Hash-based deduplication for artifact processing.
pub struct DedupEngine {
    index_path: PathBuf,
    hashes: HashSet<String>,
    arxiv_ids: HashSet<String>,
    urls: HashSet<String>,
}

impl DedupEngine {
    /// Create a new dedup engine with the given JSONL index path.
    pub fn new(index_path: impl Into<PathBuf>) -> Self {
        let index_path = index_path.into();
        let mut engine = Self {
            index_path,
            hashes: HashSet::new(),
            arxiv_ids: HashSet::new(),
            urls: HashSet::new(),
        };
        engine.load();
        engine
    }

    /// Load dedup state from the JSONL index file.
    fn load(&mut self) {
        let path = &self.index_path;
        if !path.exists() {
            return;
        }

        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("failed to open dedup index {}: {}", path.display(), e);
                return;
            }
        };

        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            let entry: DedupEntry = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.hash.is_empty() {
                self.hashes.insert(entry.hash);
            }
            if !entry.arxiv_id.is_empty() {
                self.arxiv_ids.insert(entry.arxiv_id);
            }
            if !entry.url.is_empty() {
                self.urls.insert(normalize_url(&entry.url));
            }
        }
    }

    /// Check if an artifact has already been processed.
    pub fn is_duplicate(
        &self,
        hash: Option<&str>,
        arxiv_id: Option<&str>,
        url: Option<&str>,
    ) -> bool {
        if let Some(h) = hash {
            if !h.is_empty() && self.hashes.contains(h) {
                return true;
            }
        }
        if let Some(aid) = arxiv_id {
            if !aid.is_empty() && self.arxiv_ids.contains(aid) {
                return true;
            }
        }
        if let Some(u) = url {
            if !u.is_empty() && self.urls.contains(&normalize_url(u)) {
                return true;
            }
        }
        false
    }

    /// Record a processed envelope in the dedup index.
    pub fn record(&mut self, envelope: &serde_json::Value) -> Result<(), DedupError> {
        let mut entry = DedupEntry {
            hash: String::new(),
            arxiv_id: String::new(),
            url: String::new(),
            envelope_id: String::new(),
            title: String::new(),
            recorded_at: String::new(),
        };

        if let Some(h) = envelope.get("post_sanitization_hash").and_then(|v| v.as_str()) {
            entry.hash = h.to_string();
            self.hashes.insert(h.to_string());
        }

        if let Some(url) = envelope.get("source_url").and_then(|v| v.as_str()) {
            if !url.is_empty() {
                entry.url = url.to_string();
                self.urls.insert(normalize_url(url));
            }
        }

        // Extract arXiv ID if source_type contains "arxiv"
        if let Some(st) = envelope.get("source_type").and_then(|v| v.as_str()) {
            if st.contains("arxiv") {
                if let Some(aid) = envelope
                    .get("metadata")
                    .and_then(|m| m.get("arxiv_id"))
                    .and_then(|v| v.as_str())
                {
                    entry.arxiv_id = aid.to_string();
                    self.arxiv_ids.insert(aid.to_string());
                }
            }
        }

        if let Some(eid) = envelope.get("envelope_id").and_then(|v| v.as_str()) {
            entry.envelope_id = eid.to_string();
        }
        if let Some(title) = envelope.get("title").and_then(|v| v.as_str()) {
            // Truncate to 100 chars like Python version
            entry.title = title.chars().take(100).collect();
        }
        if let Some(ts) = envelope.get("created_at").and_then(|v| v.as_str()) {
            entry.recorded_at = ts.to_string();
        }

        self.append_entry(&entry)?;
        Ok(())
    }

    /// Append a single entry to the JSONL index file.
    fn append_entry(&self, entry: &DedupEntry) -> Result<(), DedupError> {
        if let Some(parent) = self.index_path.parent() {
            fs::create_dir_all(parent).map_err(|e| DedupError::Io(e.to_string()))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.index_path)
            .map_err(|e| DedupError::Io(e.to_string()))?;

        let json = serde_json::to_string(entry).map_err(|e| DedupError::Serialize(e.to_string()))?;
        writeln!(file, "{json}").map_err(|e| DedupError::Io(e.to_string()))?;

        Ok(())
    }

    /// Return dedup index statistics.
    pub fn stats(&self) -> DedupStats {
        DedupStats {
            hashes: self.hashes.len(),
            arxiv_ids: self.arxiv_ids.len(),
            urls: self.urls.len(),
        }
    }
}

/// Statistics about the dedup index.
#[derive(Debug, Clone)]
pub struct DedupStats {
    pub hashes: usize,
    pub arxiv_ids: usize,
    pub urls: usize,
}

/// Errors from the dedup engine.
#[derive(Debug, thiserror::Error)]
pub enum DedupError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("serialization error: {0}")]
    Serialize(String),
}

/// Normalize URL for dedup comparison.
///
/// Strips trailing slashes, normalizes twitter.com to x.com,
/// and removes tracking parameters (except on arxiv.org).
pub fn normalize_url(url: &str) -> String {
    let mut url = url.trim().trim_end_matches('/').to_string();

    // Normalize x.com / twitter.com
    url = url.replace("twitter.com/", "x.com/");

    // Strip tracking params (keep arxiv version params)
    if let Some(pos) = url.find('?') {
        if !url[..pos].contains("arxiv.org") {
            url.truncate(pos);
        }
    }

    url.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_basics() {
        assert_eq!(normalize_url("https://example.com/"), "https://example.com");
        assert_eq!(
            normalize_url("https://twitter.com/user/status/123"),
            "https://x.com/user/status/123"
        );
        assert_eq!(
            normalize_url("https://example.com/page?utm_source=test"),
            "https://example.com/page"
        );
        // arXiv params preserved
        assert_eq!(
            normalize_url("https://arxiv.org/abs/2401.12345?v=2"),
            "https://arxiv.org/abs/2401.12345?v=2"
        );
    }

    #[test]
    fn normalize_url_case_insensitive() {
        assert_eq!(
            normalize_url("HTTPS://EXAMPLE.COM/Page"),
            "https://example.com/page"
        );
    }

    #[test]
    fn dedup_engine_empty() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("dedup-index.jsonl");
        let engine = DedupEngine::new(&index);
        assert!(!engine.is_duplicate(Some("abc123"), None, None));
        let stats = engine.stats();
        assert_eq!(stats.hashes, 0);
    }

    #[test]
    fn dedup_engine_record_and_check() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("dedup-index.jsonl");
        let mut engine = DedupEngine::new(&index);

        let envelope = serde_json::json!({
            "post_sanitization_hash": "abc123def456",
            "source_url": "https://arxiv.org/abs/2401.12345",
            "source_type": "arxiv-url",
            "metadata": { "arxiv_id": "2401.12345" },
            "envelope_id": "arxiv-url-abc123def456",
            "title": "Test Paper Title",
            "created_at": "2026-03-15T12:00:00Z"
        });

        engine.record(&envelope).unwrap();

        assert!(engine.is_duplicate(Some("abc123def456"), None, None));
        assert!(engine.is_duplicate(None, Some("2401.12345"), None));
        assert!(engine.is_duplicate(
            None,
            None,
            Some("https://arxiv.org/abs/2401.12345")
        ));
        assert!(!engine.is_duplicate(Some("other"), None, None));

        let stats = engine.stats();
        assert_eq!(stats.hashes, 1);
        assert_eq!(stats.arxiv_ids, 1);
        assert_eq!(stats.urls, 1);
    }

    #[test]
    fn dedup_engine_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("dedup-index.jsonl");

        // Write with one engine
        {
            let mut engine = DedupEngine::new(&index);
            let envelope = serde_json::json!({
                "post_sanitization_hash": "persist_test",
                "source_url": "https://example.com/article",
                "source_type": "web-article",
                "envelope_id": "web-article-persist_test",
                "title": "Persistence Test",
                "created_at": "2026-03-15T12:00:00Z"
            });
            engine.record(&envelope).unwrap();
        }

        // Read with a new engine
        let engine2 = DedupEngine::new(&index);
        assert!(engine2.is_duplicate(Some("persist_test"), None, None));
        assert!(engine2.is_duplicate(
            None,
            None,
            Some("https://example.com/article")
        ));
    }
}
