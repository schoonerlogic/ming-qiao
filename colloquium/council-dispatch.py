#!/usr/bin/env python3
"""Council Dispatch Daemon

Watches the council notification stream and automates routine dispatch:

1. COLLOQUIUM auto-cast: When a colloquium-intent thread appears from a convener,
   automatically casts all 6 voices (excludes Merlin).

2. DISCUSS digest: Queues discuss-intent threads and writes a summary digest
   for Proteus to review. No auto-response — just awareness.

3. DIRECT routing: When a council message @-mentions a specific agent,
   logs the routing for visibility.

Watches: council-chamber.jsonl (receives all council broadcasts)
Writes: logs/dispatch-log.jsonl, logs/dispatch-digest.md

Usage:
    python council-dispatch.py              # Daemon mode
    python council-dispatch.py --once       # Single pass
    python council-dispatch.py --digest     # Print current digest
"""

import argparse
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
LOG_DIR = SCRIPT_DIR / "logs"
LOG_DIR.mkdir(exist_ok=True)

DISPATCH_LOG = LOG_DIR / "dispatch-log.jsonl"
DISPATCH_STATE = LOG_DIR / "dispatch-state.json"
DISPATCH_DIGEST = LOG_DIR / "dispatch-digest.md"

NOTIFICATIONS = Path.home() / "astralmaris" / "ming-qiao" / "notifications" / "council-chamber.jsonl"

# Colloquium detection: subject patterns that indicate a casting request
COLLOQUIUM_MARKERS = [
    "colloquium —",
    "colloquium —",  # em dash variant
    "colloquium -",
    "inaugural colloquium",
]

# Agents that can be @-mentioned for routing
COUNCIL_AGENTS = ["aleph", "thales", "luban", "laozi-jung", "mataya", "ogma"]

POLL_INTERVAL = 10  # seconds


def load_state() -> dict:
    if DISPATCH_STATE.exists():
        return json.loads(DISPATCH_STATE.read_text())
    return {
        "last_line": 0,
        "cast_threads": [],       # Thread IDs already auto-cast
        "digest_threads": {},     # thread_subject -> {count, last_from, last_time}
    }


def save_state(state: dict):
    DISPATCH_STATE.write_text(json.dumps(state, indent=2))


def log_dispatch(action: str, thread_id: str, detail: str):
    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "action": action,
        "thread_id": thread_id,
        "detail": detail,
    }
    with open(DISPATCH_LOG, "a") as f:
        f.write(json.dumps(entry) + "\n")


def is_colloquium_convocation(msg: dict) -> bool:
    """Detect if a message is a new colloquium being convened."""
    subject = msg.get("subject", "").lower()
    intent = msg.get("intent", "")
    to = msg.get("to", "")

    # Must be to council
    if to != "council":
        return False

    # Check subject markers
    for marker in COLLOQUIUM_MARKERS:
        if marker in subject:
            return True

    # Check for explicit colloquium intent
    if intent == "colloquium":
        return True

    # Check content for round markers (Round 2, etc.)
    content = msg.get("content", msg.get("content_preview", "")).lower()
    if "round 2" in content or "round 3" in content or "directed dialogue" in content:
        return True

    return False


def extract_thread_id(msg: dict) -> str:
    """Extract thread ID from notification. May be in different fields."""
    return msg.get("thread_id", msg.get("event_id", ""))


def extract_mentions(content: str) -> list[str]:
    """Find @-mentioned agents in content."""
    mentions = []
    content_lower = content.lower()
    for agent in COUNCIL_AGENTS:
        # Check for "Aleph —", "Aleph:", "@aleph", or agent name at start of line
        if f"@{agent}" in content_lower or f"{agent} —" in content_lower:
            mentions.append(agent)
    return mentions


def auto_cast(thread_id: str, subject: str, tags: list[str]) -> bool:
    """Run cast.py --all --post against a thread (autonomous — no --authored flag).

    Captain voices (thales, merlin) are automatically skipped by the adapter
    when --authored is absent. Only non-captain voices are cast.
    """
    cmd = [
        sys.executable,
        str(SCRIPT_DIR / "cast.py"),
        "--thread", thread_id,
        "--all",
        "--post",
    ]
    if tags:
        cmd.extend(["--tags", ",".join(tags)])

    captain_skipped = ["thales", "merlin"]
    print(f"[CAST] Auto-casting voices on: {subject} (captain voices skipped: {captain_skipped})")
    print(f"       Thread: {thread_id}")

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=900,  # 15 min for voices
            cwd=str(SCRIPT_DIR),
            env={**os.environ, "VIRTUAL_ENV": str(SCRIPT_DIR / ".venv"),
                 "PATH": f"{SCRIPT_DIR / '.venv' / 'bin'}:{os.environ.get('PATH', '')}"},
        )
        if result.returncode == 0:
            print(f"[CAST] Complete. Output:\n{result.stdout[-500:]}")
            log_dispatch("auto-cast", thread_id,
                         f"5 voices cast on: {subject} (captain_voice_skipped: {captain_skipped}, autonomous: true)")
            return True
        else:
            print(f"[CAST] Failed (rc={result.returncode}): {result.stderr[-300:]}")
            log_dispatch("cast-failed", thread_id, result.stderr[-200:])
            return False
    except subprocess.TimeoutExpired:
        print(f"[CAST] Timeout on: {subject}")
        log_dispatch("cast-timeout", thread_id, "15 min timeout exceeded")
        return False


