# AstralMaris Persistence & Data Architecture

**Author:** Thales (Architect)
**Date:** 2026-02-22
**Status:** Draft for Council Review — Strategic Repositioning
**Scope:** Fleet-wide data governance, astral-forge model pipeline, repository responsibilities

---

## Strategic Decisions Under Consideration

### 1. Repository Responsibility Clarification

Based on Merlin's direction, the fleet responsibilities narrow and clarify:

| Repository | Responsibility | Scope |
|---|---|---|
| **builder-moon** | Cloud access & platform provisioning | Cluster lifecycle, CAPI, FluxCD, infrastructure-as-code. Stops being the everything-repo. |
| **ming-qiao** | Agent coordination transport | Messaging, events, real-time state. Operational data (Tier 1). Not responsible for long-term storage. |
| **echoessence** | Institutional memory & witness | Laozi-Jung's observations, agent profiles, project narratives (Tier 3). |
| **astral-forge** | Model modification & training pipeline | LoRA, distillation, RL, model merging. Consumes data; needs to trust it. |
| **inference-kitchen** | Model serving | SGLang, production inference. Receives models from the forge. |
| **latent-winds** | Public interface | Blog, API gateway. Consumes published artifacts. |

**The gap:** No repository currently owns cross-cutting data governance — the persistence classification, training lake, data lineage, partition integrity, and access control that span the entire fleet.

### 2. The Persistence Question: Where Does It Live?

Three options:

**Option A: Dedicated repository (e.g., `astral-vault`)**
A new repo solely for data governance: schemas, partition policies, promotion pipelines, access control definitions, training lake management, lineage tracking.

*Pros:* Clean separation. Data governance isn't subordinate to any one project. Scales independently. Clear ownership.
*Cons:* Another repo to maintain. Risk of becoming abstract if not tied to working systems.

**Option B: Inside astral-forge**
The forge is where data becomes models. Data governance lives alongside the training pipeline because that's where partition integrity matters most.

*Pros:* Data governance stays close to its primary consumer. The forge team (Aleph, Luban) naturally owns the data story because they need it to work. Less organizational overhead.
*Cons:* Mixes concerns — model training code and data governance policy in one repo. Other repos (ming-qiao, echoessence) would depend on forge for their data policies.

**Option C: Hybrid — governance schemas in a shared library, implementation distributed**
Define the persistence classification, tier schemas, and promotion contracts in a shared crate/package. Each repo implements its own tier responsibilities using the shared definitions.

*Pros:* Each repo owns its data but speaks the same language. The shared library enforces consistency without centralizing implementation.
*Cons:* Coordination overhead. Shared library versioning.

**My recommendation: Option A with a tight coupling to astral-forge.**

A dedicated `astral-vault` (or similar) owns the data governance definitions, partition management, and lineage tracking. But its first and primary consumer is astral-forge — the forge's training pipeline reads from the vault's partitions. The vault doesn't store models; it stores the data that models are trained on and the provenance of how that data got there.

Ming-qiao's watcher system (Aleph just shipped this) becomes the ingress pipeline — events flow from Tier 1 operational data into the vault's staging area via watcher dispatch. Echoessence contributes curated observations. The vault manages the partitioning.

---

## Astral-Forge Data Requirements

Looking at what already exists in astral-forge, the training pipeline needs:

### For LoRA Fine-Tuning
- **Training data:** JSONL, CSV, or HuggingFace datasets. The `LoRATrainer` already handles local paths, HuggingFace IDs, and directories of text files.
- **Data lineage:** Which dataset version trained which adapter. Currently captured in `training_config.json` but only records the dataset path — not the dataset version, checksum, or provenance.
- **Evaluation data:** Held-out test sets for measuring adapter quality. Currently not formalized.

### For Distillation
- **Teacher outputs:** Response traces from larger models on the training distribution. These are the most valuable and most sensitive artifacts — they contain the reasoning of capable models.
- **Student benchmarks:** Performance baselines before and after distillation.

