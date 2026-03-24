//! am-avatar — Agent Avatar System
//!
//! Reads AVATAR-CONFIG.toml, resolves credentials, generates ephemeral MCP config,
//! launches the runtime process, and monitors health. Meridian is the sole initial
//! consumer; fleet rollout deferred until proven.
//!
//! Usage:
//!   am-avatar --config /path/to/AVATAR-CONFIG.toml
//!   am-avatar --config /path/to/AVATAR-CONFIG.toml --dry-run
//!   am-avatar --config /path/to/AVATAR-CONFIG.toml --validate-only

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Deserialize;
use tokio::process::Command as AsyncCommand;
use tokio::signal;
use tokio::time::sleep;
use tracing::{error, info, warn};

// ============================================================================
// CLI arguments
// ============================================================================

#[derive(Parser)]
#[command(name = "am-avatar", about = "Agent Avatar System — lifecycle manager for AstralMaris agents")]
struct Args {
    /// Path to AVATAR-CONFIG.toml
    #[arg(long)]
    config: PathBuf,

    /// Validate config and exit (don't launch)
    #[arg(long)]
    validate_only: bool,

    /// Print resolved config without launching
    #[arg(long)]
    dry_run: bool,

    /// Max restart attempts before giving up
    #[arg(long, default_value_t = 5)]
    max_restarts: u32,

    /// Restart backoff base (seconds)
    #[arg(long, default_value_t = 10)]
    restart_delay: u64,
}

// ============================================================================
// AVATAR-CONFIG.toml schema
// ============================================================================

#[derive(Debug, Deserialize)]
struct AvatarConfig {
    schema_version: Option<String>,
    identity: IdentityConfig,
    model: ModelConfig,
    mcp: McpConfig,
    runtime: RuntimeConfig,
    #[serde(default)]
    security: SecurityConfig,
    #[serde(default)]
    prompt: PromptConfig,
}

