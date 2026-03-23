//! Daily Security Intel Sweep — Rust rewrite of daily-agentic-cve-sweep.py
//!
//! Sources:
//!   - CISA KEV (Known Exploited Vulnerabilities)
//!   - NVD (last 24h, stack keyword filtered)
//!   - MITRE CVE API (fallback enrichment)
//!   - GitHub Security Advisories (ecosystems: rust, pip, npm)
//!   - RustSec feed (RSS/XML)
//!   - OSV querybatch
//!   - Neocloud provider breach monitoring (status pages, HN Algolia, GHSA)
//!
//! Usage:
//!   cve-sweep                           # Full sweep with defaults
//!   cve-sweep --notify-council ogma     # Notify specific agent
//!   cve-sweep --dry-run                 # Sweep but don't send notifications

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use clap::Parser;
use regex::Regex;
use serde::Serialize;
use tracing::{info, warn};

// ============================================================================
// CLI
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "cve-sweep", about = "Daily security intel sweep for AstralMaris fleet")]
struct Args {
    /// Ming-qiao repo root (for mq-send.sh)
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main")]
    repo_root: PathBuf,

    /// Report output directory
    #[arg(long, default_value = "/Users/proteus/astralmaris/everwatch-spire/ogma/reports")]
    report_dir: PathBuf,

    /// Agent to notify with summary
    #[arg(long, default_value = "council")]
    notify_council: String,

    /// Agent to notify for critical/Vast.ai alerts
    #[arg(long, default_value = "proteus")]
    notify_proteus: String,

    /// Max NVD results per keyword query
    #[arg(long, default_value_t = 50)]
    max_nvd_per_query: usize,

    /// GHSA pages to fetch per ecosystem
    #[arg(long, default_value_t = 2)]
    ghsa_pages: usize,

    /// Max MITRE enrichment calls
    #[arg(long, default_value_t = 10)]
    max_mitre_enrichment: usize,

    /// Delay between NVD queries (seconds)
    #[arg(long, default_value_t = 6.2)]
    nvd_delay_seconds: f64,

    /// Dry run — don't send notifications
    #[arg(long)]
    dry_run: bool,
}

// ============================================================================
// Constants
// ============================================================================

const KEV_FEED: &str = "https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json";
const NVD_API: &str = "https://services.nvd.nist.gov/rest/json/cves/2.0";
const MITRE_CVE_API: &str = "https://cveawg.mitre.org/api/cve";
const GHSA_API: &str = "https://api.github.com/advisories";
const RUSTSEC_FEED: &str = "https://rustsec.org/feed.xml";
const OSV_QUERYBATCH: &str = "https://api.osv.dev/v1/querybatch";

const STACK_KEYWORDS: &[&str] = &[
    "rust", "nats", "docker", "falkordb", "graphiti", "sveltekit",
    "claude", "anthropic", "mcp", "surrealdb", "spiffe", "spire",
];

const GHSA_ECOSYSTEMS: &[&str] = &["rust", "pip", "npm"];

const OSV_PACKAGES: &[(&str, &str)] = &[
    ("crates.io", "nats"),
    ("crates.io", "surrealdb"),
    ("crates.io", "spiffe"),
    ("crates.io", "spiffeid"),
    ("crates.io", "spire-api"),
    ("PyPI", "anthropic"),
    ("PyPI", "mcp"),
    ("PyPI", "nats-py"),
    ("PyPI", "surrealdb"),
    ("npm", "@sveltejs/kit"),
    ("npm", "@modelcontextprotocol/sdk"),
];

const NEOCLOUD_INCIDENT_TERMS: &[&str] = &[
    "breach", "hack", "incident", "compromised", "outage",
    "security", "vulnerability", "leak", "unauthorized",
];

// ============================================================================
// Neocloud providers
// ============================================================================

struct NeocloudProvider {
    name: &'static str,
    slug: &'static str,
    active: bool,
    priority: &'static str,
    status_url: &'static str,
    hn_keywords: &'static [&'static str],
    ghsa_packages: &'static [(&'static str, &'static str)],
}

const NEOCLOUD_PROVIDERS: &[NeocloudProvider] = &[
    NeocloudProvider {
        name: "Vast.ai", slug: "vastai", active: true, priority: "highest",
        status_url: "https://status.vast.ai",
        hn_keywords: &["vast.ai", "vastai"],
        ghsa_packages: &[("PyPI", "vastai")],
    },
    NeocloudProvider {
        name: "Lambda Labs", slug: "lambda", active: false, priority: "high",
        status_url: "https://status.lambdalabs.com",
        hn_keywords: &["lambda labs", "lambdalabs", "lambda cloud"],
        ghsa_packages: &[("PyPI", "lambda-cloud-sdk")],
    },
    NeocloudProvider {
        name: "RunPod", slug: "runpod", active: false, priority: "medium",
        status_url: "https://status.runpod.io",
        hn_keywords: &["runpod"],
        ghsa_packages: &[("PyPI", "runpod")],
    },
    NeocloudProvider {
        name: "CoreWeave", slug: "coreweave", active: false, priority: "medium",
        status_url: "https://status.coreweave.com",
        hn_keywords: &["coreweave"],
        ghsa_packages: &[],
    },
    NeocloudProvider {
        name: "Nebius", slug: "nebius", active: false, priority: "low",
        status_url: "https://status.nebius.com",
        hn_keywords: &["nebius"],
        ghsa_packages: &[],
    },
];

