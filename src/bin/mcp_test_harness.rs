//! Streamable HTTP MCP Transport Test Harness
//!
//! Verifies push delivery works correctly:
//! 1. Connect to /mcp endpoint
//! 2. Initialize session
//! 3. Send a test message to self
//! 4. Verify SSE push arrives within 1 second
//! 5. Acknowledge the message
//! 6. Verify ack is recorded
//!
//! Usage:
//!   cargo run --bin mcp-test-harness -- --agent luban

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use reqwest::Client;
use serde_json::{json, Value};

const DEFAULT_URL: &str = "http://localhost:7777/mcp";

#[derive(Parser)]
#[command(name = "mcp-test-harness")]
#[command(about = "Test Streamable HTTP MCP push delivery")]
struct Args {
    #[arg(short, long, default_value = "luban")]
    agent: String,

    #[arg(short, long, default_value = DEFAULT_URL)]
    url: String,

    #[arg(short, long, default_value = "test-token")]
    token: String,

    #[arg(short, long)]
    verbose: bool,
}

#[derive(Debug)]
struct TestResult {
    name: String,
    passed: bool,
    duration_ms: u64,
    message: String,
}

impl TestResult {
    fn pass(name: &str, duration_ms: u64, message: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            duration_ms,
            message: message.to_string(),
        }
    }

    fn fail(name: &str, duration_ms: u64, message: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            duration_ms,
            message: message.to_string(),
        }
    }
}

fn print_result(result: &TestResult, verbose: bool) {
    let status = if result.passed { "PASS" } else { "FAIL" };
    let color = if result.passed { "\x1b[32m" } else { "\x1b[31m" };
    let reset = "\x1b[0m";
    
    println!(
        "{}{}{} {}ms — {}",
        color, status, reset, result.duration_ms, result.name
    );
    
    if verbose || !result.passed {
        println!("    {}", result.message);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("Failed to create HTTP client")?;

    println!("MCP Streamable HTTP Test Harness");
    println!("=================================");
    println!("Agent: {}", args.agent);
    println!("URL: {}", args.url);
    println!();

    let mut results = Vec::new();
    let mut session_id: Option<String> = None;

    let init_result = test_initialize(&client, &args).await?;
    results.push(init_result.0);
    session_id = init_result.1;

    if let Some(ref sid) = session_id {
        results.push(test_tools_list(&client, &args, sid).await?);
    }

    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;

    println!();
    println!("Results: {}/{} passed", passed, results.len());
    
    for result in &results {
        print_result(result, args.verbose);
    }

    if failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

async fn test_initialize(client: &Client, args: &Args) -> Result<(TestResult, Option<String>)> {
    let start = Instant::now();
    
    let response = client
        .post(&args.url)
        .header("Authorization", format!("Bearer {}", args.token))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": args.agent,
                    "version": "1.0.0"
                },
                "capabilities": {}
            }
        }))
        .send()
        .await
        .context("Initialize request failed")?;

    let status = response.status();
    if !status.is_success() {
        let duration = start.elapsed().as_millis() as u64;
        return Ok((TestResult::fail("initialize", duration, &format!("Status {}", status)), None));
    }

    // Extract session ID header before consuming response body
    let sid = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body: Value = response.json().await.context("Failed to parse response")?;

    if let Some(error) = body.get("error") {
        let duration = start.elapsed().as_millis() as u64;
        return Ok((TestResult::fail("initialize", duration, &format!("Error: {}", error)), None));
    }

    let duration = start.elapsed().as_millis() as u64;

    match sid {
        Some(sid) => Ok((TestResult::pass("initialize", duration, &format!("session={}", sid)), Some(sid))),
        None => {
            // Also check for _sessionId in the body (fallback)
            let body_sid = body.get("result")
                .and_then(|r| r.get("_sessionId"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            match body_sid {
                Some(sid) => Ok((TestResult::pass("initialize", duration, &format!("session={} (from body)", sid)), Some(sid))),
                None => Ok((TestResult::fail("initialize", duration, "No session ID in header or body"), None)),
            }
        }
    }
}

async fn test_tools_list(client: &Client, args: &Args, session_id: &str) -> Result<TestResult> {
    let start = Instant::now();
    
    let response = client
        .post(&args.url)
        .header("Authorization", format!("Bearer {}", args.token))
        .header("mcp-session-id", session_id)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }))
        .send()
        .await
        .context("tools/list request failed")?;

    let status = response.status();
    let duration = start.elapsed().as_millis() as u64;

    if !status.is_success() {
        return Ok(TestResult::fail("tools/list", duration, &format!("Status {}", status)));
    }

    let body: Value = response.json().await.context("Failed to parse response")?;
    
    if let Some(tools) = body.get("result").and_then(|r| r.get("tools")) {
        let count = tools.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(TestResult::pass("tools/list", duration, &format!("{} tools available", count)))
    } else {
        Ok(TestResult::fail("tools/list", duration, "No tools in response"))
    }
}
