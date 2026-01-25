# Ming-Qiao Database Schema (SurrealDB)

**Database:** SurrealDB  
**Mode:** Embedded (file-based) or standalone  
**Location:** `data/surreal/`

---

## Overview

SurrealDB serves as a **materialized index** over the event log. It is not the source of truth — that remains `data/events.jsonl`. The database can be rebuilt at any time by replaying events.

---

## Connection

```rust
// Embedded mode (default)
let db = Surreal::new::<File>("data/surreal/ming-qiao.db").await?;

// Standalone mode (if running surrealdb server)
let db = Surreal::new::<Ws>("localhost:8000").await?;
```

---

## Schema Definition

Run on startup to ensure tables exist:

```surql
-- Namespace and database
DEFINE NAMESPACE ming_qiao;
USE NS ming_qiao;
DEFINE DATABASE bridge;
USE DB bridge;

-- ============================================
-- AGENTS
-- ============================================

DEFINE TABLE agent SCHEMAFULL;
DEFINE FIELD id ON agent TYPE string;
DEFINE FIELD name ON agent TYPE string;
DEFINE FIELD kind ON agent TYPE string;  -- 'builder', 'architect', 'human'
DEFINE FIELD color ON agent TYPE string;
DEFINE FIELD created_at ON agent TYPE datetime;

-- Seed agents
CREATE agent:aleph SET 
  name = 'Aleph',
  kind = 'builder',
  color = '#22c55e',
  created_at = time::now();

CREATE agent:thales SET 
  name = 'Thales',
  kind = 'architect',
  color = '#3b82f6',
  created_at = time::now();

CREATE agent:merlin SET 
  name = 'Merlin',
  kind = 'human',
  color = '#a855f7',
  created_at = time::now();

-- ============================================
-- THREADS
-- ============================================

DEFINE TABLE thread SCHEMAFULL;
DEFINE FIELD id ON thread TYPE string;
DEFINE FIELD subject ON thread TYPE string;
DEFINE FIELD participants ON thread TYPE array;
DEFINE FIELD status ON thread TYPE string;  -- 'active', 'paused', 'blocked', 'resolved', 'archived'
DEFINE FIELD started_by ON thread TYPE string;
DEFINE FIELD started_at ON thread TYPE datetime;
DEFINE FIELD last_message_at ON thread TYPE datetime;
DEFINE FIELD message_count ON thread TYPE int DEFAULT 0;
DEFINE FIELD decision_count ON thread TYPE int DEFAULT 0;

DEFINE INDEX thread_status ON thread FIELDS status;
DEFINE INDEX thread_last_message ON thread FIELDS last_message_at;

-- ============================================
-- MESSAGES
-- ============================================

DEFINE TABLE message SCHEMAFULL;
DEFINE FIELD id ON message TYPE string;
DEFINE FIELD thread ON message TYPE record(thread);
DEFINE FIELD from_agent ON message TYPE string;
DEFINE FIELD to_agent ON message TYPE string;
DEFINE FIELD subject ON message TYPE option<string>;
DEFINE FIELD content ON message TYPE string;
DEFINE FIELD content_sha256 ON message TYPE string;
DEFINE FIELD priority ON message TYPE string DEFAULT 'normal';
DEFINE FIELD sent_at ON message TYPE datetime;
DEFINE FIELD read_at ON message TYPE option<datetime>;
DEFINE FIELD artifact_refs ON message TYPE array DEFAULT [];
DEFINE FIELD context_refs ON message TYPE array DEFAULT [];

DEFINE INDEX message_thread ON message FIELDS thread;
DEFINE INDEX message_from ON message FIELDS from_agent;
DEFINE INDEX message_to ON message FIELDS to_agent;
DEFINE INDEX message_sent ON message FIELDS sent_at;
DEFINE INDEX message_unread ON message FIELDS to_agent, read_at;

-- Full-text search on content
DEFINE ANALYZER message_analyzer TOKENIZERS blank, class FILTERS lowercase, snowball(english);
DEFINE INDEX message_content_search ON message FIELDS content SEARCH ANALYZER message_analyzer;

-- ============================================
-- DECISIONS
-- ============================================

DEFINE TABLE decision SCHEMAFULL;
DEFINE FIELD id ON decision TYPE string;
DEFINE FIELD thread ON decision TYPE record(thread);
DEFINE FIELD question ON decision TYPE string;
DEFINE FIELD resolution ON decision TYPE string;
DEFINE FIELD rationale ON decision TYPE string;
DEFINE FIELD options_considered ON decision TYPE array DEFAULT [];
DEFINE FIELD decided_by ON decision TYPE string;
DEFINE FIELD approved_by ON decision TYPE option<string>;
DEFINE FIELD status ON decision TYPE string DEFAULT 'pending';  -- 'pending', 'approved', 'rejected', 'superseded'
DEFINE FIELD decided_at ON decision TYPE datetime;
DEFINE FIELD trace ON decision TYPE option<object>;

DEFINE INDEX decision_thread ON decision FIELDS thread;
DEFINE INDEX decision_status ON decision FIELDS status;
DEFINE INDEX decision_decided_at ON decision FIELDS decided_at;

-- Full-text search
DEFINE INDEX decision_question_search ON decision FIELDS question SEARCH ANALYZER message_analyzer;
DEFINE INDEX decision_rationale_search ON decision FIELDS rationale SEARCH ANALYZER message_analyzer;

-- ============================================
-- ARTIFACTS
-- ============================================

DEFINE TABLE artifact SCHEMAFULL;
DEFINE FIELD id ON artifact TYPE string;
DEFINE FIELD path ON artifact TYPE string;
DEFINE FIELD original_path ON artifact TYPE option<string>;
DEFINE FIELD shared_by ON artifact TYPE string;
DEFINE FIELD sha256 ON artifact TYPE string;
DEFINE FIELD bytes ON artifact TYPE int;
DEFINE FIELD content_type ON artifact TYPE string;
DEFINE FIELD description ON artifact TYPE option<string>;
DEFINE FIELD shared_at ON artifact TYPE datetime;

DEFINE INDEX artifact_path ON artifact FIELDS path UNIQUE;
DEFINE INDEX artifact_sha256 ON artifact FIELDS sha256;

-- ============================================
-- ANNOTATIONS
-- ============================================

DEFINE TABLE annotation SCHEMAFULL;
DEFINE FIELD id ON annotation TYPE string;
DEFINE FIELD target_type ON annotation TYPE string;  -- 'message', 'decision', 'thread'
DEFINE FIELD target_id ON annotation TYPE string;
DEFINE FIELD annotated_by ON annotation TYPE string;
DEFINE FIELD content ON annotation TYPE string;
DEFINE FIELD created_at ON annotation TYPE datetime;

DEFINE INDEX annotation_target ON annotation FIELDS target_type, target_id;

-- ============================================
-- EVENT TRACKING
-- ============================================

DEFINE TABLE event_cursor SCHEMAFULL;
DEFINE FIELD id ON event_cursor TYPE string;
DEFINE FIELD last_event_id ON event_cursor TYPE string;
DEFINE FIELD last_processed_at ON event_cursor TYPE datetime;

-- Single row to track indexing progress
CREATE event_cursor:main SET 
  last_event_id = '',
  last_processed_at = time::now();
```