// ============================================================================
// Data types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
struct Finding {
    id: String,
    kind: String,
    description: String,
    published: Option<String>,
    last_modified: Option<String>,
    cvss_score: Option<f64>,
    severity: String,
    matched_keywords: Vec<String>,
    sources: Vec<String>,
    kev: bool,
    kev_due_date: String,
    nvd_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct RustsecEntry {
    title: String,
    link: String,
    published: String,
    summary: String,
}

#[derive(Debug, Clone, Serialize)]
struct Notice {
    id: String,
    summary: String,
    url: String,
}

#[derive(Debug, Clone, Serialize)]
struct NeocloudResult {
    name: String,
    slug: String,
    active: bool,
    priority: String,
    status: String,
    hn_mentions: Vec<HnMention>,
    ghsa_advisories: Vec<GhsaAdvisory>,
    alert_level: String,
}

#[derive(Debug, Clone, Serialize)]
struct HnMention {
    title: String,
    url: String,
    created_at: String,
    matched_term: String,
}

#[derive(Debug, Clone, Serialize)]
struct GhsaAdvisory {
    id: String,
    summary: String,
    url: String,
    severity: String,
}

// Internal mutable finding builder
struct FindingBuilder {
    description: String,
    published: Option<String>,
    last_modified: Option<String>,
    cvss_score: Option<f64>,
    severity: String,
    matched_keywords: HashSet<String>,
    sources: HashSet<String>,
    kev: bool,
    kev_due_date: String,
}

impl FindingBuilder {
    fn new() -> Self {
        Self {
            description: String::new(),
            published: None,
            last_modified: None,
            cvss_score: None,
            severity: "UNKNOWN".to_string(),
            matched_keywords: HashSet::new(),
            sources: HashSet::new(),
            kev: false,
            kev_due_date: String::new(),
        }
    }

    fn to_finding(&self, cve_id: &str) -> Finding {
        let mut kws: Vec<String> = self.matched_keywords.iter().cloned().collect();
        kws.sort();
        let mut srcs: Vec<String> = self.sources.iter().cloned().collect();
        srcs.sort();
        Finding {
            id: cve_id.to_string(),
            kind: "CVE".to_string(),
            description: self.description.clone(),
            published: self.published.clone(),
            last_modified: self.last_modified.clone(),
            cvss_score: self.cvss_score,
            severity: self.severity.clone(),
            matched_keywords: kws,
            sources: srcs,
            kev: self.kev,
            kev_due_date: self.kev_due_date.clone(),
            nvd_url: format!("https://nvd.nist.gov/vuln/detail/{}", cve_id),
        }
    }
}

// ============================================================================
// HTTP helpers
// ============================================================================

async fn fetch_json(
    client: &reqwest::Client,
    url: &str,
    retries: usize,
) -> Result<serde_json::Value> {
    for attempt in 0..=retries {
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp.json().await.context("JSON parse failed");
                }
                if attempt >= retries || !matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504) {
                    anyhow::bail!("HTTP {} for {}", status, url);
                }
                let delay = Duration::from_secs_f64(2.0 * (attempt as f64 + 1.0));
                tokio::time::sleep(delay).await;
            }
            Err(e) => {
                if attempt >= retries {
                    return Err(e.into());
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    anyhow::bail!("unreachable")
}

async fn fetch_json_post(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("User-Agent", "ogma-security-intel/1.0")
        .json(body)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} for POST {}", resp.status(), url);
    }
    resp.json().await.context("JSON parse failed")
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .header("User-Agent", "ogma-security-intel/1.0")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} for {}", resp.status(), url);
    }
    resp.text().await.context("text decode failed")
}

// ============================================================================
// CVSS / severity helpers
// ============================================================================

fn severity_from_cvss(score: Option<f64>) -> &'static str {
    match score {
        None => "UNKNOWN",
        Some(s) if s >= 9.0 => "CRITICAL",
        Some(s) if s >= 7.0 => "HIGH",
        Some(s) if s >= 4.0 => "MEDIUM",
        _ => "LOW",
    }
}

fn parse_nvd_cvss(cve: &serde_json::Value) -> (Option<f64>, String) {
    let metrics = &cve["metrics"];
    for key in &["cvssMetricV31", "cvssMetricV30", "cvssMetricV2"] {
        if let Some(rows) = metrics[key].as_array() {
            if let Some(first) = rows.first() {
                let data = &first["cvssData"];
                if let Some(score) = data["baseScore"].as_f64() {
                    let sev = data["baseSeverity"].as_str().unwrap_or("").to_uppercase();
                    return (Some(score), sev);
                }
            }
        }
    }
    (None, String::new())
}

fn english_desc_nvd(cve: &serde_json::Value) -> String {
    if let Some(descs) = cve["descriptions"].as_array() {
        for d in descs {
            if d["lang"].as_str() == Some("en") {
                if let Some(v) = d["value"].as_str() {
                    return normalize_whitespace(v);
                }
            }
        }
    }
    String::new()
}

