//! Dedup engine — prevents reprocessing of known artifacts.
//!
//! Uses a JSONL index to track processed artifacts by hash, arXiv ID, and URL.
//!
//! Agent: luban

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// A single entry in the dedup index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arxiv_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<String>,
}

/// Hash-based deduplication for artifact processing.
pub struct DedupEngine {
    index_path: PathBuf,
    hashes: HashSet<String>,
    arxiv_ids: HashSet<String>,
    urls: HashSet<String>,
}

impl DedupEngine {
    /// Create a new dedup engine with the given index file path.
    pub fn new(index_path: PathBuf) -> Self {
        let mut engine = Self {
            index_path,
            hashes: HashSet::new(),
            arxiv_ids: HashSet::new(),
            urls: HashSet::new(),
        };
        engine.load();
        engine
    }

    fn load(&mut self) {
        let file = match std::fs::File::open(&self.index_path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            let entry: DedupEntry = match serde_json::from_str(&trimmed) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if let Some(ref h) = entry.hash {
                self.hashes.insert(h.clone());
            }
            if let Some(ref aid) = entry.arxiv_id {
                self.arxiv_ids.insert(aid.clone());
            }
            if let Some(ref url) = entry.url {
                self.urls.insert(normalize_url(url));
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
            if self.hashes.contains(h) {
                return true;
            }
        }
        if let Some(aid) = arxiv_id {
            if self.arxiv_ids.contains(aid) {
                return true;
            }
        }
        if let Some(u) = url {
            if self.urls.contains(&normalize_url(u)) {
                return true;
            }
        }
        false
    }

    /// Record a processed envelope in the dedup index.
    pub fn record(&mut self, envelope: &serde_json::Value) -> std::io::Result<()> {
        let mut entry = DedupEntry {
            hash: None,
            arxiv_id: None,
            url: None,
            envelope_id: envelope.get("envelope_id").and_then(|v| v.as_str()).map(String::from),
            title: envelope
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(100).collect()),
            recorded_at: envelope.get("created_at").and_then(|v| v.as_str()).map(String::from),
        };

        if let Some(h) = envelope.get("post_sanitization_hash").and_then(|v| v.as_str()) {
            entry.hash = Some(h.to_string());
            self.hashes.insert(h.to_string());
        }

        if let Some(url) = envelope.get("source_url").and_then(|v| v.as_str()) {
            if !url.is_empty() {
                entry.url = Some(url.to_string());
                self.urls.insert(normalize_url(url));
            }
        }

        if let Some(source_type) = envelope.get("source_type").and_then(|v| v.as_str()) {
            if source_type.contains("arxiv") {
                if let Some(aid) = envelope
                    .get("metadata")
                    .and_then(|m| m.get("arxiv_id"))
                    .and_then(|v| v.as_str())
                {
                    entry.arxiv_id = Some(aid.to_string());
                    self.arxiv_ids.insert(aid.to_string());
                }
            }
        }

        if let Some(parent) = self.index_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.index_path)?;

        let json = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{json}")?;
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
#[derive(Debug, Clone, Serialize)]
pub struct DedupStats {
    pub hashes: usize,
    pub arxiv_ids: usize,
    pub urls: usize,
}

/// Normalize URL for dedup comparison.
fn normalize_url(url: &str) -> String {
    let mut url = url.trim().trim_end_matches('/').to_string();
    url = url.replace("twitter.com/", "x.com/");
    if let Some(idx) = url.find('?') {
        if !url.contains("arxiv.org") {
            url.truncate(idx);
        }
    }
    url.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("https://x.com/user/status/123?s=20"),
            "https://x.com/user/status/123"
        );
        assert_eq!(
            normalize_url("https://twitter.com/user/status/123"),
            "https://x.com/user/status/123"
        );
        assert_eq!(
            normalize_url("https://arxiv.org/abs/2401.12345?v=2"),
            "https://arxiv.org/abs/2401.12345?v=2"
        );
    }

    #[test]
    fn test_dedup_engine_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().join("dedup-index.jsonl");
        let mut engine = DedupEngine::new(index_path.clone());

        assert!(!engine.is_duplicate(Some("abc123"), None, None));

        let envelope = serde_json::json!({
            "post_sanitization_hash": "abc123",
            "source_url": "https://arxiv.org/abs/2401.12345",
            "source_type": "arxiv-url",
            "envelope_id": "test-1",
            "title": "Test Paper",
            "created_at": "2026-03-15T00:00:00Z",
            "metadata": { "arxiv_id": "2401.12345" }
        });
        engine.record(&envelope).unwrap();

        assert!(engine.is_duplicate(Some("abc123"), None, None));
        assert!(engine.is_duplicate(None, Some("2401.12345"), None));
        assert!(engine.is_duplicate(None, None, Some("https://arxiv.org/abs/2401.12345")));
        assert!(!engine.is_duplicate(Some("other"), None, None));

        // Persistence: reload from disk
        let engine2 = DedupEngine::new(index_path);
        assert!(engine2.is_duplicate(Some("abc123"), None, None));
    }
}