---

## Common Queries

### Get inbox for agent

```surql
SELECT * FROM message
WHERE to_agent = $agent
  AND read_at IS NONE
ORDER BY sent_at DESC
LIMIT $limit;
```

### Get thread with messages

```surql
LET $thread = (SELECT * FROM thread WHERE id = $thread_id)[0];

LET $messages = SELECT * FROM message 
  WHERE thread = thread:[$thread_id]
  ORDER BY sent_at ASC;

LET $decisions = SELECT * FROM decision
  WHERE thread = thread:[$thread_id]
  ORDER BY decided_at ASC;

RETURN {
  thread: $thread,
  messages: $messages,
  decisions: $decisions
};
```

### List active threads

```surql
SELECT 
  *,
  (SELECT count() FROM message WHERE thread = $parent.id AND read_at IS NONE GROUP ALL)[0].count AS unread_count
FROM thread
WHERE status IN ['active', 'paused', 'blocked']
ORDER BY last_message_at DESC
LIMIT $limit;
```

### Search messages and decisions

```surql
-- Messages
SELECT *, search::score(1) AS score
FROM message
WHERE content @1@ $query
ORDER BY score DESC
LIMIT 10;

-- Decisions
SELECT *, search::score(1) AS score
FROM decision
WHERE question @1@ $query OR rationale @1@ $query
ORDER BY score DESC
LIMIT 10;
```

### Pending decisions

```surql
SELECT * FROM decision
WHERE status = 'pending'
ORDER BY decided_at ASC;
```

### Update thread stats