### For Semantic Analysis
- **Document corpora:** PDFs, text files. The `personal-corpus-exploration.yaml` job already references an S3 path for the personal corpus.
- **Embedding artifacts:** `embeddings.npy`, `clusters.parquet`, `clusters.csv`, visualization HTML. These are intermediate artifacts with research value.

### For RL (Future)
- **Reward signal data:** Agent interaction traces where outcomes are known — decisions that worked vs. decisions that didn't. This is precisely what the ming-qiao decision records provide once enriched with outcome tracking.
- **Policy trajectories:** Sequences of agent actions with rewards. The tribal flow.

---

## Model Candidate Criteria

For the task-specific models Merlin wants to begin with, the selection criteria should be:

### Hardware Constraint
M4 Mac Mini Pro — 24GB unified memory, Apple Silicon (MPS backend). This sets a hard ceiling:
- Training: Models up to ~3B parameters with 4-bit quantization
- Inference: Models up to ~7B with quantization
- LoRA training: Comfortable at 1-3B base models

### Candidate Base Models for First Tasks

| Model | Parameters | License | Context | MPS Support | Use Case |
|---|---|---|---|---|---|
| Qwen2.5-3B | 3B | Apache 2.0 | 32K | Good | General purpose, multilingual, strong reasoning for size |
| Qwen2.5-1.5B | 1.5B | Apache 2.0 | 32K | Good | Lightweight, fast iteration, embedding tasks |
| Llama 3.2 1B | 1.24B | Llama 3.2 | 128K | Good | Long context, code understanding |
| Llama 3.2 3B | 3.21B | Llama 3.2 | 128K | Good | Strongest small Llama, instruction following |
| Phi-3 Mini | 3.8B | MIT | 128K | Good | Microsoft's efficient small model, strong reasoning |
| Gemma 2 2B | 2B | Gemma | 8K | Good | Google's efficient model, good for distillation targets |
| SmolLM2 1.7B | 1.7B | Apache 2.0 | 8K | Good | HuggingFace's small model, designed for fine-tuning |

### First Task Candidates

Given the current AstralMaris data and needs:

**Task 1: Agent coordination classifier**
Train a small model to classify agent messages by category (routine coordination, design convergence, design divergence, decision point, capability signal). This directly supports Laozi-Jung's observation categories and the training lake promotion criteria.
- Base: Qwen2.5-1.5B or SmolLM2 1.7B
- Technique: LoRA
- Data: Classified examples from ming-qiao conversation history
- Output: Adapter that scores messages for significance

**Task 2: Decision trace summarizer**
Train a model to produce structured summaries from raw decision threads — extracting question, alternatives, rationale, and resolution from conversational text.
- Base: Qwen2.5-3B
- Technique: LoRA or distillation (using Claude as teacher)
- Data: Decision threads from ming-qiao + their corresponding `record_decision` outputs as supervision signal
- Output: Model that can auto-draft decision records from conversation

**Task 3: Document semantic embedder**
Fine-tune an embedding model on AstralMaris domain vocabulary — Kubernetes, Rust, agent coordination, ML concepts. Improves semantic search quality for research papers and internal documents.
- Base: Qwen2.5-1.5B or a sentence-transformer base
- Technique: Contrastive LoRA
- Data: Internal documents + research papers with similarity labels
- Output: Domain-adapted embedder for the semantic analysis pipeline

---

## How Persistence Classification Connects to the Forge

The five-tier persistence system feeds astral-forge's training pipeline:

```
Tier 1 (Operational)                    Tier 5 (Training Lake)
┌──────────────────┐                    ┌──────────────────────┐
│ ming-qiao events │──── watcher ──────▶│ staging/             │
│ messages, tasks   │    dispatch        │   conversations/     │
│ presence          │                    │   decisions/         │
└──────────────────┘                    │   observations/      │
                                        ├──────────────────────┤
Tier 2 (Decisions)                      │ training/            │
┌──────────────────┐                    │   v001/              │
│ decision records  │──── auto ────────▶│   v002/              │
│ ADRs, proposals   │    promote        │   manifest.yaml      │
└──────────────────┘                    ├──────────────────────┤
                                        │ validation/          │
Tier 3 (Witness)                        │   manifest.yaml      │
┌──────────────────┐                    ├──────────────────────┤
│ observations      │──── curated ─────▶│ holdout/             │
│ patterns          │    selection       │   manifest.yaml      │
│ daily witness     │                    ├──────────────────────┤
└──────────────────┘                    │ provenance/          │
                                        │   lineage.jsonl      │
Tier 4 (Artifacts)                      └───────────┬──────────┘
┌──────────────────┐                                │
│ shared files      │                                │ reads
│ code, configs     │                                ▼
└──────────────────┘                    ┌──────────────────────┐
                                        │ astral-forge         │
                                        │   LoRA trainer       │
                                        │   distillation       │
                                        │   semantic analysis  │
                                        │   RL pipeline        │
                                        └──────────────────────┘
```

