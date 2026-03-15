//! ASTROLABE CLI — unified command-line tool for the ASTROLABE knowledge graph.
//!
//! Replaces: astrolabe-ingest.py, astrolabe-query.py
//!
//! Subcommands:
//!   ingest  — Add content to the knowledge graph via MCP
//!   query   — Search nodes and/or facts in the knowledge graph
//!   process — Process a raw artifact through the processor pipeline
//!
//! Agent: luban

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use astrolabe_toolkit::envelope::{self, EnvelopeBuilder};
use astrolabe_toolkit::ingest::AstrolabeClient;
use astrolabe_toolkit::processor::RawArtifact;
use astrolabe_toolkit::processors::ProcessorRegistry;

#[derive(Parser)]
#[command(name = "astrolabe", version, about = "ASTROLABE knowledge graph CLI")]
struct Cli {
    /// MCP server URL
    #[arg(long, default_value = "http://localhost:8001/mcp")]
    url: String,

    /// Graph group ID
    #[arg(long, default_value = "astrolabe_main")]
    group: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest content into the ASTROLABE knowledge graph
    Ingest {
        /// Title/name of the content
        #[arg(long)]
        name: String,

        /// Content body (use --file to read from file instead)
        #[arg(long, conflicts_with = "file")]
        body: Option<String>,

        /// Read content from file (use - for stdin)
        #[arg(long, conflicts_with = "body")]
        file: Option<PathBuf>,

        /// Content format (text, json, message)
        #[arg(long, default_value = "text")]
        format: String,

        /// Source description
        #[arg(long, default_value = "")]
        source_description: String,
    },

    /// Search nodes and/or facts in the knowledge graph
    Query {
        /// Natural language search query
        query: String,

        /// Search nodes only
        #[arg(long)]
        nodes: bool,

        /// Search facts only
        #[arg(long)]
        facts: bool,

        /// Maximum results to show
        #[arg(long, default_value = "10")]
        max: usize,

        /// Output raw JSON (for scripting)
        #[arg(long)]
        json: bool,
    },

    /// Process a raw artifact through the pipeline
    Process {
        /// Source type classification (e.g., arxiv_url, x_post, web_url)
        #[arg(long)]
        source_type: String,

        /// Artifact subject line
        #[arg(long)]
        subject: String,

        /// Artifact body content
        #[arg(long, default_value = "")]
        body: String,

        /// Output directory for quarantine envelope
        #[arg(long)]
        quarantine_dir: Option<PathBuf>,

        /// Also ingest into ASTROLABE (skip quarantine)
        #[arg(long)]
        ingest: bool,
    },
}

fn connect_client(url: &str) -> AstrolabeClient {
    let mut client = AstrolabeClient::new(Some(url));
    if let Err(e) = client.connect() {
        eprintln!("Failed to connect to MCP at {url}: {e}");
        std::process::exit(1);
    }
    client
}

fn format_nodes(data: &serde_json::Value, max: usize) {
    let nodes = match data.as_array() {
        Some(arr) => arr,
        None => {
            println!("No nodes found.");
            return;
        }
    };

    let total = nodes.len();
    let showing = total.min(max);
    println!("{}", "=".repeat(60));
    println!("Found {total} nodes (showing {showing}):");
    println!("{}", "=".repeat(60));

    for node in nodes.iter().take(max) {
        let name = node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let labels = node
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let uuid = node
            .get("uuid")
            .and_then(|v| v.as_str())
            .map(|u| &u[..8.min(u.len())])
            .unwrap_or("");
        let summary = node
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let created = node
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        println!();
        if labels.is_empty() {
            println!("  {name} ({uuid})");
        } else {
            println!("  {name} [{labels}] ({uuid})");
        }
        if !summary.is_empty() {
            println!("     {summary}");
        }
        if !created.is_empty() {
            println!("     Created: {created}");
        }
    }

    if total > max {
        println!();
        println!("... and {} more nodes", total - max);
        println!("Use --max {total} to see all");
    }
}