fn english_desc_mitre(payload: &serde_json::Value) -> String {
    if let Some(descs) = payload["containers"]["cna"]["descriptions"].as_array() {
        for d in descs {
            if d["lang"].as_str() == Some("en") {
                if let Some(v) = d["value"].as_str() {
                    return normalize_whitespace(v);
                }
            }
        }
    }
    String::new()
}

fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn matches_stack_keywords(text: &str) -> HashSet<String> {
    let lower = text.to_lowercase();
    let mut found = HashSet::new();
    for &kw in STACK_KEYWORDS {
        let pattern = format!(r"\b{}\b", regex::escape(kw));
        if let Ok(re) = Regex::new(&pattern) {
            if re.is_match(&lower) {
                found.insert(kw.to_string());
            }
        }
    }
    // MCP often appears as acronym
    if let Ok(re) = Regex::new(r"\bmcp\b|model context protocol") {
        if re.is_match(&lower) {
            found.insert("mcp".to_string());
        }
    }
    found
}

fn trim_text(text: &str, max: usize) -> String {
    let normalized = normalize_whitespace(text);
    if normalized.len() <= max {
        normalized
    } else {
        format!("{}...", &normalized[..max - 3])
    }
}

fn fmt_cvss(score: Option<f64>) -> String {
    match score {
        None => "-".to_string(),
        Some(s) => format!("{:.1}", s),
    }
}

// ============================================================================
// Running services detection
// ============================================================================

fn running_service_names() -> HashSet<String> {
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN"])
        .output();

    let mut names = HashSet::new();
    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines().skip(1) {
            if let Some(name) = line.split_whitespace().next() {
                names.insert(name.to_lowercase());
            }
        }
    }
    names
}

// ============================================================================
// Policy filter
// ============================================================================

fn allow_by_policy(
    severity: &str,
    matched_keywords: &HashSet<String>,
    service_names: &HashSet<String>,
    searchable_text: &str,
) -> bool {
    match severity {
        "CRITICAL" | "HIGH" => true,
        "MEDIUM" => !matched_keywords.is_empty(),
        "LOW" => {
            let lower = searchable_text.to_lowercase();
            service_names.iter().any(|name| !name.is_empty() && lower.contains(name.as_str()))
        }
        _ => false,
    }
}

// ============================================================================
// Source queries
// ============================================================================

