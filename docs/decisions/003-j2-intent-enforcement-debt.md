# Decision: J-2 Intent Enforcement — Design Debt (Phase 2 Mandatory)

**Date:** 2026-03-13
**Context:** Ogma verification of Jikimi pre-launch conditions found that ming-qiao has no per-agent intent enforcement. Any authenticated agent can send `intent: request` to any other agent. This is a platform-level gap, not just a Jikimi concern.

**Ogma Verdict:** J-2 FAIL — no enforcement mechanism exists.

**Options considered:**
1. **Path A** — Delay Jikimi Phase 1 until Rust-level enforcement is built → Too slow, blocks health monitoring
2. **Path B** — Proceed with script-layer self-restriction for Phase 1, mandate architectural enforcement before Phase 2 → Accepted by Proteus

**Decision:** Path B — Jikimi Phase 1 uses `intent: inform` only (self-enforced in scripts). Architectural enforcement mandatory before Phase 2.

**Implementation required (before Phase 2):**
- Add `allowed_intents` field to `agent-capabilities.toml` entries
- Enforce in ming-qiao Rust HTTP handlers: reject messages where `intent` is not in sender's `allowed_intents`
- Jikimi's config: `allowed_intents = ["inform", "discuss"]` (no `request` authority)
- All existing agents grandfathered with full intent access

**Consequences:**
- Phase 1 Jikimi can only self-restrict — a bug or model hallucination could send `intent: request`
- This is acceptable risk for Phase 1 (shell scripts, no LLM involvement)
- Phase 2 (LLM-driven analysis) MUST NOT proceed without Rust-level enforcement

**Participants:** Ogma (verification), Thales (recommendation), Proteus (decision), Aleph (implementation)