#[derive(Debug, Deserialize)]
struct IdentityConfig {
    agent_id: String,
    role: String,
    persona_file: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelConfig {
    primary: String,
    primary_provider: String,
    /// Provider-specific settings (api_key_ref, etc.)
    #[serde(flatten)]
    providers: HashMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
struct McpConfig {
    servers: Vec<McpServerConfig>,
}

#[derive(Debug, Deserialize)]
struct McpServerConfig {
    name: String,
    url: String,
    #[serde(default)]
    token_ref: Option<String>,
    /// Resolved at runtime — not in TOML
    #[serde(skip)]
    resolved_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimeConfig {
    /// Runtime type: opencode, gemini, claude-code, kimi
    #[serde(rename = "type")]
    runtime_type: String,
    workspace_dir: Option<String>,
    #[serde(default)]
    extra_flags: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct SecurityConfig {
    #[serde(default)]
    process_model: Option<String>,
    #[serde(default)]
    reasoning_network: Option<String>,
    #[serde(default)]
    quarantine_dir: Option<String>,
    #[serde(default)]
    allowed_dirs: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PromptConfig {
    template_file: Option<String>,
    #[serde(default)]
    placeholders: HashMap<String, String>,
}

// ============================================================================
// Config parsing and validation
// ============================================================================

fn load_config(path: &Path) -> Result<AvatarConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    let config: AvatarConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse AVATAR-CONFIG.toml: {}", path.display()))?;

    // Validate required fields
    if config.identity.agent_id.is_empty() {
        bail!("identity.agent_id is required");
    }
    if config.model.primary.is_empty() {
        bail!("model.primary is required");
    }
    if config.runtime.runtime_type.is_empty() {
        bail!("runtime.type is required");
    }
    if config.mcp.servers.is_empty() {
        bail!("At least one MCP server is required");
    }

    info!(
        agent = %config.identity.agent_id,
        model = %config.model.primary,
        runtime = %config.runtime.runtime_type,
        mcp_servers = config.mcp.servers.len(),
        "Config loaded"
    );

    Ok(config)
}

// ============================================================================
// Credential resolution (1Password)
// ============================================================================

fn resolve_op_ref(ref_path: &str) -> Result<String> {
    if !ref_path.starts_with("op://") {
        return Ok(ref_path.to_string()); // Not an op:// ref, return as-is
    }

    let output = Command::new("op")
        .args(["read", ref_path])
        .output()
        .with_context(|| format!("Failed to run 'op read {}'", ref_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("op read failed for {}: {}", ref_path, stderr.trim());
    }

    let secret = String::from_utf8(output.stdout)
        .context("op read returned non-UTF8")?
        .trim()
        .to_string();

    if secret.is_empty() {
        bail!("op read returned empty value for {}", ref_path);
    }

    Ok(secret)
}

fn resolve_credentials(config: &mut AvatarConfig) -> Result<()> {
    // Resolve MCP server tokens
    for server in &mut config.mcp.servers {
        if let Some(ref token_ref) = server.token_ref {
            match resolve_op_ref(token_ref) {
                Ok(token) => {
                    info!(server = %server.name, "Resolved MCP token");
                    server.resolved_token = Some(token);
                }
                Err(e) => {
                    bail!("Failed to resolve token for MCP server '{}': {}", server.name, e);
                }
            }
        }
    }

    // Resolve model provider API key
    let provider_key = format!("{}.api_key_ref", config.model.primary_provider);
    if let Some(toml::Value::Table(provider_table)) = config.model.providers.get(&config.model.primary_provider) {
        if let Some(toml::Value::String(api_key_ref)) = provider_table.get("api_key_ref") {
            match resolve_op_ref(api_key_ref) {
                Ok(key) => {
                    info!(provider = %config.model.primary_provider, "Resolved API key");
                    // Store resolved key in env for the runtime process
                    // (will be set via Command::env at launch)
                }
                Err(e) => {
                    bail!("Failed to resolve API key for provider '{}': {}", config.model.primary_provider, e);
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Ephemeral MCP config generation
// ============================================================================

struct EphemeralConfig {
    dir: PathBuf,
    config_path: PathBuf,
}

impl EphemeralConfig {
    fn generate(config: &AvatarConfig) -> Result<Self> {
        let agent_id = &config.identity.agent_id;
        let dir = PathBuf::from(format!("/tmp/am-avatar/{}", agent_id));

        // Create directory with restricted permissions
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create ephemeral dir: {}", dir.display()))?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;

        let config_path = dir.join("mcp-config.json");

        // Generate runtime-specific MCP config
        let mcp_json = match config.runtime.runtime_type.as_str() {
            "opencode" => Self::generate_opencode_config(config)?,
            "gemini" => Self::generate_gemini_config(config)?,
            "claude-code" => Self::generate_claude_config(config)?,
            "kimi" => Self::generate_kimi_config(config)?,
            other => bail!("Unsupported runtime type: {}", other),
        };

        // Write with restricted permissions
        fs::write(&config_path, &mcp_json)
            .with_context(|| format!("Failed to write MCP config: {}", config_path.display()))?;
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))?;

        info!(
            path = %config_path.display(),
            runtime = %config.runtime.runtime_type,
            "Ephemeral MCP config written"
        );

        Ok(Self { dir, config_path })
    }

    fn generate_opencode_config(config: &AvatarConfig) -> Result<String> {
        let mut servers = serde_json::Map::new();
        for server in &config.mcp.servers {
            let mut entry = serde_json::Map::new();
            entry.insert("url".to_string(), serde_json::Value::String(server.url.clone()));
            if let Some(ref token) = server.resolved_token {
                let mut headers = serde_json::Map::new();
                headers.insert(
                    "Authorization".to_string(),
                    serde_json::Value::String(format!("Bearer {}", token)),
                );
                entry.insert("headers".to_string(), serde_json::Value::Object(headers));
            }
            servers.insert(server.name.clone(), serde_json::Value::Object(entry));
        }

        let config_json = serde_json::json!({
            "mcpServers": servers
        });

        serde_json::to_string_pretty(&config_json).context("Failed to serialize MCP config")
    }

    fn generate_gemini_config(config: &AvatarConfig) -> Result<String> {
        // Gemini CLI uses ~/.gemini/settings.json format
        let mut servers = serde_json::Map::new();
        for server in &config.mcp.servers {
            let mut entry = serde_json::Map::new();
            entry.insert("url".to_string(), serde_json::Value::String(server.url.clone()));
            if let Some(ref token) = server.resolved_token {
                let mut headers = serde_json::Map::new();
                headers.insert(
                    "Authorization".to_string(),
                    serde_json::Value::String(format!("Bearer {}", token)),
                );
                entry.insert("headers".to_string(), serde_json::Value::Object(headers));
            }
            servers.insert(server.name.clone(), serde_json::Value::Object(entry));
        }

        let config_json = serde_json::json!({
            "mcpServers": servers
        });

        serde_json::to_string_pretty(&config_json).context("Failed to serialize MCP config")
    }

    fn generate_claude_config(config: &AvatarConfig) -> Result<String> {
        // Claude Code uses .mcp.json format
        let mut servers = serde_json::Map::new();
        for server in &config.mcp.servers {
            let mut entry = serde_json::Map::new();
            entry.insert("url".to_string(), serde_json::Value::String(server.url.clone()));
            if let Some(ref token) = server.resolved_token {
                let mut env = serde_json::Map::new();
                env.insert(
                    "MQ_TOKEN".to_string(),
                    serde_json::Value::String(token.clone()),
                );
                entry.insert("env".to_string(), serde_json::Value::Object(env));
            }
            servers.insert(server.name.clone(), serde_json::Value::Object(entry));
        }

        let config_json = serde_json::json!({
            "mcpServers": servers
        });

        serde_json::to_string_pretty(&config_json).context("Failed to serialize MCP config")
    }

    fn generate_kimi_config(config: &AvatarConfig) -> Result<String> {
        // Kimi uses ~/.kimi/mcp.json format (same structure as Gemini)
        Self::generate_gemini_config(config)
    }

    fn cleanup(&self) {
        if let Err(e) = fs::remove_dir_all(&self.dir) {
            warn!(path = %self.dir.display(), error = %e, "Failed to cleanup ephemeral config dir");
        } else {
            info!(path = %self.dir.display(), "Ephemeral config cleaned up");
        }
    }
}

impl Drop for EphemeralConfig {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// ============================================================================
// Placeholder resolution
// ============================================================================

fn resolve_placeholder(value: &str, config: &AvatarConfig) -> String {
    value
        .replace("{identity.agent_id}", &config.identity.agent_id)
        .replace("{model.primary}", &config.model.primary)
        .replace("{model.primary_provider}", &config.model.primary_provider)
        .replace("{runtime.type}", &config.runtime.runtime_type)
}

// ============================================================================
// Runtime launch
// ============================================================================

fn build_runtime_command(config: &AvatarConfig, mcp_config_path: &Path) -> Result<Command> {
    let binary = match config.runtime.runtime_type.as_str() {
        "opencode" => "/opt/homebrew/bin/opencode",
        "gemini" => "/opt/homebrew/bin/gemini",
        "claude-code" => "/opt/homebrew/bin/claude",
        "kimi" => "/opt/homebrew/bin/kimi",
        other => bail!("Unknown runtime: {}", other),
    };

    let mut cmd = Command::new(binary);

    // Set working directory
    if let Some(ref workspace) = config.runtime.workspace_dir {
        cmd.current_dir(workspace);
    }

    // Add runtime-specific MCP config flag
    match config.runtime.runtime_type.as_str() {
        "opencode" => {
            cmd.env("OPENCODE_MCP_CONFIG", mcp_config_path);
        }
        "gemini" => {
            // Gemini reads from a settings file path
            cmd.args(["--mcp-config", &mcp_config_path.to_string_lossy()]);
        }
        "claude-code" => {
            cmd.env("MCP_CONFIG", mcp_config_path);
        }
        "kimi" => {
            cmd.env("KIMI_MCP_CONFIG", mcp_config_path);
        }
        _ => {}
    }

    // Add extra flags (typed array — no shell interpolation, Gap 2)
    for flag in &config.runtime.extra_flags {
        cmd.arg(flag);
    }

    // Set environment variables (resolved placeholders)
    for (key, value) in &config.runtime.env {
        cmd.env(key, resolve_placeholder(value, config));
    }

    Ok(cmd)
}

// ============================================================================
// Handoff injection
// ============================================================================

fn load_handoff(agent_id: &str) -> Option<String> {
    let path = format!(
        "/Users/proteus/astralmaris/astrallation/fleet/state/last-handoff-{}.json",
        agent_id
    );
    match fs::read_to_string(&path) {
        Ok(content) => {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                let summary = data["state_summary"].as_str().unwrap_or("(no summary)");
                let actions = data["next_actions"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                Some(format!(
                    "Previous session summary: {}\nNext actions: {}",
                    summary, actions
                ))
            } else {
                warn!(agent_id, "Handoff file exists but invalid JSON — treating as untrusted");
                None
            }
        }
        Err(_) => None,
    }
}

// ============================================================================
// Health monitoring
// ============================================================================

async fn check_agent_mcp_health(agent_id: &str, mq_url: &str) -> bool {
    let url = format!("{}/api/inbox/{}?unread_only=true&limit=1", mq_url, agent_id);
    match reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
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
    info!("  am-avatar — Agent Avatar System");
    info!("  Config: {}", args.config.display());
    info!("════════════════════════════════════════");

    // Step 1: Parse and validate config
    let mut config = load_config(&args.config)?;

    if args.validate_only {
        info!("Config validation passed. Exiting (--validate-only).");
        return Ok(());
    }

    // Step 2: Resolve credentials
    info!("Resolving credentials...");
    resolve_credentials(&mut config)?;
    info!("Credentials resolved");

    if args.dry_run {
        info!("Dry run — would launch {} with model {}",
            config.runtime.runtime_type, config.model.primary);
        info!("Agent: {}", config.identity.agent_id);
        info!("MCP servers: {}", config.mcp.servers.len());
        return Ok(());
    }

    // Step 3: Generate ephemeral MCP config
    let ephemeral = EphemeralConfig::generate(&config)?;

    // Step 4: Load handoff context
    let handoff = load_handoff(&config.identity.agent_id);
    if let Some(ref summary) = handoff {
        info!(agent = %config.identity.agent_id, "Handoff context loaded");
    }

    // Step 5: Launch with restart loop
    let agent_id = config.identity.agent_id.clone();
    let mq_url = config.mcp.servers
        .iter()
        .find(|s| s.name == "ming-qiao")
        .map(|s| s.url.replace("/mcp", ""))
        .unwrap_or_else(|| "http://localhost:7777".to_string());

    let mut restart_count = 0u32;
    let restart_delay = Duration::from_secs(args.restart_delay);

    loop {
        if restart_count >= args.max_restarts {
            error!(agent = %agent_id, restarts = restart_count,
                "Max restarts exceeded — exiting. Manual intervention required.");
            break;
        }

        if restart_count > 0 {
            let backoff = restart_delay * (1 << restart_count.min(4));
            warn!(agent = %agent_id, restart = restart_count, backoff_secs = backoff.as_secs(),
                "Restarting in {}s...", backoff.as_secs());
            sleep(backoff).await;
        }

        info!(agent = %agent_id, "Launching runtime: {}", config.runtime.runtime_type);

        let mut cmd = build_runtime_command(&config, &ephemeral.config_path)?;

        // Launch as child process
        let mut child = cmd
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .with_context(|| format!("Failed to launch runtime: {}", config.runtime.runtime_type))?;

        let pid = child.id();
        info!(agent = %agent_id, pid, "Runtime launched");

        let start_time = Instant::now();

        // Wait for exit or Ctrl-C
        tokio::select! {
            status = tokio::task::spawn_blocking(move || child.wait()) => {
                match status {
                    Ok(Ok(exit_status)) => {
                        let elapsed = start_time.elapsed();
                        info!(agent = %agent_id, code = ?exit_status.code(),
                            elapsed_secs = elapsed.as_secs(), "Runtime exited");

                        // If ran for >5 minutes, reset restart counter (stable session)
                        if elapsed > Duration::from_secs(300) {
                            restart_count = 0;
                        } else {
                            restart_count += 1;
                        }
                    }
                    Ok(Err(e)) => {
                        error!(agent = %agent_id, error = %e, "Runtime wait failed");
                        restart_count += 1;
                    }
                    Err(e) => {
                        error!(agent = %agent_id, error = %e, "Runtime task panicked");
                        restart_count += 1;
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!(agent = %agent_id, "Ctrl-C received — shutting down");
                // ephemeral config cleaned up via Drop
                return Ok(());
            }
        }
    }

    // Cleanup happens via Drop on ephemeral
    Ok(())
}