fn format_facts(data: &serde_json::Value, max: usize) {
    let facts = match data.as_array() {
        Some(arr) => arr,
        None => {
            println!("No facts found.");
            return;
        }
    };

    let total = facts.len();
    let showing = total.min(max);
    println!("{}", "=".repeat(60));
    println!("Found {total} facts (showing {showing}):");
    println!("{}", "=".repeat(60));

    for fact in facts.iter().take(max) {
        let name = fact
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let description = fact
            .get("fact")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let uuid = fact
            .get("uuid")
            .and_then(|v| v.as_str())
            .map(|u| &u[..8.min(u.len())])
            .unwrap_or("");
        let created = fact
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let invalid = fact.get("invalid_at").and_then(|v| v.as_str());
        let valid_mark = if invalid.is_some() {
            " [invalid]"
        } else {
            ""
        };

        println!();
        println!("  {name} ({uuid}){valid_mark}");
        if !description.is_empty() {
            println!("     {description}");
        }
        if !created.is_empty() {
            println!("     Created: {created}");
        }
    }

    if total > max {
        println!();
        println!("... and {} more facts", total - max);
        println!("Use --max {total} to see all");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Ingest {
            name,
            body,
            file,
            format,
            source_description,
        } => {
            let content = match (body, file) {
                (Some(b), _) => b,
                (_, Some(path)) => {
                    if path.to_str() == Some("-") {
                        use std::io::Read;
                        let mut buf = String::new();
                        std::io::stdin()
                            .read_to_string(&mut buf)
                            .unwrap_or_else(|e| {
                                eprintln!("Error reading stdin: {e}");
                                std::process::exit(1);
                            });
                        buf
                    } else {
                        std::fs::read_to_string(&path).unwrap_or_else(|e| {
                            eprintln!("Error reading {}: {e}", path.display());
                            std::process::exit(1);
                        })
                    }
                }
                (None, None) => {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .unwrap_or_else(|e| {
                            eprintln!("Error reading stdin: {e}");
                            std::process::exit(1);
                        });
                    buf
                }
            };

            let client = connect_client(&cli.url);
            match client.ingest(
                &name,
                &content,
                &format,
                &source_description,
                Some(&cli.group),
            ) {
                Ok(result) => {
                    if result.is_error {
                        eprintln!("Ingest error: {}", result.data);
                        std::process::exit(1);
                    }
                    println!("Ingested: {name}");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&result.data).unwrap_or_default()
                    );
                }
                Err(e) => {
                    eprintln!("Ingest failed: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Query {
            query,
            nodes,
            facts,
            max,
            json,
        } => {
            // Default: search both unless one flag is specified
            let search_nodes = !facts || nodes;
            let search_facts = !nodes || facts;

            let client = connect_client(&cli.url);

            let mut json_output = serde_json::Map::new();

            if search_nodes {
                match client.search_nodes(&query, Some(&cli.group), Some(max)) {
                    Ok(result) => {
                        if json {
                            json_output.insert("nodes".to_string(), result.data.clone());
                        } else {
                            format_nodes(&result.data, max);
                        }
                    }
                    Err(e) => {
                        eprintln!("Node search failed: {e}");
                    }
                }
            }

            if search_facts {
                match client.search_facts(&query, Some(&cli.group), Some(max)) {
                    Ok(result) => {
                        if json {
                            json_output.insert("facts".to_string(), result.data.clone());
                        } else {
                            println!();
                            format_facts(&result.data, max);
                        }
                    }
                    Err(e) => {
                        eprintln!("Facts search failed: {e}");
                    }
                }
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::Value::Object(json_output))
                        .unwrap_or_default()
                );
            }
        }

        Commands::Process {
            source_type,
            subject,
            body,
            quarantine_dir,
            ingest,
        } => {
            let artifact = RawArtifact {
                source_type,
                subject,
                body,
                message_id: String::new(),
                date: String::new(),
                attachments: vec![],
                metadata: std::collections::HashMap::new(),
            };

            let registry = ProcessorRegistry::new();
            let result = match registry.process(&artifact) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Processing failed: {e}");
                    std::process::exit(1);
                }
            };

            if !result.is_valid() {
                eprintln!("Warning: processor output failed validation");
            }

            let env = envelope::build_envelope(&result);
            println!("Processed: {}", env.title);
            println!("Envelope ID: {}", env.envelope_id);
            println!("Hash: {}", env.post_sanitization_hash);

            if let Some(dir) = quarantine_dir {
                match envelope::write_to_quarantine(&env, &dir) {
                    Ok(path) => println!("Quarantined: {}", path.display()),
                    Err(e) => eprintln!("Quarantine write failed: {e}"),
                }
            }

            if ingest {
                let args = EnvelopeBuilder::build_ingest_args(&env);
                let client = connect_client(&cli.url);
                match client.ingest(
                    &args.name,
                    &args.episode_body,
                    &args.source,
                    &args.source_description,
                    Some(&args.group_id),
                ) {
                    Ok(r) => {
                        if r.is_error {
                            eprintln!("Ingest error: {}", r.data);
                            std::process::exit(1);
                        }
                        println!("Ingested to ASTROLABE");
                    }
                    Err(e) => {
                        eprintln!("Ingest failed: {e}");
                        std::process::exit(1);
                    }
                }
            }

            println!(
                "\n{}",
                serde_json::to_string_pretty(&env).unwrap_or_default()
            );
        }
    }
}
