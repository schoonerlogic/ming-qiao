//! am-comms — Human CLI client for the ming-qiao network
//!
//! A lightweight, standalone terminal app for Proteus/Merlin to send and
//! receive messages on the ming-qiao network. Runs OUTSIDE cmux in its
//! own terminal — immune to agent stdout focus-stealing.
//!
//! Features:
//!   - Interactive prompt loop (rustyline) for composing messages
//!   - Background inbox polling with desktop notification on new messages
//!   - Thread viewing, reply, and send commands
//!   - Zero agent management, zero logs, clean TTY
//!
//! Usage:
//!   am-comms                          # Interactive mode as merlin
//!   am-comms --agent proteus          # Interactive mode as proteus
//!   am-comms send <to> <subject>      # One-shot send (reads body from stdin)

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde::Deserialize;
use tokio::sync::Mutex;

// ============================================================================
// CLI
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "am-comms", about = "Human CLI client for the ming-qiao network")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Agent identity
    #[arg(long, default_value = "merlin")]
    agent: String,

    /// Ming-qiao API URL
    #[arg(long, default_value = "http://localhost:7777")]
    url: String,

    /// Bearer token (auto-loaded from token file if not set)
    #[arg(long)]
    token: Option<String>,

    /// Token file path
    #[arg(long, default_value = "/Users/proteus/astralmaris/ming-qiao/main/config/agent-tokens.json")]
    tokens_file: PathBuf,

    /// Poll interval for inbox checking (seconds)
    #[arg(long, default_value_t = 15)]
    poll: u64,

    /// Disable background polling
    #[arg(long)]
    no_poll: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Send a message (one-shot, reads body from stdin)
    Send {
        /// Recipient agent
        to: String,
        /// Subject line
        subject: String,
        /// Intent
        #[arg(long, default_value = "discuss")]
        intent: String,
    },
}

// ============================================================================
// API types
// ============================================================================

#[derive(Debug, Deserialize)]
struct InboxResponse {
    #[serde(default)]
    unread_count: usize,
    #[serde(default)]
    messages: Vec<InboxMessage>,
}

