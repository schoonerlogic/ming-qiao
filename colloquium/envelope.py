"""
Signed event envelopes — Python port of ming-qiao/src/crypto/envelope.rs

Provides Ed25519 signing and verification for colloquium invocations and responses.
Matches the Rust implementation exactly: same signing message format, same hash,
same nonce/TTL replay defense.

Addresses Ogma gate controls:
  1. Forged principal rejection (agent must have signing key)
  2. Signed context package (payload hash in signature)
  3. Replay defense (nonce registry with TTL)
  5. Signed response envelope + audit trail
"""

import hashlib
import json
import os
import threading
import time
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from nacl.signing import SigningKey, VerifyKey

MAX_EVENT_AGE_SECS = 60
NONCE_TTL_SECS = 120

# Key and keyring paths (same as ming-qiao P0 infrastructure)
# Signing keys — currently in aleph's config (only copy with seed files)
KEYS_DIR = Path.home() / "astralmaris" / "ming-qiao" / "aleph" / "config" / "keys"
# Keyring — use main config (canonical location)
KEYRING_PATH = Path.home() / "astralmaris" / "ming-qiao" / "main" / "config" / "council-keyring.json"


def load_signing_key(agent_id: str) -> SigningKey:
    """Load an agent's Ed25519 signing key from hex seed file."""
    seed_file = KEYS_DIR / f"{agent_id}.seed"
    hex_seed = seed_file.read_text().strip()
    return SigningKey(bytes.fromhex(hex_seed))


def load_keyring() -> dict[str, VerifyKey]:
    """Load the council keyring (agent_id -> VerifyKey)."""
    data = json.loads(KEYRING_PATH.read_text())
    agents = data.get("agents", {})
    keyring = {}
    for agent_id, info in agents.items():
        pub_hex = info.get("public_key", "")
        if pub_hex:
            keyring[agent_id] = VerifyKey(bytes.fromhex(pub_hex))
    return keyring


class NonceRegistry:
    """Thread-safe nonce registry with TTL-based expiry and optional file persistence.

    When persist_path is provided, nonces are appended to a JSONL file and loaded
    on init so replay defense survives across process restarts.
    """

    def __init__(self, ttl: int = NONCE_TTL_SECS, persist_path: Path | None = None):
        self._seen: dict[str, float] = {}
        self._ttl = ttl
        self._lock = threading.Lock()
        self._persist_path = persist_path
        if persist_path:
            self._load_persisted()

    def _load_persisted(self):
        """Load nonces from persistent storage, discarding expired entries."""
        if not self._persist_path or not self._persist_path.exists():
            return
        now = time.time()
        for line in self._persist_path.read_text().splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
                nonce, ts = entry["nonce"], entry["ts"]
                if now - ts <= self._ttl:
                    self._seen[nonce] = ts
            except (json.JSONDecodeError, KeyError):
                continue

    def _persist(self, nonce: str, ts: float):
        """Append a nonce to persistent storage."""
        if not self._persist_path:
            return
        self._persist_path.parent.mkdir(parents=True, exist_ok=True)
        with open(self._persist_path, "a") as f:
            f.write(json.dumps({"nonce": nonce, "ts": ts}) + "\n")

    def check_and_insert(self, nonce: str) -> bool:
        """Returns True if nonce is new, False if replayed."""
        now = time.time()
        with self._lock:
            # Cleanup expired
            expired = [k for k, t in self._seen.items() if now - t > self._ttl]
            for k in expired:
                del self._seen[k]
            # Check
            if nonce in self._seen:
                return False
            self._seen[nonce] = now
            self._persist(nonce, now)
            return True


def _hash_payload(payload: dict) -> str:
    """SHA-256 hash of canonical JSON payload, hex-encoded."""
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode()).hexdigest()


def _build_signing_message(event_id: str, timestamp_utc: str, nonce: str, payload_hash: str) -> bytes:
    """Build the message that gets signed: event_id\\ntimestamp\\nnonce\\npayload_hash"""
    return f"{event_id}\n{timestamp_utc}\n{nonce}\n{payload_hash}".encode()


def _generate_nonce() -> str:
    """Generate a 32-byte random nonce, hex-encoded."""
    return os.urandom(32).hex()


@dataclass
class SignedEnvelope:
    """Signed event envelope wrapping an arbitrary JSON payload."""
    event_id: str
    from_agent: str
    timestamp_utc: str
    nonce: str
    payload_hash: str
    signature: str
    payload: dict

    @classmethod
    def create(cls, from_agent: str, payload: dict, signing_key: SigningKey) -> "SignedEnvelope":
        event_id = str(uuid.uuid4())
        timestamp_utc = datetime.now(timezone.utc).isoformat()
        nonce = _generate_nonce()
        payload_hash = _hash_payload(payload)

        msg = _build_signing_message(event_id, timestamp_utc, nonce, payload_hash)
        signed = signing_key.sign(msg)
        signature_hex = signed.signature.hex()

        return cls(
            event_id=event_id,
            from_agent=from_agent,
            timestamp_utc=timestamp_utc,
            nonce=nonce,
            payload_hash=payload_hash,
            signature=signature_hex,
            payload=payload,
        )

    def verify(self, keyring: dict[str, VerifyKey], nonce_registry: NonceRegistry) -> str:
        """Verify envelope. Returns agent_id on success, raises on failure."""
        # 1. Timestamp freshness
        event_time = datetime.fromisoformat(self.timestamp_utc)
        age = (datetime.now(timezone.utc) - event_time).total_seconds()
        if age > MAX_EVENT_AGE_SECS:
            raise EnvelopeError(f"Event expired (age: {age:.0f}s, max: {MAX_EVENT_AGE_SECS}s)")
        if age < -5:
            raise EnvelopeError("Event timestamp is in the future")

        # 2. Nonce uniqueness
        if not nonce_registry.check_and_insert(self.nonce):
            raise EnvelopeError("Replayed nonce detected")

        # 3. Payload hash
        expected_hash = _hash_payload(self.payload)
        if self.payload_hash != expected_hash:
            raise EnvelopeError("Payload hash mismatch")

        # 4. Agent lookup
        verify_key = keyring.get(self.from_agent)
        if not verify_key:
            raise EnvelopeError(f"Unknown agent: {self.from_agent}")

        # 5. Signature verification
        msg = _build_signing_message(self.event_id, self.timestamp_utc, self.nonce, self.payload_hash)
        try:
            verify_key.verify(msg, bytes.fromhex(self.signature))
        except Exception:
            raise EnvelopeError("Invalid signature")

        return self.from_agent

    def to_dict(self) -> dict:
        return {
            "event_id": self.event_id,
            "from_agent": self.from_agent,
            "timestamp_utc": self.timestamp_utc,
            "nonce": self.nonce,
            "payload_hash": self.payload_hash,
            "signature": self.signature,
            "payload": self.payload,
        }

    @classmethod
    def from_dict(cls, d: dict) -> "SignedEnvelope":
        return cls(**d)


class EnvelopeError(Exception):
    pass