```surql
UPDATE thread:[$thread_id] SET
  message_count = (SELECT count() FROM message WHERE thread = thread:[$thread_id] GROUP ALL)[0].count,
  decision_count = (SELECT count() FROM decision WHERE thread = thread:[$thread_id] GROUP ALL)[0].count,
  last_message_at = (SELECT sent_at FROM message WHERE thread = thread:[$thread_id] ORDER BY sent_at DESC LIMIT 1)[0].sent_at;
```

---

## Indexer

The indexer reads events from `events.jsonl` and materializes them to SurrealDB.

### Indexer pseudocode

```rust
async fn index_events(db: &Surreal<Db>) -> Result<()> {
    // Get last processed event
    let cursor: Option<EventCursor> = db
        .query("SELECT * FROM event_cursor:main")
        .await?
        .take(0)?;
    
    let last_event_id = cursor
        .map(|c| c.last_event_id)
        .unwrap_or_default();
    
    // Read events from file
    let file = File::open("data/events.jsonl")?;
    let reader = BufReader::new(file);
    
    let mut found_last = last_event_id.is_empty();
    
    for line in reader.lines() {
        let event: Event = serde_json::from_str(&line?)?;
        
        // Skip until we find where we left off
        if !found_last {
            if event.event_id == last_event_id {
                found_last = true;
            }
            continue;
        }
        
        // Process event
        match event.event_type.as_str() {
            "thread_created" => index_thread_created(db, &event).await?,
            "message_sent" => index_message_sent(db, &event).await?,
            "message_read" => index_message_read(db, &event).await?,
            "decision_recorded" => index_decision_recorded(db, &event).await?,
            "decision_approved" => index_decision_approved(db, &event).await?,
            "decision_rejected" => index_decision_rejected(db, &event).await?,
            "thread_status_changed" => index_thread_status(db, &event).await?,
            "artifact_shared" => index_artifact(db, &event).await?,
            "annotation_added" => index_annotation(db, &event).await?,
            _ => {} // Skip unknown events
        }
        
        // Update cursor
        db.query("UPDATE event_cursor:main SET last_event_id = $id, last_processed_at = time::now()")
            .bind(("id", &event.event_id))
            .await?;
    }
    
    Ok(())
}
```

### Event handlers

```rust
async fn index_thread_created(db: &Surreal<Db>, event: &Event) -> Result<()> {
    db.query(r#"
        CREATE thread CONTENT {
            id: $thread_id,
            subject: $subject,
            participants: $participants,
            status: 'active',
            started_by: $started_by,
            started_at: $at,
            last_message_at: $at,
            message_count: 0,
            decision_count: 0
        }
    "#)
    .bind(("thread_id", &event.thread_id))
    .bind(("subject", &event.subject))
    .bind(("participants", &event.participants))
    .bind(("started_by", &event.started_by))
    .bind(("at", &event.at))
    .await?;
    
    Ok(())
}

async fn index_message_sent(db: &Surreal<Db>, event: &Event) -> Result<()> {
    // Create message
    db.query(r#"
        CREATE message CONTENT {
            id: $message_id,
            thread: type::thing('thread', $thread_id),
            from_agent: $from_agent,
            to_agent: $to_agent,
            subject: $subject,
            content: $content,
            content_sha256: $content_sha256,
            priority: $priority,
            sent_at: $at,
            read_at: NONE,
            artifact_refs: $artifact_refs,
            context_refs: $context_refs
        }
    "#)
    .bind(("message_id", &event.message_id))
    .bind(("thread_id", &event.thread_id))
    // ... bind other fields
    .await?;
    
    // Update thread stats
    update_thread_stats(db, &event.thread_id).await?;
    
    Ok(())
}
```

---

## Rebuild from scratch

To rebuild the database from events:

```bash
# Delete existing database
rm -rf data/surreal/

# Run indexer
ming-qiao index --full

# Or via CLI
ming-qiao db rebuild
```

The indexer will:
1. Create fresh database with schema
2. Reset event cursor
3. Process all events from `events.jsonl`

---

## Backup

Since the event log is the source of truth, backup strategy is simple:

```bash
# Backup events (primary)
cp data/events.jsonl backup/events-$(date +%Y%m%d).jsonl

# Optional: backup database for faster recovery
cp -r data/surreal/ backup/surreal-$(date +%Y%m%d)/
```

Restore:

```bash
# From events (always works)
cp backup/events-YYYYMMDD.jsonl data/events.jsonl
ming-qiao db rebuild

# Or from database snapshot (faster)
cp -r backup/surreal-YYYYMMDD/ data/surreal/
```
