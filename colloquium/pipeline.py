"""
Context Assembly Pipeline — Phase 2.

Assembles the full context package for a colloquium casting:
1. ORACLE briefing (search_nodes + search_facts for context tags)
2. Thread retrieval (proposal + prior responses from ming-qiao)
3. Agent work context (recent work from agent's worktree — Luban's prototype)
4. Signed invocation envelope (Ed25519, Ogma gate control 1-3)
5. Signed response envelope + audit log (Ogma gate control 5)

Commitment detection (Ogma gate control 4) is applied post-generation.
"""

import asyncio
import json
import re
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from adapter import VoiceAdapter, ColloquiumResponse, _build_user_message
from envelope import SignedEnvelope, NonceRegistry, load_signing_key, load_keyring, EnvelopeError
from mingqiao import load_token, read_thread, post_reply
from oracle_client import query_oracle, OracleBriefing

LOG_DIR = Path(__file__).parent / "logs"
LOG_DIR.mkdir(exist_ok=True)

# Module-level persistent nonce registry — survives across cast() calls and
# loads from disk on process restart (Ogma re-review finding #1).
_NONCE_REGISTRY = NonceRegistry(persist_path=LOG_DIR / "nonce-registry.jsonl")

# Commitment patterns — ported from chamber-voice.py (Ogma gate control 4)
COMMITMENT_PATTERNS = [
    r"\bI will\b",
    r"\bI'll\b(?!\s+(?:note|observe|watch|consider|think|flag|keep|check|look))",
    r"\bI commit\b",
    r"\bI volunteer\b",
    r"\blet me (?:handle|build|deploy|implement|fix|create)\b",
    r"\bI(?:'ll| will) (?:build|deploy|implement|fix|create|write|deliver|ship)\b",
    r"\bI take (?:ownership|responsibility)\b",
    r"\bI(?:'ll| will) have (?:it|this|that) (?:ready|done|finished)\b",
]


def detect_commitment(text: str) -> bool:
    for pattern in COMMITMENT_PATTERNS:
        if re.search(pattern, text, re.IGNORECASE):
            return True
    return False


@dataclass
class CastingResult:
    """Full result of a colloquium casting including security envelope."""
    response: ColloquiumResponse
    invocation_envelope: SignedEnvelope
    response_envelope: SignedEnvelope
    oracle_briefing: OracleBriefing
    commitment_detected: bool
    posted: bool
    thread_id: str | None


async def cast(
    voice: VoiceAdapter,
    thread_id: str | None = None,
    proposal_text: str | None = None,
    context_tags: list[str] | None = None,
    post: bool = False,
) -> CastingResult:
    """Execute a full colloquium casting with security controls.

    1. Query ORACLE for context tags
    2. Read thread for proposal + prior responses
    3. Sign invocation envelope
    4. Invoke voice adapter
    5. Check for commitment language
    6. Sign response envelope
    7. Optionally post to ming-qiao
    8. Write audit log
    """
    agent_id = voice.agent_id()
    token = load_token(agent_id)
    signing_key = load_signing_key(agent_id)
    keyring = load_keyring()
    nonce_registry = _NONCE_REGISTRY

    # --- 1. ORACLE briefing ---
    tags = context_tags or []
    if not tags and proposal_text:
        # Extract basic tags from proposal (simple word extraction)
        tags = _extract_tags(proposal_text)

    briefing = await query_oracle(tags) if tags else OracleBriefing(
        nodes=[], facts=[], raw_query="", available=True,
    )
    briefing_text = briefing.to_text()

    # --- 2. Thread retrieval ---
    prior_responses = []
    if thread_id:
        thread = await read_thread(thread_id, token)
        messages = thread.get("messages", [])
        if not proposal_text and messages:
            proposal_text = f"**{messages[0].get('subject', '')}**\n\n{messages[0]['content']}"
        for msg in messages[1:]:
            prior_responses.append({
                "from": msg.get("from", "unknown"),
                "content": msg.get("content", ""),
            })

    if not proposal_text:
        raise ValueError("No proposal: provide --thread or --proposal")

    # --- 3. Agent work context (Phase 2: static; Luban's prototype will replace) ---
    agent_work = _load_agent_work_context(agent_id)

    # --- 4. Sign invocation envelope (Ogma controls 1-3) ---
    invocation_payload = {
        "type": "colloquium_invocation",
        "agent_id": agent_id,
        "thread_id": thread_id or "",
        "proposal_hash": _sha256(proposal_text),
        "context_tags": tags,
        "oracle_available": briefing.available,
        "prior_response_count": len(prior_responses),
    }
    invocation_envelope = SignedEnvelope.create(agent_id, invocation_payload, signing_key)

    # Verify our own invocation (proves the signing infrastructure works)
    verified_agent = invocation_envelope.verify(keyring, nonce_registry)
    assert verified_agent == agent_id, f"Self-verification failed: {verified_agent} != {agent_id}"

    # --- 5. Prepare context and invoke ---
    ctx = voice.prepare_context(
        proposal=proposal_text,
        briefing=briefing_text,
        prior_responses=prior_responses,
        agent_work_context=agent_work,
    )

    response = await voice.invoke(ctx, colloquium_id=thread_id or "")

    # --- 6. Commitment detection (Ogma control 4) ---
    commitment_detected = detect_commitment(response.content)

    # --- 7. Sign response envelope (Ogma control 5) ---
    response_payload = {
        "type": "colloquium_response",
        "agent_id": agent_id,
        "thread_id": thread_id or "",
        "invocation_id": invocation_envelope.event_id,
        "model": response.model,
        "response_hash": _sha256(response.content),
        "commitment_detected": commitment_detected,
        "autonomous": response.autonomous,
    }
    response_envelope = SignedEnvelope.create(agent_id, response_payload, signing_key)

    # --- 8. Post if authorized and no commitment ---
    posted = False
    if post and thread_id and not commitment_detected:
        tagged = _format_for_posting(response, invocation_envelope.event_id)
        await post_reply(thread_id, agent_id, token, tagged)
        posted = True
    elif post and commitment_detected:
        print(f"[HELD] Commitment detected — response not posted. Requires human confirmation.")

    # --- 9. Audit log ---
    result = CastingResult(
        response=response,
        invocation_envelope=invocation_envelope,
        response_envelope=response_envelope,
        oracle_briefing=briefing,
        commitment_detected=commitment_detected,
        posted=posted,
        thread_id=thread_id,
    )
    _write_audit_log(result)

    return result


