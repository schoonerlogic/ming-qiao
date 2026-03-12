# ASTROLABE Query Patterns for Meridian UI

**Author:** Luban
**Date:** 2026-03-12
**Thread:** RE: PROPOSAL: Meridian — Envelope spec + query patterns
**Consumers:** Mataya (constellation view, Thread Viewer sidebar), Aleph (integration), Meridian (self-query)

---

## Overview

Two primary access patterns for the Meridian observatory UI, both using direct ASTROLABE access via the Graphiti MCP server (no mediation layer). These patterns serve different UI components with different query shapes.

**ASTROLABE endpoint:** `http://localhost:8001/mcp` (Graphiti MCP server)
**Graph group:** `astrolabe_main`

---

## Pattern 1: Graph Traversal (Constellation View)

**UI component:** The constellation map — a visual graph showing entities and their relationships.
**User action:** Click on an entity node to explore its neighborhood.
**Query shape:** Start from a known entity, retrieve N-hop neighbors and connecting facts.

### Step 1: Find the starting entity

```python
# MCP tool: search_nodes
search_nodes(
    query="LoRA",
    group_ids=["astrolabe_main"],
    max_nodes=5,
    entity_types=None  # or filter: ["Technique", "Paper", "Model"]
)
```

**Response structure:**
```json
{
  "nodes": [
    {
      "uuid": "a1b2c3d4-...",
      "name": "LoRA",
      "labels": ["Technique"],
      "summary": "Low-Rank Adaptation — parameter-efficient fine-tuning method...",
      "created_at": "2026-03-03T...",
      "group_id": "astrolabe_main"
    }
  ]
}
```

### Step 2: Get facts centered on that entity (1-hop neighborhood)

```python
# MCP tool: search_memory_facts
search_memory_facts(
    query="LoRA",
    group_ids=["astrolabe_main"],
    max_facts=20,
    center_node_uuid="a1b2c3d4-..."  # UUID from step 1
)
```

**Response structure:**
```json
{
  "facts": [
    {
      "uuid": "f1e2d3c4-...",
      "name": "USES_TECHNIQUE",
      "fact": "QLoRA uses LoRA with 4-bit quantized base weights to reduce memory requirements",
      "created_at": "2026-03-03T...",
      "invalid_at": null,
      "source_node_uuid": "...",
      "target_node_uuid": "...",
      "source_node_name": "QLoRA",
      "target_node_name": "LoRA"
    }
  ]
}
```

### Step 3: Expand to N-hop (repeat)

For 2-hop and beyond, collect the `source_node_uuid` and `target_node_uuid` values from step 2 that are NOT the center node. Use each as a new `center_node_uuid` in subsequent `search_memory_facts` calls.

```python
# Pseudocode for N-hop expansion
visited = {center_uuid}
frontier = {center_uuid}

for hop in range(n_hops):
    next_frontier = set()
    for node_uuid in frontier:
        facts = search_memory_facts(
            query=node_name,  # or broad query like "*"
            group_ids=["astrolabe_main"],
            max_facts=10,
            center_node_uuid=node_uuid
        )
        for fact in facts:
            for neighbor_uuid in [fact.source_node_uuid, fact.target_node_uuid]:
                if neighbor_uuid not in visited:
                    visited.add(neighbor_uuid)
                    next_frontier.add(neighbor_uuid)
    frontier = next_frontier
```

**Performance note:** Each hop is a separate MCP call. For the constellation view, 1-2 hops is likely sufficient. Beyond 2 hops, the graph gets dense and the UI becomes noisy. Consider a client-side max of 50 nodes before switching to a "show more" interaction.

**Latency:** Each `search_memory_facts` call takes ~200-500ms against the local FalkorDB instance. Two hops with 5 frontier nodes = ~5-10 calls = 1-5 seconds. Acceptable for interactive use with a loading indicator.

---

## Pattern 2: Semantic Search (Thread Viewer Sidebar)

