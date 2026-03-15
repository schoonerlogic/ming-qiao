//! ASTROLABE CLI — unified command-line tool for the ASTROLABE knowledge graph.
//!
//! Replaces: astrolabe-ingest.py, gmail-ingest.py (partially)
//!
//! Subcommands:
//!   ingest  — Add content to the knowledge graph via MCP
//!   search  — Search nodes in the knowledge graph
//!   facts   — Search facts/relationships in the knowledge graph
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
    mcp_url: String,

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

        /// Read content from file
        #[arg(long, conflicts_with = "body")]
        file: Option<PathBuf>,

        /// Source type (text, json, message)
        #[arg(long, default_value = "text")]
        source: String,

        /// Source description
        #[arg(long, default_value = "")]
        source_description: String,
    },

    /// Search nodes in the knowledge graph
    Search {
        /// Search query
        query: String,
    },

    /// Search facts/relationships in the knowledge graph
    Facts {
        /// Search query
        query: String,
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
            source,
            source_description,
        } => {
            let content = match (body, file) {
                (Some(b), _) => b,
                (_, Some(path)) => {
                    std::fs::read_to_string(&path).unwrap_or_else(|e| {
                        eprintln!("Error reading {}: {e}", path.display());
                        std::process::exit(1);
                    })
                }
                (None, None) => {
                    // Read from stdin
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                        eprintln!("Error reading stdin: {e}");
                        std::process::exit(1);
                    });
                    buf
                }
            };

            let mut client = AstrolabeClient::new(Some(&cli.mcp_url));
            if let Err(e) = client.connect() {
                eprintln!("Failed to connect to MCP: {e}");
                std::process::exit(1);
            }

            match client.ingest(&name, &content, &source, &source_description, Some(&cli.group)) {
                Ok(result) => {
                    if result.is_error {
                        eprintln!("Ingest error: {}", result.data);
                        std::process::exit(1);
                    }
                    println!("Ingested: {name}");
                    println!("{}", serde_json::to_string_pretty(&result.data).unwrap_or_default());
                }
                Err(e) => {
                    eprintln!("Ingest failed: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Search { query } => {
            let mut client = AstrolabeClient::new(Some(&cli.mcp_url));
            if let Err(e) = client.connect() {
                eprintln!("Failed to connect to MCP: {e}");
                std::process::exit(1);
            }

            match client.search_nodes(&query, Some(&cli.group)) {
                Ok(result) => {
                    println!("{}", serde_json::to_string_pretty(&result.data).unwrap_or_default());
                }
                Err(e) => {
                    eprintln!("Search failed: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Facts { query } => {
            let mut client = AstrolabeClient::new(Some(&cli.mcp_url));
            if let Err(e) = client.connect() {
                eprintln!("Failed to connect to MCP: {e}");
                std::process::exit(1);
            }

            match client.search_facts(&query, Some(&cli.group)) {
                Ok(result) => {
                    println!("{}", serde_json::to_string_pretty(&result.data).unwrap_or_default());
                }
                Err(e) => {
                    eprintln!("Facts search failed: {e}");
                    std::process::exit(1);
                }
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

            // Build envelope
            let env = envelope::build_envelope(&result);
            println!("Processed: {}", env.title);
            println!("Envelope ID: {}", env.envelope_id);
            println!("Hash: {}", env.post_sanitization_hash);

            // Write to quarantine if dir specified
            if let Some(dir) = quarantine_dir {
                match envelope::write_to_quarantine(&env, &dir) {
                    Ok(path) => println!("Quarantined: {}", path.display()),
                    Err(e) => eprintln!("Quarantine write failed: {e}"),
                }
            }

            // Optionally ingest directly
            if ingest {
                let args = EnvelopeBuilder::build_ingest_args(&env);
                let mut client = AstrolabeClient::new(Some(&cli.mcp_url));
                if let Err(e) = client.connect() {
                    eprintln!("Failed to connect to MCP: {e}");
                    std::process::exit(1);
                }

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

            // Print envelope JSON
            println!("\n{}", serde_json::to_string_pretty(&env).unwrap_or_default());
        }
    }
}