def _extract_tags(text: str) -> list[str]:
    """Extract simple context tags from proposal text."""
    # Take notable words (>4 chars, not common stopwords)
    stopwords = {"about", "after", "before", "between", "could", "every", "first",
                 "their", "there", "these", "those", "which", "while", "would", "should"}
    words = re.findall(r'\b[a-zA-Z]{5,}\b', text.lower())
    unique = []
    seen = set()
    for w in words:
        if w not in stopwords and w not in seen:
            seen.add(w)
            unique.append(w)
    return unique[:8]  # Top 8 distinct terms


def _load_agent_work_context(agent_id: str) -> str:
    """Load agent work context. Phase 2: static files. Luban's prototype will replace."""
    context_file = Path(__file__).parent / "work_context" / f"{agent_id}.md"
    if context_file.exists():
        return context_file.read_text()

    # Fallback: hardcoded for Aleph (Phase 1 compatibility)
    if agent_id == "aleph":
        return (
            "## ATLAS-01 Findings (2026-02-28)\n"
            "- Adapter merging FAILS for factual knowledge (all 12 configs)\n"
            "- Joint training WORKS — single adapter holds multiple domains\n"
            "- Architecture: purpose-built adapters + RAG for dynamic knowledge\n\n"
            "## ORACLE System\n"
            "- Knowledge graph: 799 nodes, 1798 relationships, 78 episodes\n"
            "- Models: qwen3:8b (extraction), nomic-embed-text (embeddings)\n\n"
            "## Council Awakener\n"
            "- PostToolUse hooks, no matcher, fires on all tool uses\n"
            "- Three wake paths: INJECT, INTERRUPT, HEADLESS\n"
        )
    return ""


def _sha256(text: str) -> str:
    import hashlib
    return hashlib.sha256(text.encode()).hexdigest()


def _format_for_posting(response: ColloquiumResponse, invocation_id: str) -> str:
    meta = json.dumps({
        "autonomous": response.autonomous,
        "model": response.model,
        "adapter": f"{response.agent_id.title()}Voice",
        "invocation_id": invocation_id,
    })
    provenance = "authored" if not response.autonomous else "autonomous"
    return (
        f"{response.content}\n\n"
        f"--- *{response.agent_id} [colloquium voice | {response.model} | {provenance}]* ---\n"
        f"<!-- colloquium-meta: {meta} -->"
    )


def _write_audit_log(result: CastingResult):
    """Write full audit record for Ogma gate review."""
    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "agent": result.response.agent_id,
        "model": result.response.model,
        "thread_id": result.thread_id,
        "invocation_envelope": result.invocation_envelope.to_dict(),
        "response_envelope": result.response_envelope.to_dict(),
        "oracle_available": result.oracle_briefing.available,
        "oracle_query": result.oracle_briefing.raw_query,
        "oracle_node_count": len(result.oracle_briefing.nodes),
        "oracle_fact_count": len(result.oracle_briefing.facts),
        "commitment_detected": result.commitment_detected,
        "autonomous": result.response.autonomous,
        "posted": result.posted,
    }
    with open(LOG_DIR / "audit-log.jsonl", "a") as f:
        f.write(json.dumps(entry) + "\n")