The watcher dispatch mechanism Aleph just shipped is the first pipe in this diagram. Today it routes events to Laozi-Jung's observation stream. Tomorrow it routes them to training lake staging. Same mechanism, different destination.

---

## Data Security Boundaries for Model Training

This is the glass bulkheads applied to the forge:

### Contamination Prevention
- Training and holdout partitions are physically separate (different directories, different access keys)
- No pipeline reads from both training and holdout in the same job
- Partition manifests include checksums of every file; any modification invalidates the partition
- Promotion from staging to a partition is an explicit, logged, irreversible action

### Provenance Chain
Every training sample carries:
- `source_tier`: Which tier it originated from (1, 2, 3, or 4)
- `source_id`: The event_id, decision_id, or observation path
- `source_timestamp`: When the original data was created
- `promoted_at`: When it entered the training lake
- `promoted_by`: Who or what promoted it (Merlin, Laozi-Jung, automatic rule)
- `partition`: Which partition it belongs to (staging, training, validation, holdout)
- `partition_version`: Which version of the partition

### Sovereign Data Principles
- All training data originates from AstralMaris's own agent interactions, decisions, and observations
- External data (HuggingFace datasets, arXiv papers) is clearly labeled with `source_tier: external` and tracked separately
- Models trained on sovereign data carry a `sovereignty` tag indicating what percentage of training data is internal vs. external
- The holdout partition NEVER contains external data — it measures performance on the system's own reasoning

---

## Implementation Sequence

### Phase 1: Repository Setup (This Week)
- Create the persistence governance home (dedicated repo or section of astral-forge — pending decision)
- Define tier schemas as data contracts (YAML or JSON Schema)
- Document promotion rules and retention policies
- Wire the first promotion: `DecisionRecorded` → Tier 2 git YAML (dual-write)

### Phase 2: Forge Data Pipeline (Next)
- Add provenance fields to astral-forge's `training_config.json`
- Implement dataset versioning (content-addressed by checksum)
- Create staging directory structure for training lake
- First training job using sovereign data: agent coordination classifier

### Phase 3: Model Selection & First Run
- Validate base model candidates on M4 Mac Mini Pro (MPS compatibility, memory fit)
- Run first LoRA training job with ming-qiao conversation data
- Establish evaluation baseline
- Record the decision formally through ming-qiao

### Phase 4: Partition Management
- Implement promotion pipeline (staging → training/validation/holdout)
- Implement lineage tracking
- Implement access controls (forge reads training; eval reads holdout; never both)

---

## Open Decisions for Merlin

1. **Persistence home:** Dedicated repo (`astral-vault`) or section of astral-forge? This affects ownership and access patterns.

2. **First base model:** Qwen2.5-3B is the strongest candidate for the M4 constraint. Qwen2.5-1.5B is faster for iteration. Should we start with one and evaluate both, or commit to one?

3. **First task:** Agent coordination classifier (most immediately useful for the observation pipeline), decision trace summarizer (most directly valuable for institutional memory), or domain embedder (foundational for all semantic work)?

4. **Teacher model for distillation:** Claude is the obvious choice for generating high-quality supervision signal. Should we establish a formal distillation pipeline where Claude generates training data, or keep it informal?

5. **External data policy:** When astral-forge uses HuggingFace datasets (like `bigcode/starcoderdata` in the existing LoRA example), how strictly do we track the boundary between sovereign and external training data?
