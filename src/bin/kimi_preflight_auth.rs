//! Pre-flight OAuth token refresh for Kimi headless wake.
//!
//! Called by am-fleet before spawning Kimi sessions to ensure
//! the credential file has a fresh access token. Eliminates the race
//! condition where an interactive session rotates the refresh_token while
//! the headless session is starting up.
//!
//! Exit codes:
//!   0 — Token is fresh (existing or newly refreshed)
//!   1 — Refresh failed (credentials missing, API error, etc.)

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

const OAUTH_HOST: &str = "https://auth.kimi.com";
const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const DEFAULT_THRESHOLD: u64 = 300; // 5 minutes

// ============================================================================
// CLI
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "kimi-preflight-auth", about = "Pre-flight OAuth token refresh for Kimi")]
struct Args {
    /// Minimum remaining token lifetime in seconds before refresh
    #[arg(long, default_value_t = DEFAULT_THRESHOLD)]
    threshold: u64,

    /// Path to Kimi credentials file
    #[arg(long)]
    credentials: Option<PathBuf>,
}

// ============================================================================
// Credentials
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Credentials {
    access_token: String,
    refresh_token: String,
    expires_at: f64,
    scope: String,
    token_type: String,
}

fn credentials_path(args: &Args) -> PathBuf {
    args.credentials.clone().unwrap_or_else(|| {
        dirs_next().join(".kimi").join("credentials").join("kimi-code.json")
    })
}

fn dirs_next() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn load_credentials(path: &PathBuf) -> Option<Credentials> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_credentials(path: &PathBuf, creds: &Credentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create credentials dir")?;
    }
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string(creds).context("serialize credentials")?;
    std::fs::write(&tmp, &json).context("write tmp credentials")?;

    // Set permissions to 0o600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .context("set permissions")?;
    }

    std::fs::rename(&tmp, path).context("atomic rename credentials")?;
    Ok(())
}

// ============================================================================
// OAuth refresh
// ============================================================================

fn refresh_token_sync(refresh_tok: &str) -> Result<Credentials> {
    let body = format!(
        "client_id={}&grant_type=refresh_token&refresh_token={}",
        CLIENT_ID, refresh_tok
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("build HTTP client")?;

    let resp = client
        .post(format!("{}/api/oauth/token", OAUTH_HOST))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .context("OAuth request failed")?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("OAuth refresh HTTP {}", status.as_u16());
    }

    let payload: serde_json::Value = resp.json().context("parse OAuth response")?;

    let expires_in = payload["expires_in"]
        .as_f64()
        .context("missing expires_in")?;

    Ok(Credentials {
        access_token: payload["access_token"]
            .as_str()
            .context("missing access_token")?
            .to_string(),
        refresh_token: payload["refresh_token"]
            .as_str()
            .context("missing refresh_token")?
            .to_string(),
        expires_at: now_epoch() + expires_in,
        scope: payload["scope"].as_str().unwrap_or("").to_string(),
        token_type: payload["token_type"]
            .as_str()
            .unwrap_or("Bearer")
            .to_string(),
    })
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args = Args::parse();
    let cred_path = credentials_path(&args);

    let creds = match load_credentials(&cred_path) {
        Some(c) => c,
        None => {
            eprintln!("ERROR: No kimi credentials found at {}", cred_path.display());
            std::process::exit(1);
        }
    };

    let remaining = creds.expires_at - now_epoch();

    if remaining >= args.threshold as f64 {
        eprintln!("OK: Token valid for {:.0}s", remaining);
        std::process::exit(0);
    }

    if creds.refresh_token.is_empty() {
        eprintln!("ERROR: No refresh token available");
        std::process::exit(1);
    }

    match refresh_token_sync(&creds.refresh_token) {
        Ok(new_creds) => {
            if let Err(e) = save_credentials(&cred_path, &new_creds) {
                eprintln!("ERROR: Failed to save credentials: {}", e);
                std::process::exit(1);
            }
            let new_remaining = new_creds.expires_at - now_epoch();
            eprintln!("OK: Token refreshed, valid for {:.0}s", new_remaining);
            std::process::exit(0);
        }
        Err(e) => {
            let err_str = format!("{}", e);
            // Handle race condition: another session may have rotated the refresh token
            if err_str.contains("401") || err_str.contains("403") {
                if let Some(creds2) = load_credentials(&cred_path) {
                    if creds2.refresh_token != creds.refresh_token {
                        // Another session already refreshed, retry with new token
                        match refresh_token_sync(&creds2.refresh_token) {
                            Ok(new_creds) => {
                                if let Err(e) = save_credentials(&cred_path, &new_creds) {
                                    eprintln!("ERROR: Failed to save credentials: {}", e);
                                    std::process::exit(1);
                                }
                                let new_remaining = new_creds.expires_at - now_epoch();
                                eprintln!(
                                    "OK: Token refreshed (retry), valid for {:.0}s",
                                    new_remaining
                                );
                                std::process::exit(0);
                            }
                            Err(e2) => {
                                eprintln!("ERROR: Retry refresh failed: {}", e2);
                                std::process::exit(1);
                            }
                        }
                    }
                }
                eprintln!("ERROR: Token refresh rejected: {}", e);
                std::process::exit(1);
            }
            eprintln!("ERROR: Token refresh failed: {}", e);
            std::process::exit(1);
        }
    }
}
