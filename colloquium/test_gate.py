#!/usr/bin/env python3
"""
Ogma Gate Control Tests — evidence for security review.

Tests all 5 controls from the threat gate:
  1. Forged principal rejection
  2. Tampered context package rejection (hash mismatch)
  3. Replay defense (duplicate nonce rejection)
  4. Commitment-intent detection
  5. Signed response envelope + audit trail

Run: python test_gate.py
"""

import json
import sys
from pathlib import Path

from nacl.signing import SigningKey

from envelope import SignedEnvelope, NonceRegistry, load_signing_key, load_keyring, EnvelopeError
from pipeline import detect_commitment, _write_audit_log, CastingResult, LOG_DIR

PASS = 0
FAIL = 0


def check(name: str, condition: bool, detail: str = ""):
    global PASS, FAIL
    if condition:
        PASS += 1
        print(f"  PASS  {name}")
    else:
        FAIL += 1
        print(f"  FAIL  {name} — {detail}")


def test_1_forged_principal():
    """Control 1: Forged principal cannot invoke another agent's adapter."""
    print("\n--- Control 1: Forged Principal Rejection ---")

    keyring = load_keyring()
    nonce_registry = NonceRegistry()

    # Generate a rogue key not in the keyring
    rogue_key = SigningKey.generate()
    payload = {"type": "colloquium_invocation", "agent_id": "aleph"}

    # Sign with rogue key claiming to be aleph
    envelope = SignedEnvelope.create("aleph", payload, rogue_key)

    try:
        envelope.verify(keyring, nonce_registry)
        check("rogue key rejected", False, "verification should have failed")
    except EnvelopeError as e:
        check("rogue key rejected", "Invalid signature" in str(e))

    # Sign claiming to be nonexistent agent
    envelope2 = SignedEnvelope.create("rogue-agent", payload, rogue_key)
    try:
        envelope2.verify(keyring, nonce_registry)
        check("unknown agent rejected", False, "verification should have failed")
    except EnvelopeError as e:
        check("unknown agent rejected", "Unknown agent" in str(e))

    # Legitimate key, legitimate agent — should pass
    real_key = load_signing_key("aleph")
    envelope3 = SignedEnvelope.create("aleph", payload, real_key)
    try:
        agent = envelope3.verify(keyring, nonce_registry)
        check("legitimate agent accepted", agent == "aleph")
    except EnvelopeError as e:
        check("legitimate agent accepted", False, str(e))


def test_2_tampered_context():
    """Control 2: Tampered context package is rejected."""
    print("\n--- Control 2: Tampered Context Package Rejection ---")

    keyring = load_keyring()
    nonce_registry = NonceRegistry()
    real_key = load_signing_key("aleph")

    payload = {"type": "colloquium_invocation", "proposal_hash": "abc123", "agent_id": "aleph"}
    envelope = SignedEnvelope.create("aleph", payload, real_key)

    # Tamper with payload
    envelope.payload["proposal_hash"] = "TAMPERED"

    try:
        envelope.verify(keyring, nonce_registry)
        check("tampered payload rejected", False, "verification should have failed")
    except EnvelopeError as e:
        check("tampered payload rejected", "hash mismatch" in str(e).lower())

    # Tamper with from_agent field (identity spoofing via payload)
    envelope2 = SignedEnvelope.create("aleph", {"agent_id": "aleph"}, real_key)
    original_payload = envelope2.payload.copy()
    envelope2.from_agent = "luban"  # Try to claim we're luban

    try:
        envelope2.verify(keyring, nonce_registry)
        check("identity spoof rejected", False, "verification should have failed")
    except EnvelopeError as e:
        # Fails because luban's public key doesn't match aleph's signature
        check("identity spoof rejected", True)


def test_3_replay_defense():
    """Control 3: Duplicate invocation is rejected."""
    print("\n--- Control 3: Replay Defense ---")

    keyring = load_keyring()
    nonce_registry = NonceRegistry()
    real_key = load_signing_key("aleph")

    payload = {"type": "colloquium_invocation", "agent_id": "aleph"}
    envelope = SignedEnvelope.create("aleph", payload, real_key)

    # First verification succeeds
    try:
        agent = envelope.verify(keyring, nonce_registry)
        check("first invocation accepted", agent == "aleph")
    except EnvelopeError as e:
        check("first invocation accepted", False, str(e))

    # Replay with same nonce — must fail
    try:
        envelope.verify(keyring, nonce_registry)
        check("replay rejected", False, "second verification should have failed")
    except EnvelopeError as e:
        check("replay rejected", "nonce" in str(e).lower())

    # Cross-process replay defense: persist nonce, create new registry, verify rejection
    import tempfile
    with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
        persist_path = Path(f.name)
    try:
        reg1 = NonceRegistry(persist_path=persist_path)
        envelope2 = SignedEnvelope.create("aleph", payload, real_key)
        envelope2.verify(keyring, reg1)  # Accept and persist nonce

        # New registry instance loads from same file — simulates process restart
        reg2 = NonceRegistry(persist_path=persist_path)
        try:
            envelope2.verify(keyring, reg2)
            check("cross-process replay rejected", False, "should have failed after restart")
        except EnvelopeError as e:
            check("cross-process replay rejected", "nonce" in str(e).lower())
    finally:
        persist_path.unlink(missing_ok=True)