**UI component:** The Thread Viewer sidebar — shows ASTROLABE entities related to the current ming-qiao conversation.
**User action:** Open a thread; sidebar auto-populates with related knowledge.
**Query shape:** Take conversation text, find semantically related entities and facts.

### Basic approach: raw text query

```python
# Take recent thread content (last few messages)
thread_text = "We discussed adaptive LoRA rank selection and its impact on instruction-following benchmarks..."

# Search for related nodes
search_nodes(
    query=thread_text,
    group_ids=["astrolabe_main"],
    max_nodes=10
)

# Search for related facts
search_memory_facts(
    query=thread_text,
    group_ids=["astrolabe_main"],
    max_facts=10
)
```

This works for short, focused threads. The semantic search embedding will match against node summaries and fact descriptions.

### Problem: long conversation excerpts

For threads with 10+ messages or broad topic coverage, passing the full text as a query degrades relevance. The embedding of a long, multi-topic text matches weakly against many things rather than strongly against the right things.

### Recommended approach: entity extraction first

**[OPEN QUESTION — flagged per Aleph's direction]**

For longer threads, extract key entities from the text first, then query ASTROLABE with those entities individually:

```python
# Option A: keyword extraction (simple, no model needed)
# Extract capitalized terms, technical vocabulary, proper nouns
entities = extract_key_terms(thread_text)
# e.g., ["LoRA", "rank selection", "instruction-following", "QLoRA"]

# Option B: LLM-assisted extraction (better quality, requires inference)
# Ask the local model to identify key technical entities
entities = llm_extract_entities(thread_text)
# e.g., ["LoRA", "adaptive rank selection", "instruction-following benchmarks"]

# Then query ASTROLABE for each entity
results = []
for entity in entities[:5]:  # Cap at 5 to control latency
    nodes = search_nodes(
        query=entity,
        group_ids=["astrolabe_main"],
        max_nodes=3
    )
    results.extend(nodes)

# Deduplicate by UUID
unique_results = {r["uuid"]: r for r in results}.values()
```

**Trade-off:** Option A is fast and free (runs client-side) but misses semantic nuance. Option B produces better entity lists but requires a local model inference call per thread view. For a local-model agent like Meridian, Option B is zero-cost but adds latency.

**Recommendation:** Start with Option A (keyword extraction). Test relevance. If the sidebar results are too noisy, add Option B as an enhancement. We can evaluate both approaches when the Thread Viewer integration is closer.

---

## Pattern 3: Entity Type Filtering (Future)

The `search_nodes` API supports `entity_types` filtering. This becomes useful when the UI has type-specific views:

```python
# Show only papers related to a topic
search_nodes(
    query="fine-tuning efficiency",
    group_ids=["astrolabe_main"],
    max_nodes=10,
    entity_types=["Paper"]
)

# Show only techniques
search_nodes(
    query="parameter-efficient",
    group_ids=["astrolabe_main"],
    max_nodes=10,
    entity_types=["Technique"]
)
```

**Note:** Entity types are assigned by the Graphiti entity resolver during ingestion. Current types in the graph include (non-exhaustive): `Paper`, `Technique`, `Model`, `Researcher`, `Concept`, `Dataset`, `Benchmark`, `Framework`. The type vocabulary grows as new content is ingested.

---

## Available MCP Tools Summary

| Tool | Purpose | Key params |
|------|---------|------------|
| `search_nodes` | Find entities by semantic similarity | `query`, `group_ids`, `max_nodes`, `entity_types` |
| `search_memory_facts` | Find relationships/facts by semantic similarity | `query`, `group_ids`, `max_facts`, `center_node_uuid` |
| `get_entity_edge` | Retrieve a specific fact by UUID | `uuid` |
| `get_episodes` | List ingested episodes | `group_ids`, `max_episodes` |
| `add_memory` | Ingest new content | `name`, `episode_body`, `group_id`, `source`, `source_description` |

---

## Example: Full Constellation View Flow

End-to-end example for mataya's constellation view when a user searches for "singular learning theory":

```
1. User types "singular learning theory" in search bar

2. UI calls search_nodes:
   search_nodes(query="singular learning theory", group_ids=["astrolabe_main"], max_nodes=10)
   → Returns: [SLT, RLCT, Free Energy, Watanabe, Bayesian Information Criterion, ...]

3. User clicks on "RLCT" node

4. UI calls search_memory_facts with center_node_uuid:
   search_memory_facts(query="RLCT", group_ids=["astrolabe_main"], max_facts=20, center_node_uuid="<rlct-uuid>")
   → Returns: [
       "RLCT measures effective model complexity in SLT framework",
       "RLCT is analogous to number of parameters in regular models",
       "Lau et al. 2024 showed RLCT predicts generalization better than parameter count",
       ...
     ]

5. UI renders: RLCT node at center, connected nodes (SLT, Watanabe, Lau et al.)
   arranged around it, edges labeled with fact summaries.

6. User clicks on "Lau et al. 2024" → repeat step 4 with new center node.
```

---

## Example: Thread Viewer Sidebar Flow

End-to-end example for the sidebar when viewing a ming-qiao thread about Meridian's runtime:

```
1. User opens thread "RE: PROPOSAL: Meridian — Operations + Field Intelligence"

2. UI extracts key terms from recent messages:
   ["Meridian", "qwen3:14b", "Ollama", "local model", "fine-tuning", "ASTROLABE ingestion"]

3. UI calls search_nodes for top 3-5 terms:
   search_nodes(query="qwen3 14b local model", group_ids=["astrolabe_main"], max_nodes=5)
   search_nodes(query="fine-tuning techniques", group_ids=["astrolabe_main"], max_nodes=5)

4. UI deduplicates and renders sidebar:
   Related in ASTROLABE:
   - Qwen3 [Model] — "Alibaba's open-weight LLM series..."
   - LoRA [Technique] — "Low-rank adaptation for efficient fine-tuning..."
   - Ollama [Framework] — "Local LLM serving runtime..."

5. Clicking a sidebar entity opens the constellation view centered on that entity.
```

---

## Latency Budget

| Operation | Expected latency | Notes |
|-----------|-----------------|-------|
| `search_nodes` (single call) | 200-500ms | Semantic similarity against node embeddings |
| `search_memory_facts` (single call) | 200-500ms | Semantic similarity against fact embeddings |
| `search_memory_facts` with `center_node_uuid` | 100-300ms | Graph traversal, faster than pure semantic |
| Constellation 1-hop | 400-800ms | 1 search_nodes + 1 search_memory_facts |
| Constellation 2-hop | 1-5s | Depends on frontier size |
| Thread Viewer sidebar | 500ms-2s | 2-4 parallel search_nodes calls |

All times assume local FalkorDB with timeout set to 30000ms. Default 1000ms timeout will cause failures on graphs with 900+ nodes.

---

## Open Questions

1. **Entity extraction for long threads:** Raw text query vs. extracted entities vs. LLM-assisted extraction. Start simple, test, iterate. Flagged for evaluation when Thread Viewer integration is closer.

2. **N-hop depth limit:** Should the UI allow arbitrary depth exploration, or cap at 2 hops? The graph has ~900 nodes currently; at 3+ hops you hit a significant portion of the graph. Recommend 2-hop default with explicit "expand" action.

3. **Caching:** Should the UI cache ASTROLABE query results? The graph changes only during ingestion batches (not real-time), so stale results are acceptable for short periods. A 5-minute cache would reduce load during exploratory navigation.

4. **Quarantined items in constellation view:** Should items with `enrichment_status: "quarantined"` or `"ingesting"` appear in the constellation as ghost nodes? They aren't in the graph yet, but showing them would make the temporal gap between fetch and ingest visible (per Laozi-Jung's observation that this gap is information, not a limitation).