def extract_tags_from_subject(subject: str) -> list[str]:
    """Pull rough context tags from a colloquium subject line."""
    # Remove common prefixes
    clean = subject.lower()
    for prefix in ["colloquium —", "colloquium -", "inaugural colloquium —", "round 2 —"]:
        clean = clean.replace(prefix, "")
    words = [w.strip(".,!?()") for w in clean.split() if len(w) > 3]
    return words[:6]


def update_digest(state: dict):
    """Write a human-readable digest of pending discuss threads."""
    digest = state.get("digest_threads", {})
    if not digest:
        DISPATCH_DIGEST.write_text("# Council Dispatch Digest\n\nNo pending discussions.\n")
        return

    lines = [
        "# Council Dispatch Digest",
        f"*Updated: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')}*\n",
    ]

    for subject, info in sorted(digest.items(), key=lambda x: x[1].get("last_time", ""), reverse=True):
        lines.append(f"### {subject}")
        lines.append(f"- Messages: {info['count']}")
        lines.append(f"- Last from: {info['last_from']}")
        lines.append(f"- Last activity: {info['last_time'][:19]}")
        if info.get("mentions"):
            lines.append(f"- Mentions: {', '.join(info['mentions'])}")
        lines.append("")

    DISPATCH_DIGEST.write_text("\n".join(lines))


def process_notifications(state: dict) -> int:
    """Process new notifications since last checkpoint. Returns count processed."""
    if not NOTIFICATIONS.exists():
        return 0

    with open(NOTIFICATIONS) as f:
        lines = f.readlines()

    last_line = state.get("last_line", 0)
    new_lines = lines[last_line:]
    state["last_line"] = len(lines)

    if not new_lines:
        return 0

    cast_threads = set(state.get("cast_threads", []))
    digest = state.get("digest_threads", {})
    actions = 0

    for line in new_lines:
        try:
            msg = json.loads(line.strip())
        except (json.JSONDecodeError, ValueError):
            continue

        subject = msg.get("subject", "")
        from_agent = msg.get("from", "")
        intent = msg.get("intent", "")
        to = msg.get("to", "")
        timestamp = msg.get("timestamp", "")
        thread_id = extract_thread_id(msg)
        content = msg.get("content", msg.get("content_preview", ""))

        # Skip our own messages
        if from_agent == "council-dispatch":
            continue

        # --- Colloquium auto-cast ---
        if is_colloquium_convocation(msg) and thread_id and thread_id not in cast_threads:
            tags = extract_tags_from_subject(subject)
            if auto_cast(thread_id, subject, tags):
                cast_threads.add(thread_id)
                actions += 1

        # --- Discuss thread tracking ---
        if intent == "discuss" and to == "council":
            mentions = extract_mentions(content)
            if subject not in digest:
                digest[subject] = {"count": 0, "mentions": []}
            digest[subject]["count"] = digest[subject].get("count", 0) + 1
            digest[subject]["last_from"] = from_agent
            digest[subject]["last_time"] = timestamp
            if mentions:
                existing = set(digest[subject].get("mentions", []))
                existing.update(mentions)
                digest[subject]["mentions"] = list(existing)
            actions += 1

    state["cast_threads"] = list(cast_threads)
    state["digest_threads"] = digest
    update_digest(state)

    return actions


def run_once():
    state = load_state()
    count = process_notifications(state)
    save_state(state)
    print(f"Processed {count} actions")
    return count


def run_daemon():
    print(f"Council dispatch daemon starting")
    print(f"Watching: {NOTIFICATIONS}")
    print(f"Digest: {DISPATCH_DIGEST}")
    print(f"PID: {os.getpid()}")

    while True:
        state = load_state()
        try:
            count = process_notifications(state)
            if count > 0:
                save_state(state)
                ts = datetime.now(timezone.utc).strftime('%H:%M:%S')
                print(f"[{ts}] Processed {count} actions")
        except Exception as e:
            ts = datetime.now(timezone.utc).strftime('%H:%M:%S')
            print(f"[{ts}] Error: {e}")
            log_dispatch("error", "", str(e))

        time.sleep(POLL_INTERVAL)


def print_digest():
    if DISPATCH_DIGEST.exists():
        print(DISPATCH_DIGEST.read_text())
    else:
        print("No digest yet. Run --once or start the daemon first.")


def main():
    parser = argparse.ArgumentParser(description="Council dispatch daemon")
    parser.add_argument("--once", action="store_true", help="Single pass, then exit")
    parser.add_argument("--digest", action="store_true", help="Print current digest")
    parser.add_argument("--reset", action="store_true", help="Reset state (reprocess all)")
    args = parser.parse_args()

    if args.digest:
        print_digest()
    elif args.reset:
        if DISPATCH_STATE.exists():
            DISPATCH_STATE.unlink()
        print("State reset.")
    elif args.once:
        run_once()
    else:
        run_daemon()


if __name__ == "__main__":
    main()