#[derive(Debug, Deserialize, Clone)]
struct InboxMessage {
    #[serde(alias = "message_id")]
    id: Option<String>,
    #[serde(alias = "from_agent")]
    from: Option<String>,
    subject: Option<String>,
    content: Option<String>,
    intent: Option<String>,
    thread_id: Option<String>,
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadResponse {
    subject: Option<String>,
    thread_id: Option<String>,
    messages: Vec<InboxMessage>,
}

// ============================================================================
// State
// ============================================================================

struct AppState {
    client: reqwest::Client,
    url: String,
    agent: String,
    token: String,
    seen_ids: Mutex<HashSet<String>>,
}

// ============================================================================
// API calls
// ============================================================================

async fn fetch_inbox(state: &AppState) -> Option<InboxResponse> {
    let url = format!("{}/api/inbox/{}", state.url, state.agent);
    let resp = state
        .client
        .get(&url)
        .header("Authorization", format!("Bearer {}", state.token))
        .send()
        .await
        .ok()?;
    resp.json().await.ok()
}

async fn fetch_thread(state: &AppState, thread_id: &str) -> Option<ThreadResponse> {
    let url = format!("{}/api/thread/{}", state.url, thread_id);
    let resp = state
        .client
        .get(&url)
        .header("Authorization", format!("Bearer {}", state.token))
        .send()
        .await
        .ok()?;
    resp.json().await.ok()
}

async fn send_message(
    state: &AppState,
    to: &str,
    subject: &str,
    content: &str,
    intent: &str,
) -> Result<String, String> {
    let url = format!("{}/api/threads", state.url);
    let body = serde_json::json!({
        "from_agent": state.agent,
        "to_agent": to,
        "subject": subject,
        "content": content,
        "intent": intent,
    });

    let resp = state
        .client
        .post(&url)
        .header("Authorization", format!("Bearer {}", state.token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("send failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {}", e))?;
    Ok(data["thread_id"]
        .as_str()
        .unwrap_or("?")
        .to_string())
}

async fn reply_to_thread(
    state: &AppState,
    thread_id: &str,
    content: &str,
    intent: &str,
) -> Result<(), String> {
    let url = format!("{}/api/thread/{}/reply", state.url, thread_id);
    let body = serde_json::json!({
        "from_agent": state.agent,
        "content": content,
        "intent": intent,
    });

    let resp = state
        .client
        .post(&url)
        .header("Authorization", format!("Bearer {}", state.token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("reply failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(())
}

// ============================================================================
// Display helpers
// ============================================================================

fn print_inbox(inbox: &InboxResponse) {
    if inbox.messages.is_empty() {
        println!("  (empty)");
        return;
    }
    println!(
        "  {:<4} {:<14} {:<10} {}",
        "#", "FROM", "INTENT", "SUBJECT"
    );
    println!("  {}", "-".repeat(70));
    for (i, msg) in inbox.messages.iter().enumerate() {
        let from = msg.from.as_deref().unwrap_or("?");
        let subject = msg.subject.as_deref().unwrap_or("(no subject)");
        let intent = msg.intent.as_deref().unwrap_or("?");
        let time = msg
            .created_at
            .as_deref()
            .unwrap_or("")
            .get(11..16)
            .unwrap_or("");
        println!(
            "  {:<4} {:<14} {:<10} {} [{}]",
            i + 1,
            from,
            intent,
            truncate(subject, 40),
            time
        );
    }
}

fn print_thread(thread: &ThreadResponse) {
    let subject = thread.subject.as_deref().unwrap_or("(no subject)");
    println!();
    println!("  Thread: {}", subject);
    println!("  {}", "=".repeat(60));
    for msg in &thread.messages {
        let from = msg.from.as_deref().unwrap_or("?");
        let time = msg.created_at.as_deref().unwrap_or("");
        let content = msg.content.as_deref().unwrap_or("");
        println!();
        println!("  [{}] {} :", time.get(..19).unwrap_or(time), from);
        for line in content.lines() {
            println!("    {}", line);
        }
    }
    println!();
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn print_help() {
    println!();
    println!("  Commands:");
    println!("    inbox              Show inbox");
    println!("    read <#>           Read thread for message #");
    println!("    send <to> <subj>   Compose and send a message");
    println!("    reply <#>          Reply to message # thread");
    println!("    threads            List recent threads");
    println!("    help               Show this help");
    println!("    quit               Exit");
    println!();
}

// ============================================================================
// Background poller
// ============================================================================

async fn poll_loop(state: Arc<AppState>, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;

        if let Some(inbox) = fetch_inbox(&state).await {
            let mut seen = state.seen_ids.lock().await;
            let mut new_count = 0;
            for msg in &inbox.messages {
                if let Some(id) = &msg.id {
                    if seen.insert(id.clone()) {
                        new_count += 1;
                    }
                }
            }
            if new_count > 0 {
                let from = inbox
                    .messages
                    .last()
                    .and_then(|m| m.from.as_deref())
                    .unwrap_or("?");
                let subject = inbox
                    .messages
                    .last()
                    .and_then(|m| m.subject.as_deref())
                    .unwrap_or("?");
                // Ring terminal bell + print notification
                eprint!("\x07");
                eprintln!(
                    "\n  ** {} new message(s) — latest from {} re: {} **\n",
                    new_count, from, truncate(subject, 50)
                );
            }
        }
    }
}

// ============================================================================
// Token loading
// ============================================================================

fn load_token(args: &Args) -> String {
    if let Some(t) = &args.token {
        return t.clone();
    }
    if let Ok(content) = std::fs::read_to_string(&args.tokens_file) {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(t) = data["tokens"][&args.agent].as_str() {
                return t.to_string();
            }
        }
    }
    String::new()
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let token = load_token(&args);

    if token.is_empty() {
        eprintln!("Warning: no bearer token found for '{}'. Some operations may fail.", args.agent);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let state = Arc::new(AppState {
        client,
        url: args.url.clone(),
        agent: args.agent.clone(),
        token: token.clone(),
        seen_ids: Mutex::new(HashSet::new()),
    });

    // One-shot send mode
    if let Some(Commands::Send {
        to,
        subject,
        intent,
    }) = &args.command
    {
        let mut body = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut body)?;
        match send_message(&state, to, subject, &body, intent).await {
            Ok(tid) => {
                println!("Sent to {}: {} (thread: {})", to, subject, tid);
            }
            Err(e) => {
                eprintln!("ERROR: {}", e);
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // Interactive mode
    println!();
    println!("  am-comms — {} @ {}", args.agent, args.url);
    println!("  Type 'help' for commands, 'quit' to exit.");
    println!();

    // Seed seen_ids from current inbox
    if let Some(inbox) = fetch_inbox(&state).await {
        let mut seen = state.seen_ids.lock().await;
        for msg in &inbox.messages {
            if let Some(id) = &msg.id {
                seen.insert(id.clone());
            }
        }
        println!("  Inbox: {} message(s), {} unread", inbox.messages.len(), inbox.unread_count);
    }
    println!();

    // Start background poller
    if !args.no_poll {
        let poll_state = state.clone();
        let poll_interval = Duration::from_secs(args.poll);
        tokio::spawn(async move {
            poll_loop(poll_state, poll_interval).await;
        });
    }

    // Cached inbox for # references
    let last_inbox: Arc<Mutex<Vec<InboxMessage>>> = Arc::new(Mutex::new(Vec::new()));

    let mut rl = DefaultEditor::new()?;
    let history_path = dirs_home().join(".am-comms-history");
    let _ = rl.load_history(&history_path);

    loop {
        let prompt = format!("[{}] > ", args.agent);
        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                rl.add_history_entry(&line)?;

                let parts: Vec<&str> = line.splitn(3, ' ').collect();
                let cmd = parts[0].to_lowercase();

                match cmd.as_str() {
                    "quit" | "exit" | "q" => break,
                    "help" | "?" => print_help(),

                    "inbox" | "i" => {
                        if let Some(inbox) = fetch_inbox(&state).await {
                            let mut cached = last_inbox.lock().await;
                            *cached = inbox.messages.clone();
                            print_inbox(&inbox);
                        } else {
                            println!("  (failed to fetch inbox)");
                        }
                    }

                    "read" | "r" => {
                        if parts.len() < 2 {
                            println!("  Usage: read <#>");
                            continue;
                        }
                        let idx: usize = match parts[1].parse::<usize>() {
                            Ok(n) if n > 0 => n - 1,
                            _ => {
                                println!("  Invalid message number");
                                continue;
                            }
                        };
                        let cached = last_inbox.lock().await;
                        if let Some(msg) = cached.get(idx) {
                            if let Some(tid) = &msg.thread_id {
                                if let Some(thread) = fetch_thread(&state, tid).await {
                                    print_thread(&thread);
                                } else {
                                    println!("  (failed to fetch thread)");
                                }
                            } else {
                                println!("  (no thread_id for this message)");
                            }
                        } else {
                            println!("  No message #{}. Run 'inbox' first.", idx + 1);
                        }
                    }

                    "reply" | "re" => {
                        if parts.len() < 2 {
                            println!("  Usage: reply <#>");
                            continue;
                        }
                        let idx: usize = match parts[1].parse::<usize>() {
                            Ok(n) if n > 0 => n - 1,
                            _ => {
                                println!("  Invalid message number");
                                continue;
                            }
                        };
                        let cached = last_inbox.lock().await;
                        let tid = cached
                            .get(idx)
                            .and_then(|m| m.thread_id.clone());
                        drop(cached);

                        if let Some(tid) = tid {
                            println!("  Compose reply (end with '.' on empty line):");
                            let mut body = String::new();
                            loop {
                                match rl.readline("  | ") {
                                    Ok(l) if l.trim() == "." => break,
                                    Ok(l) => {
                                        body.push_str(&l);
                                        body.push('\n');
                                    }
                                    Err(_) => break,
                                }
                            }
                            if !body.trim().is_empty() {
                                match reply_to_thread(&state, &tid, body.trim(), "discuss").await {
                                    Ok(_) => println!("  Replied to thread {}", &tid[..8]),
                                    Err(e) => println!("  ERROR: {}", e),
                                }
                            } else {
                                println!("  (empty reply, cancelled)");
                            }
                        } else {
                            println!("  No thread for message #{}. Run 'inbox' first.", idx + 1);
                        }
                    }

                    "send" | "s" => {
                        if parts.len() < 3 {
                            println!("  Usage: send <to> <subject>");
                            continue;
                        }
                        let to = parts[1];
                        let subject = parts[2];
                        println!("  Compose message (end with '.' on empty line):");
                        let mut body = String::new();
                        loop {
                            match rl.readline("  | ") {
                                Ok(l) if l.trim() == "." => break,
                                Ok(l) => {
                                    body.push_str(&l);
                                    body.push('\n');
                                }
                                Err(_) => break,
                            }
                        }
                        if !body.trim().is_empty() {
                            match send_message(&state, to, subject, body.trim(), "discuss").await {
                                Ok(tid) => println!("  Sent to {} (thread: {})", to, &tid[..8.min(tid.len())]),
                                Err(e) => println!("  ERROR: {}", e),
                            }
                        } else {
                            println!("  (empty message, cancelled)");
                        }
                    }

                    "threads" | "t" => {
                        let url = format!("{}/api/threads", state.url);
                        if let Ok(resp) = state
                            .client
                            .get(&url)
                            .header("Authorization", format!("Bearer {}", state.token))
                            .send()
                            .await
                        {
                            if let Ok(data) = resp.json::<serde_json::Value>().await {
                                if let Some(threads) = data.as_array() {
                                    println!("  Recent threads ({}):", threads.len().min(20));
                                    for t in threads.iter().take(20) {
                                        let subject = t["subject"].as_str().unwrap_or("?");
                                        let tid = t["thread_id"].as_str().unwrap_or("?");
                                        println!("    {} {}", &tid[..8.min(tid.len())], truncate(subject, 55));
                                    }
                                }
                            }
                        }
                    }

                    _ => println!("  Unknown command: '{}'. Type 'help'.", cmd),
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("  (Ctrl-C — type 'quit' to exit)");
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("  Error: {}", e);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    println!("  Goodbye.");
    Ok(())
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}