async fn query_nvd_last24h(
    client: &reqwest::Client,
    keyword: &str,
    start_utc: &DateTime<Utc>,
    end_utc: &DateTime<Utc>,
    limit: usize,
) -> Result<Vec<serde_json::Value>> {
    let start_str = start_utc.format("%Y-%m-%dT%H:%M:%S.000").to_string();
    let end_str = end_utc.format("%Y-%m-%dT%H:%M:%S.000").to_string();
    let url = format!(
        "{}?keywordSearch={}&keywordExactMatch&pubStartDate={}&pubEndDate={}&resultsPerPage={}",
        NVD_API,
        urlencoding::encode(keyword),
        urlencoding::encode(&start_str),
        urlencoding::encode(&end_str),
        limit
    );
    let payload = fetch_json(client, &url, 2).await?;
    Ok(payload["vulnerabilities"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

async fn enrich_mitre(client: &reqwest::Client, cve_id: &str) -> String {
    let url = format!("{}/{}", MITRE_CVE_API, urlencoding::encode(cve_id));
    match fetch_json(client, &url, 1).await {
        Ok(payload) => english_desc_mitre(&payload),
        Err(_) => String::new(),
    }
}

async fn query_ghsa(
    client: &reqwest::Client,
    ecosystem: &str,
    pages: usize,
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let headers = [
        ("Accept", "application/vnd.github+json"),
        ("User-Agent", "ogma-security-intel/1.0"),
    ];

    for page in 1..=pages {
        let url = format!(
            "{}?ecosystem={}&type=reviewed&per_page=100&page={}",
            GHSA_API,
            urlencoding::encode(ecosystem),
            page
        );
        let mut req = client.get(&url);
        for (k, v) in &headers {
            req = req.header(*k, *v);
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<Vec<serde_json::Value>>().await {
                    if data.is_empty() {
                        break;
                    }
                    out.extend(data);
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    out
}

async fn parse_rustsec_recent(
    client: &reqwest::Client,
    start_utc: &DateTime<Utc>,
) -> Vec<RustsecEntry> {
    let xml_text = match fetch_text(client, RUSTSEC_FEED).await {
        Ok(t) => t,
        Err(_) => return vec![],
    };

    let mut entries = Vec::new();
    let mut reader = quick_xml::Reader::from_str(&xml_text);
    reader.config_mut().trim_text(true);

    let mut in_item = false;
    let mut title = String::new();
    let mut link = String::new();
    let mut pub_date = String::new();
    let mut description = String::new();
    let mut current_tag = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "item" {
                    in_item = true;
                    title.clear();
                    link.clear();
                    pub_date.clear();
                    description.clear();
                }
                if in_item {
                    current_tag = tag;
                }
            }
            Ok(quick_xml::events::Event::Text(e)) => {
                if in_item {
                    let text = e.unescape().unwrap_or_default().to_string();
                    match current_tag.as_str() {
                        "title" => title.push_str(&text),
                        "link" => link.push_str(&text),
                        "pubDate" => pub_date.push_str(&text),
                        "description" => description.push_str(&text),
                        _ => {}
                    }
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "item" && in_item {
                    in_item = false;
                    if !title.is_empty() {
                        // Filter by date if parseable
                        let include = if !pub_date.is_empty() {
                            DateTime::parse_from_rfc2822(&pub_date)
                                .map(|dt| dt.with_timezone(&Utc) >= *start_utc)
                                .unwrap_or(true)
                        } else {
                            true
                        };
                        if include {
                            // Strip HTML tags from description
                            let clean_desc = Regex::new(r"<[^>]+>")
                                .map(|re| re.replace_all(&description, "").to_string())
                                .unwrap_or(description.clone());
                            entries.push(RustsecEntry {
                                title: title.trim().to_string(),
                                link: link.trim().to_string(),
                                published: pub_date.trim().to_string(),
                                summary: clean_desc.trim().to_string(),
                            });
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    entries.truncate(50);
    entries
}

async fn query_osv_recent(
    client: &reqwest::Client,
    start_utc: &DateTime<Utc>,
) -> Vec<serde_json::Value> {
    let queries: Vec<serde_json::Value> = OSV_PACKAGES
        .iter()
        .map(|(eco, name)| {
            serde_json::json!({
                "package": { "ecosystem": eco, "name": name }
            })
        })
        .collect();

    let body = serde_json::json!({ "queries": queries });

    let payload = match fetch_json_post(client, OSV_QUERYBATCH, &body).await {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let mut out = Vec::new();
    if let Some(results) = payload["results"].as_array() {
        for result in results {
            if let Some(vulns) = result["vulns"].as_array() {
                for vuln in vulns {
                    let modified = vuln["modified"]
                        .as_str()
                        .or_else(|| vuln["published"].as_str());
                    if let Some(ts_str) = modified {
                        let ts_str = ts_str.replace('Z', "+00:00");
                        if let Ok(mts) = DateTime::parse_from_rfc3339(&ts_str) {
                            if mts.with_timezone(&Utc) < *start_utc {
                                continue;
                            }
                        }
                    }
                    out.push(vuln.clone());
                }
            }
        }
    }
    out
}

// ============================================================================
// Neocloud monitoring
// ============================================================================

async fn check_neocloud_status(client: &reqwest::Client, provider: &NeocloudProvider) -> String {
    if provider.status_url.is_empty() {
        return "no-status-url".to_string();
    }

    let text_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| client.clone());

    match fetch_text(&text_client, provider.status_url).await {
        Ok(text) => {
            let lower = text.to_lowercase();
            if lower.contains("all systems operational") || lower.contains("all services are online") {
                "operational".to_string()
            } else if lower.contains("major outage") || lower.contains("service disruption") {
                "major-outage".to_string()
            } else if lower.contains("degraded") {
                "degraded".to_string()
            } else if lower.contains("partial outage") || lower.contains("minor") {
                "partial-outage".to_string()
            } else if lower.contains("maintenance") {
                "maintenance".to_string()
            } else {
                "check-manually".to_string()
            }
        }
        Err(_) => "unreachable".to_string(),
    }
}

async fn check_neocloud_hn(
    client: &reqwest::Client,
    provider: &NeocloudProvider,
) -> Vec<HnMention> {
    let mut hits = Vec::new();
    let cutoff = Utc::now().timestamp() - 86400;

    for keyword in provider.hn_keywords {
        for term in NEOCLOUD_INCIDENT_TERMS {
            let query = format!("{} {}", keyword, term);
            let url = format!(
                "https://hn.algolia.com/api/v1/search_by_date?query={}&tags=story&numericFilters=created_at_i>{}",
                urlencoding::encode(&query),
                cutoff
            );

            let hn_client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| client.clone());

            if let Ok(data) = fetch_json(&hn_client, &url, 0).await {
                if let Some(results) = data["hits"].as_array() {
                    for hit in results.iter().take(3) {
                        let title = hit["title"].as_str().unwrap_or("").trim().to_string();
                        if title.is_empty() {
                            continue;
                        }
                        let object_id = hit["objectID"].as_str().unwrap_or("");
                        hits.push(HnMention {
                            title,
                            url: format!("https://news.ycombinator.com/item?id={}", object_id),
                            created_at: hit["created_at"]
                                .as_str()
                                .unwrap_or("")
                                .chars()
                                .take(16)
                                .collect(),
                            matched_term: term.to_string(),
                        });
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    // Deduplicate by title
    let mut seen = HashSet::new();
    hits.retain(|h| seen.insert(h.title.clone()));
    hits
}

async fn check_neocloud_ghsa(
    client: &reqwest::Client,
    provider: &NeocloudProvider,
) -> Vec<GhsaAdvisory> {
    let mut advisories = Vec::new();

    for (ecosystem, package) in provider.ghsa_packages {
        let url = format!(
            "{}?ecosystem={}&per_page=10",
            GHSA_API,
            urlencoding::encode(ecosystem)
        );

        let resp = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "ogma-security-intel/1.0")
            .send()
            .await;

        if let Ok(resp) = resp {
            if let Ok(data) = resp.json::<Vec<serde_json::Value>>().await {
                for adv in &data {
                    if adv["withdrawn_at"].as_str().is_some() {
                        continue;
                    }
                    let summary = adv["summary"].as_str().unwrap_or("").to_string();
                    let pkg_match = adv["vulnerabilities"]
                        .as_array()
                        .map(|vulns| {
                            vulns.iter().any(|v| {
                                v["package"]["name"]
                                    .as_str()
                                    .map(|n| n.eq_ignore_ascii_case(package))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false);

                    if pkg_match || summary.to_lowercase().contains(&package.to_lowercase()) {
                        advisories.push(GhsaAdvisory {
                            id: adv["ghsa_id"].as_str().unwrap_or("GHSA").to_string(),
                            summary: summary.trim().to_string(),
                            url: adv["html_url"].as_str().unwrap_or("").to_string(),
                            severity: adv["severity"].as_str().unwrap_or("unknown").to_string(),
                        });
                    }
                }
            }
        }
    }
    advisories
}

async fn sweep_neocloud_providers(client: &reqwest::Client) -> Vec<NeocloudResult> {
    let mut results = Vec::new();
    for provider in NEOCLOUD_PROVIDERS {
        let status = check_neocloud_status(client, provider).await;
        let hn_hits = check_neocloud_hn(client, provider).await;
        let ghsa_hits = check_neocloud_ghsa(client, provider).await;

        let mut alert_level = "clear".to_string();
        if matches!(status.as_str(), "major-outage" | "degraded") || !hn_hits.is_empty() || !ghsa_hits.is_empty() {
            alert_level = "warning".to_string();
        }
        if provider.active && alert_level == "warning" {
            alert_level = "alert".to_string();
        }

        results.push(NeocloudResult {
            name: provider.name.to_string(),
            slug: provider.slug.to_string(),
            active: provider.active,
            priority: provider.priority.to_string(),
            status,
            hn_mentions: hn_hits,
            ghsa_advisories: ghsa_hits,
            alert_level,
        });
    }
    results
}

// ============================================================================
// Notification via mq-send.sh
// ============================================================================

fn send_mq(repo_root: &PathBuf, to: &str, subject: &str, body: &str, intent: &str) -> bool {
    let script = repo_root.join("scripts").join("mq-send.sh");
    if !script.exists() {
        warn!("mq-send.sh not found at {}", script.display());
        return false;
    }
    let output = Command::new("bash")
        .args([
            script.to_str().unwrap_or(""),
            to,
            subject,
            body,
            "--intent",
            intent,
        ])
        .current_dir(repo_root)
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                print!("{}", stdout);
                true
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                warn!("mq-send failed ({}): {}", to, stderr.trim());
                false
            }
        }
        Err(e) => {
            warn!("mq-send exec failed: {}", e);
            false
        }
    }
}

// ============================================================================
// Main sweep
// ============================================================================

async fn run(args: &Args) -> Result<i32> {
    let now = Utc::now();
    let start_utc = now - chrono::Duration::hours(24);
    let local_now = Local::now();
    let day = local_now.format("%Y-%m-%d").to_string();

    let report_dir = &args.report_dir;
    let evidence_dir = report_dir.join("evidence").join("cve-intel");
    let report_path = report_dir.join(format!("{}-daily-intel.md", day));
    let json_path = evidence_dir.join(format!("{}-daily-intel.json", day));

    let service_names = running_service_names();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(35))
        .user_agent("ogma-security-intel/1.0")
        .build()
        .context("HTTP client build failed")?;

    let mut findings: HashMap<String, FindingBuilder> = HashMap::new();

    // 1) NVD (last 24h, stack keywords)
    info!("Querying NVD for {} keywords...", STACK_KEYWORDS.len());
    for kw in STACK_KEYWORDS {
        match query_nvd_last24h(&client, kw, &start_utc, &now, args.max_nvd_per_query).await {
            Ok(vulns) => {
                for wrapped in &vulns {
                    let cve = &wrapped["cve"];
                    let cve_id = match cve["id"].as_str() {
                        Some(id) => id.to_string(),
                        None => continue,
                    };
                    let desc = english_desc_nvd(cve);
                    let (score, sev) = parse_nvd_cvss(cve);

                    let row = findings.entry(cve_id.clone()).or_insert_with(FindingBuilder::new);
                    if row.description.is_empty() {
                        row.description = desc.clone();
                    }
                    if row.published.is_none() {
                        row.published = cve["published"].as_str().map(String::from);
                    }
                    row.last_modified = cve["lastModified"]
                        .as_str()
                        .map(String::from)
                        .or_else(|| row.last_modified.clone());
                    if row.cvss_score.is_none() {
                        if let Some(s) = score {
                            row.cvss_score = Some(s);
                            row.severity = if sev.is_empty() {
                                severity_from_cvss(Some(s)).to_string()
                            } else {
                                sev.clone()
                            };
                        }
                    }
                    row.sources.insert("NVD".to_string());
                    row.matched_keywords.insert(kw.to_string());
                    row.matched_keywords.extend(matches_stack_keywords(&format!("{} {}", desc, cve_id)));
                }
            }
            Err(e) => {
                warn!("NVD query failed for '{}': {}", kw, e);
            }
        }
        tokio::time::sleep(Duration::from_secs_f64(args.nvd_delay_seconds)).await;
    }

    // MITRE enrichment for missing descriptions
    info!("MITRE enrichment...");
    let mut enrich_budget = args.max_mitre_enrichment;
    let cve_ids: Vec<String> = findings.keys().cloned().collect();
    for cve_id in &cve_ids {
        if enrich_budget == 0 {
            break;
        }
        if let Some(row) = findings.get(cve_id) {
            if !row.description.is_empty() {
                continue;
            }
        }
        let desc = enrich_mitre(&client, cve_id).await;
        if !desc.is_empty() {
            if let Some(row) = findings.get_mut(cve_id) {
                row.description = desc;
                row.sources.insert("MITRE".to_string());
            }
        }
        enrich_budget -= 1;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // 2) KEV mapping
    info!("Fetching CISA KEV...");
    let (kev_version, kev_date) = match fetch_json(&client, KEV_FEED, 2).await {
        Ok(kev_payload) => {
            let version = kev_payload["catalogVersion"].as_str().unwrap_or("unknown").to_string();
            let date = kev_payload["dateReleased"].as_str().unwrap_or("unknown").to_string();
            if let Some(vulns) = kev_payload["vulnerabilities"].as_array() {
                for kev_entry in vulns {
                    if let Some(kev_cve_id) = kev_entry["cveID"].as_str() {
                        if let Some(row) = findings.get_mut(kev_cve_id) {
                            row.kev = true;
                            row.kev_due_date = kev_entry["dueDate"].as_str().unwrap_or("").to_string();
                            row.sources.insert("CISA-KEV".to_string());
                        }
                    }
                }
            }
            (version, date)
        }
        Err(e) => {
            warn!("KEV fetch failed: {}", e);
            ("unknown".to_string(), "unknown".to_string())
        }
    };

    // 3) GHSA
    info!("Querying GHSA...");
    let mut nats_notices: Vec<Notice> = Vec::new();
    let mut anthropic_notices: Vec<Notice> = Vec::new();

    for eco in GHSA_ECOSYSTEMS {
        let advisories = query_ghsa(&client, eco, args.ghsa_pages).await;
        for adv in &advisories {
            if adv["withdrawn_at"].as_str().is_some() {
                continue;
            }
            let text_blob = format!(
                "{} {} {}",
                adv["summary"].as_str().unwrap_or(""),
                adv["description"].as_str().unwrap_or(""),
                adv["source_code_location"].as_str().unwrap_or("")
            );
            let src_loc = adv["source_code_location"].as_str().unwrap_or("").to_lowercase();
            let lower_blob = text_blob.to_lowercase();

            if src_loc.contains("github.com/nats-io/nats-server") || lower_blob.contains("nats") {
                nats_notices.push(Notice {
                    id: adv["ghsa_id"].as_str().unwrap_or("GHSA").to_string(),
                    summary: adv["summary"].as_str().unwrap_or("").trim().to_string(),
                    url: adv["html_url"].as_str().unwrap_or("").to_string(),
                });
            }
            if src_loc.contains("github.com/anthropics/anthropic-sdk-python")
                || Regex::new(r"\banthropic\b|\bmcp\b|\bclaude\b")
                    .map(|re| re.is_match(&lower_blob))
                    .unwrap_or(false)
            {
                anthropic_notices.push(Notice {
                    id: adv["ghsa_id"].as_str().unwrap_or("GHSA").to_string(),
                    summary: adv["summary"].as_str().unwrap_or("").trim().to_string(),
                    url: adv["html_url"].as_str().unwrap_or("").to_string(),
                });
            }

            if let Some(cve_id) = adv["cve_id"].as_str() {
                let row = findings.entry(cve_id.to_string()).or_insert_with(FindingBuilder::new);
                if row.description.is_empty() {
                    row.description = adv["summary"]
                        .as_str()
                        .or_else(|| adv["description"].as_str())
                        .unwrap_or("")
                        .to_string();
                }
                if row.cvss_score.is_none() {
                    if let Some(score) = adv["cvss"]["score"].as_f64() {
                        row.cvss_score = Some(score);
                        row.severity = severity_from_cvss(Some(score)).to_string();
                    }
                }
                row.sources.insert("GHSA".to_string());
                row.matched_keywords.extend(matches_stack_keywords(&text_blob));
            }
        }
    }

    // 4) RustSec
    info!("Parsing RustSec feed...");
    let rustsec_recent = parse_rustsec_recent(&client, &start_utc).await;

    // 5) OSV
    info!("Querying OSV...");
    let osv_recent = query_osv_recent(&client, &start_utc).await;

    // 6) Neocloud
    info!("Sweeping neocloud providers...");
    let neocloud_results = sweep_neocloud_providers(&client).await;

    // Merge OSV into findings
    for vuln in &osv_recent {
        let aliases = vuln["aliases"].as_array();
        let cve_id = aliases.and_then(|a| {
            a.iter()
                .find_map(|v| v.as_str().filter(|s| s.starts_with("CVE-")).map(String::from))
        });
        let summary = vuln["summary"]
            .as_str()
            .or_else(|| vuln["details"].as_str())
            .unwrap_or("")
            .to_string();

        if let Some(cve_id) = cve_id {
            let row = findings.entry(cve_id).or_insert_with(FindingBuilder::new);
            if row.description.is_empty() {
                row.description = summary.clone();
            }
            row.sources.insert("OSV".to_string());
            row.matched_keywords.extend(matches_stack_keywords(&summary));
        }
    }

    // Fill severity defaults and keyword matches from descriptions
    for row in findings.values_mut() {
        if row.severity == "UNKNOWN" {
            row.severity = severity_from_cvss(row.cvss_score).to_string();
        }
        if !row.description.is_empty() {
            row.matched_keywords.extend(matches_stack_keywords(&row.description));
        }
    }

    // Apply policy filter
    let mut filtered: Vec<Finding> = findings
        .iter()
        .filter_map(|(cve_id, row)| {
            let searchable = format!(
                "{} {} {}",
                cve_id,
                row.description,
                row.matched_keywords.iter().cloned().collect::<Vec<_>>().join(" ")
            );
            if allow_by_policy(&row.severity, &row.matched_keywords, &service_names, &searchable) {
                Some(row.to_finding(cve_id))
            } else {
                None
            }
        })
        .collect();

    // Sort by severity then CVSS
    let severity_order = |s: &str| -> u8 {
        match s {
            "CRITICAL" => 0,
            "HIGH" => 1,
            "MEDIUM" => 2,
            "LOW" => 3,
            _ => 4,
        }
    };
    filtered.sort_by(|a, b| {
        severity_order(&a.severity)
            .cmp(&severity_order(&b.severity))
            .then_with(|| {
                b.cvss_score
                    .unwrap_or(0.0)
                    .partial_cmp(&a.cvss_score.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.id.cmp(&b.id))
    });

    let critical: Vec<&Finding> = filtered.iter().filter(|f| f.severity == "CRITICAL").collect();
    let high: Vec<&Finding> = filtered.iter().filter(|f| f.severity == "HIGH").collect();
    let medium: Vec<&Finding> = filtered.iter().filter(|f| f.severity == "MEDIUM").collect();

    // Write JSON evidence
    std::fs::create_dir_all(&evidence_dir).context("create evidence dir")?;
    let json_output = serde_json::json!({
        "generated_at": local_now.to_rfc3339(),
        "window_start_utc": start_utc.to_rfc3339(),
        "window_end_utc": now.to_rfc3339(),
        "stack_keywords": STACK_KEYWORDS,
        "kev_catalog_version": kev_version,
        "kev_catalog_date": kev_date,
        "running_service_names": service_names.iter().cloned().collect::<Vec<_>>(),
        "findings": filtered,
        "rustsec_recent": rustsec_recent,
        "nats_notices": &nats_notices[..nats_notices.len().min(30)],
        "anthropic_notices": &anthropic_notices[..anthropic_notices.len().min(30)],
        "osv_recent_count": osv_recent.len(),
        "neocloud_provider_status": neocloud_results,
    });
    std::fs::write(&json_path, serde_json::to_string_pretty(&json_output)?)
        .context("write JSON evidence")?;

    // Write Markdown report
    let mut lines = Vec::new();
    lines.push(format!("# Daily Security Intel - {}", day));
    lines.push(String::new());
    lines.push(format!("- Generated: {}", local_now.to_rfc3339()));
    lines.push(format!("- Window: {} to {}", start_utc.to_rfc3339(), now.to_rfc3339()));
    lines.push(format!("- NVD keywords: {}", STACK_KEYWORDS.join(", ")));
    lines.push(format!("- KEV catalog: {} ({})", kev_version, kev_date));
    lines.push(format!("- Findings after policy filter: {}", filtered.len()));
    lines.push(format!(
        "- Critical: {} | High: {} | Medium: {}",
        critical.len(),
        high.len(),
        medium.len()
    ));
    lines.push(String::new());

    let add_table = |lines: &mut Vec<String>, title: &str, rows: &[&Finding], limit: usize| {
        if rows.is_empty() {
            return;
        }
        lines.push(format!("## {}", title));
        lines.push(String::new());
        lines.push("| ID | Severity | CVSS | KEV | Keywords | Summary |".to_string());
        lines.push("|---|---|---:|---|---|---|".to_string());
        for r in rows.iter().take(limit) {
            let kev = if r.kev { "yes" } else { "no" };
            let kws = if r.matched_keywords.is_empty() {
                "-".to_string()
            } else {
                r.matched_keywords.join(", ")
            };
            lines.push(format!(
                "| {} | {} | {} | {} | {} | {} |",
                r.id,
                r.severity,
                fmt_cvss(r.cvss_score),
                kev,
                kws,
                trim_text(&r.description, 180)
            ));
        }
        lines.push(String::new());
    };

    add_table(&mut lines, "Critical Findings", &critical, 50);
    add_table(&mut lines, "High Findings", &high, 70);
    add_table(&mut lines, "Medium Findings (Stack-Matched)", &medium, 80);

    if !rustsec_recent.is_empty() {
        lines.push("## RustSec (Last 24h)".to_string());
        lines.push(String::new());
        for item in rustsec_recent.iter().take(20) {
            lines.push(format!("- {} ({})", item.title, item.published));
            if !item.link.is_empty() {
                lines.push(format!("  Link: {}", item.link));
            }
        }
        lines.push(String::new());
    }

    if !nats_notices.is_empty() {
        lines.push("## NATS Advisory Channel".to_string());
        lines.push(String::new());
        for n in nats_notices.iter().take(15) {
            lines.push(format!("- {}: {}", n.id, n.summary));
            if !n.url.is_empty() {
                lines.push(format!("  Link: {}", n.url));
            }
        }
        lines.push(String::new());
    }

    if !anthropic_notices.is_empty() {
        lines.push("## Anthropic/MCP Advisory Channel".to_string());
        lines.push(String::new());
        for n in anthropic_notices.iter().take(15) {
            lines.push(format!("- {}: {}", n.id, n.summary));
            if !n.url.is_empty() {
                lines.push(format!("  Link: {}", n.url));
            }
        }
        lines.push(String::new());
    }

    // Neocloud Provider Status
    lines.push("## Neocloud Provider Status".to_string());
    lines.push(String::new());
    for nc in &neocloud_results {
        let active_tag = if nc.active { " **[ACTIVE]**" } else { "" };
        let alert_tag = match nc.alert_level.as_str() {
            "alert" => " *** ALERT ***",
            "warning" => " * WARNING *",
            _ => "",
        };
        lines.push(format!("- **{}**{}: {}{}", nc.name, active_tag, nc.status, alert_tag));
        for hn in &nc.hn_mentions {
            lines.push(format!(
                "  - HN: [{}]({}) ({}, {})",
                hn.title, hn.url, hn.matched_term, hn.created_at
            ));
        }
        for adv in &nc.ghsa_advisories {
            lines.push(format!("  - GHSA: {} ({}): {}", adv.id, adv.severity, adv.summary));
            if !adv.url.is_empty() {
                lines.push(format!("    Link: {}", adv.url));
            }
        }
    }
    lines.push(String::new());

    lines.push("## References".to_string());
    lines.push(String::new());
    lines.push(format!("- CISA KEV: {}", KEV_FEED));
    lines.push(format!("- NVD API: {}", NVD_API));
    lines.push(format!("- MITRE CVE API: {}", MITRE_CVE_API));
    lines.push(format!("- GHSA API: {}", GHSA_API));
    lines.push(format!("- RustSec feed: {}", RUSTSEC_FEED));
    lines.push(format!("- OSV querybatch: {}", OSV_QUERYBATCH));
    lines.push(String::new());

    std::fs::create_dir_all(report_path.parent().unwrap()).context("create report dir")?;
    std::fs::write(&report_path, lines.join("\n")).context("write report")?;

    println!("Wrote report: {}", report_path.display());
    println!("Wrote JSON:   {}", json_path.display());
    println!(
        "Findings: total={} critical={} high={} medium={}",
        filtered.len(),
        critical.len(),
        high.len(),
        medium.len()
    );

    // Distribution
    if !args.dry_run {
        let subject = format!("Daily Security Intel - {}", day);
        let top: Vec<String> = filtered
            .iter()
            .take(5)
            .map(|r| {
                format!(
                    "- {} ({}, cvss={})",
                    r.id,
                    r.severity,
                    fmt_cvss(r.cvss_score)
                )
            })
            .collect();
        let body = format!(
            "Daily sweep complete ({}).\nWindow: last 24h.\nFindings: total={}, critical={}, high={}, medium={}.\nTop items:\n{}\nReport: {}",
            day,
            filtered.len(),
            critical.len(),
            high.len(),
            medium.len(),
            top.join("\n"),
            report_path.display()
        );
        send_mq(&args.repo_root, &args.notify_council, &subject, &body, "inform");

        // Vast.ai alert escalation
        let vastai_alert: Vec<&NeocloudResult> = neocloud_results
            .iter()
            .filter(|nc| nc.slug == "vastai" && nc.alert_level == "alert")
            .collect();
        if !vastai_alert.is_empty() {
            let nc = &vastai_alert[0];
            let mut parts = vec![
                format!("Vast.ai breach/incident signal detected in daily sweep ({}).", day),
                format!("Status page: {}", nc.status),
            ];
            for hn in nc.hn_mentions.iter().take(5) {
                parts.push(format!("- HN: {} ({})", hn.title, hn.matched_term));
            }
            for adv in nc.ghsa_advisories.iter().take(5) {
                parts.push(format!("- GHSA: {}: {}", adv.id, adv.summary));
            }
            parts.push(format!("Report: {}", report_path.display()));
            send_mq(
                &args.repo_root,
                &args.notify_proteus,
                &format!("ALERT: Vast.ai incident signal - {}", day),
                &parts.join("\n"),
                "request",
            );
        }

        // Critical alert
        if !critical.is_empty() {
            let crit_items: Vec<String> = critical
                .iter()
                .take(10)
                .map(|r| {
                    format!(
                        "- {} (cvss={}, kev={})",
                        r.id,
                        fmt_cvss(r.cvss_score),
                        if r.kev { "yes" } else { "no" }
                    )
                })
                .collect();
            let body = format!(
                "Critical vulnerabilities detected in daily security intel sweep.\n{}\nReport: {}",
                crit_items.join("\n"),
                report_path.display()
            );
            send_mq(
                &args.repo_root,
                &args.notify_proteus,
                &format!("CRITICAL Security Intel Alert - {}", day),
                &body,
                "request",
            );
        }
    }

    Ok(0)
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    info!("════════════════════════════════════════");
    info!("CVE Sweep (Rust) — Daily Security Intel");
    info!("  Report dir: {}", args.report_dir.display());
    info!("  Dry run: {}", args.dry_run);
    info!("════════════════════════════════════════");

    let rc = run(&args).await?;
    std::process::exit(rc);
}