def test_4_commitment_detection():
    """Control 4: Commitment-intent responses are flagged."""
    print("\n--- Control 4: Commitment Detection ---")

    # Should detect
    check("'I will build' detected",
          detect_commitment("I will build the pipeline tomorrow"))
    check("'Let me handle' detected",
          detect_commitment("Let me handle the deployment"))
    check("'I'll deploy' detected",
          detect_commitment("I'll deploy this to production"))
    check("'I take ownership' detected",
          detect_commitment("I take ownership of this task"))

    # Should NOT detect (conversational, not commitments)
    check("observation not flagged",
          not detect_commitment("The adapter merging fails for factual knowledge"))
    check("recommendation not flagged",
          not detect_commitment("I recommend using real-time queries"))
    check("'I'll note' not flagged",
          not detect_commitment("I'll note this for future reference"))
    check("question not flagged",
          not detect_commitment("Should we use SLERP for adapter merging?"))


def test_5_response_envelope():
    """Control 5: Signed response envelope + audit trail."""
    print("\n--- Control 5: Signed Response Envelope ---")

    keyring = load_keyring()
    nonce_registry = NonceRegistry()
    real_key = load_signing_key("aleph")

    # Simulate invocation + response pair
    inv_payload = {"type": "colloquium_invocation", "agent_id": "aleph"}
    inv_envelope = SignedEnvelope.create("aleph", inv_payload, real_key)

    inv_id = inv_envelope.event_id
    try:
        inv_envelope.verify(keyring, nonce_registry)
        check("invocation envelope verified", True)
    except EnvelopeError as e:
        check("invocation envelope verified", False, str(e))

    # Response envelope references invocation
    resp_payload = {
        "type": "colloquium_response",
        "agent_id": "aleph",
        "invocation_id": inv_id,
        "response_hash": "deadbeef" * 8,
        "commitment_detected": False,
    }
    resp_envelope = SignedEnvelope.create("aleph", resp_payload, real_key)

    try:
        agent = resp_envelope.verify(keyring, nonce_registry)
        check("response envelope verified", agent == "aleph")
    except EnvelopeError as e:
        check("response envelope verified", False, str(e))

    # Verify invocation_id linkage
    check("response links to invocation",
          resp_envelope.payload["invocation_id"] == inv_id)

    # Verify envelope serialization round-trip
    d = resp_envelope.to_dict()
    restored = SignedEnvelope.from_dict(d)
    check("envelope serialization round-trip",
          restored.event_id == resp_envelope.event_id and
          restored.signature == resp_envelope.signature)

    # Write a test audit entry and verify its signed contents
    from pipeline import _write_audit_log, CastingResult
    from adapter import ColloquiumResponse
    from astrolabe_client import AstrolabeBriefing

    test_result = CastingResult(
        response=ColloquiumResponse(agent_id="aleph", content="test", model="sonnet"),
        invocation_envelope=inv_envelope,
        response_envelope=resp_envelope,
        astrolabe_briefing=AstrolabeBriefing(nodes=[], facts=[], raw_query="test", available=True),
        commitment_detected=False,
        posted=False,
        thread_id="test-thread-001",
    )
    _write_audit_log(test_result)

    audit_log = Path(__file__).parent / "logs" / "audit-log.jsonl"
    check("audit log file exists", audit_log.exists())

    last_line = audit_log.read_text().strip().split("\n")[-1]
    entry = json.loads(last_line)
    check("audit has signed invocation envelope",
          entry["invocation_envelope"]["from_agent"] == "aleph"
          and "signature" in entry["invocation_envelope"])
    check("audit has signed response envelope",
          entry["response_envelope"]["from_agent"] == "aleph"
          and "signature" in entry["response_envelope"])
    check("audit links invocation to response",
          entry["response_envelope"]["payload"]["invocation_id"]
          == entry["invocation_envelope"]["event_id"])
    check("audit has astrolabe stats",
          "astrolabe_available" in entry and "astrolabe_node_count" in entry)
    check("audit has commitment flag",
          "commitment_detected" in entry and entry["commitment_detected"] is False)


if __name__ == "__main__":
    print("Ogma Gate Control Tests")
    print("=" * 50)

    test_1_forged_principal()
    test_2_tampered_context()
    test_3_replay_defense()
    test_4_commitment_detection()
    test_5_response_envelope()

    print("\n" + "=" * 50)
    print(f"Results: {PASS} passed, {FAIL} failed")

    if FAIL > 0:
        sys.exit(1)
    print("\nAll gate controls validated. Evidence ready for Ogma review.")
