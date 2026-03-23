//! Generate MCP Streamable HTTP configs for all agents.
//!
//! Reads agent-capabilities.toml and generates:
//! - OpenCode configs (opencode.json) for Luban + Jikimi
//! - Kimi configs (kimi-mcp-http.json) for Mataya + Laozi-Jung
//!
//! Rust rewrite of generate-streamable-http-configs.py

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;

const DEFAULT_MCP_URL: &str = "http://localhost:7777/mcp";

const OPENCODE_AGENTS: &[&str] = &["luban", "jikimi"];
const KIMI_AGENTS: &[&str] = &["mataya", "laozi-jung"];

// ============================================================================
// CLI
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "generate-streamable-http-configs", about = "Generate MCP Streamable HTTP configs")]
struct Args {
    /// Output directory for generated configs
    #[arg(long, default_value = "./output")]
    output_dir: PathBuf,

    /// MCP Streamable HTTP URL
    #[arg(long, default_value = DEFAULT_MCP_URL)]
    mcp_url: String,

    /// Agent capabilities TOML path
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/agent-capabilities.toml")]
    capabilities: PathBuf,

    /// Print configs without writing files
    #[arg(long)]
    dry_run: bool,
}

// ============================================================================
// Agent capabilities TOML
// ============================================================================

#[derive(Debug, Deserialize)]
struct Capabilities {
    agents: HashMap<String, toml::Value>,
}

// ============================================================================
// Config generators
// ============================================================================

fn generate_opencode_config(agent_id: &str, mcp_url: &str) -> serde_json::Value {
    let model = if agent_id == "jikimi" {
        "ollama/qwen3:8b"
    } else {
        "glm-5"
    };

    serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "model": model,
        "permission": {
            "edit": "ask",
            "bash": { "*": "allow" }
        },
        "provider": {
            "ollama": {
                "options": {
                    "baseURL": "http://localhost:11434/v1"
                }
            }
        },
        "mcp": {
            "ming-qiao": {
                "type": "remote",
                "url": mcp_url
            }
        }
    })
}

fn generate_kimi_config(mcp_url: &str) -> serde_json::Value {
    serde_json::json!({
        "mcpServers": {
            "ming-qiao": {
                "transport": "http",
                "url": mcp_url
            }
        }
    })
}

// ============================================================================
// Main
// ============================================================================

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.capabilities.exists() {
        anyhow::bail!(
            "agent-capabilities.toml not found at {}",
            args.capabilities.display()
        );
    }

    let content = std::fs::read_to_string(&args.capabilities)
        .with_context(|| format!("read {}", args.capabilities.display()))?;
    let caps: Capabilities =
        toml::from_str(&content).with_context(|| "parse agent-capabilities.toml")?;

    if !args.dry_run {
        std::fs::create_dir_all(args.output_dir.join("opencode"))
            .context("create opencode output dir")?;
        std::fs::create_dir_all(args.output_dir.join("kimi"))
            .context("create kimi output dir")?;
    }

    println!("Generating configs for MCP URL: {}", args.mcp_url);
    println!();

    for &agent_id in OPENCODE_AGENTS {
        if !caps.agents.contains_key(agent_id) {
            println!(
                "Warning: {} not found in agent-capabilities.toml, skipping",
                agent_id
            );
            continue;
        }

        let config = generate_opencode_config(agent_id, &args.mcp_url);
        let config_path = args
            .output_dir
            .join("opencode")
            .join(format!("{}-opencode.json", agent_id));

        if args.dry_run {
            println!("\n--- {} OpenCode config ---", agent_id);
            println!("{}", serde_json::to_string_pretty(&config)?);
        } else {
            std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)
                .with_context(|| format!("write {}", config_path.display()))?;
            println!("Written: {}", config_path.display());
        }
    }

    for &agent_id in KIMI_AGENTS {
        if !caps.agents.contains_key(agent_id) {
            println!(
                "Warning: {} not found in agent-capabilities.toml, skipping",
                agent_id
            );
            continue;
        }

        let config = generate_kimi_config(&args.mcp_url);
        let config_path = args
            .output_dir
            .join("kimi")
            .join(format!("{}-kimi-mcp.json", agent_id));

        if args.dry_run {
            println!("\n--- {} Kimi config ---", agent_id);
            println!("{}", serde_json::to_string_pretty(&config)?);
        } else {
            std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)
                .with_context(|| format!("write {}", config_path.display()))?;
            println!("Written: {}", config_path.display());
        }
    }

    println!();
    println!("Done. To apply configs:");
    println!();
    println!("OpenCode (Luban, Jikimi):");
    println!("  cp output/opencode/{{agent}}-opencode.json ~/.config/opencode/config.json");
    println!();
    println!("Kimi (Mataya, Laozi-Jung):");
    println!("  kimi mcp add --transport http ming-qiao {}", args.mcp_url);
    println!();

    Ok(())
}
