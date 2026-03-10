#!/usr/bin/env python3
"""
Worktree Context Extractor for Colloquium Briefings

Extracts relevant recent context from an agent's worktree and ming-qiao threads
for inclusion in colloquium context packages.

Usage:
    python worktree-extract.py --agent luban --keywords "benchmark inference"
    python worktree-extract.py --agent aleph --keywords "council,ming-qiao" --days 14
"""

import argparse
import subprocess
import json
import urllib.request
from pathlib import Path
from datetime import datetime, timedelta
import re


MINGQIAO_BASE = "http://localhost:7777"

WORKTREE_PATHS = {
    "luban": [
        Path("/Users/proteus/astralmaris/ming-qiao/luban"),
        Path("/Users/proteus/astralmaris/inference-kitchen/luban"),
    ],
    "aleph": [
        Path("/Users/proteus/astralmaris/ming-qiao/aleph"),
        Path("/Users/proteus/astralmaris/astral-forge/aleph"),
        Path("/Users/proteus/astralmaris/inference-kitchen/aleph"),
    ],
    "thales": [],
    "mataya": [
        Path("/Users/proteus/astralmaris/ming-qiao/main"),
    ],
}


def get_git_commits(worktree_path: Path, days: int = 7, keywords: list = None) -> list:
    """Get recent commits from agent's worktree."""
    since_date = (datetime.now() - timedelta(days=days)).strftime("%Y-%m-%d")
    cmd = [
        "git", "-C", str(worktree_path), "log",
        f"--since={since_date}", "--oneline", "--no-merges", "HEAD",
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        return []

    commits = []
    for line in result.stdout.strip().split("\n"):
        if not line:
            continue
        match = re.match(r"^([a-f0-9]+)\s+(.+)$", line)
        if not match:
            continue
        commit_hash, message = match.groups()
        if keywords and not any(kw.lower() in message.lower() for kw in keywords):
            continue
        commits.append({"hash": commit_hash, "message": message})
    return commits


def get_mingqiao_threads(agent_id: str, keywords: list, max_threads: int = 5) -> list:
    """Search ming-qiao inbox for relevant threads matching keywords."""
    try:
        url = f"{MINGQIAO_BASE}/api/inbox/{agent_id}?limit=50"
        req = urllib.request.Request(url)
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read())
    except Exception:
        return []

    matches = []
    for msg in data.get("messages", []):
        content = (msg.get("content", "") + " " + msg.get("subject", "")).lower()
        if any(kw.lower() in content for kw in keywords):
            matches.append({
                "subject": msg.get("subject", ""),
                "from": msg.get("from", ""),
                "timestamp": msg.get("timestamp", ""),
                "preview": msg.get("content", "")[:200],
            })
        if len(matches) >= max_threads:
            break
    return matches


def format_context(agent_id: str, commits: list, mq_threads: list,
                   keywords: list, max_tokens: int = 1500) -> str:
    """Format into token-budgeted text blob."""
    lines = []
    chars = 0
    max_chars = max_tokens * 4  # rough token estimate

    lines.append(f"# Context for {agent_id}")
    lines.append(f"Keywords: {', '.join(keywords)}")
    lines.append("")

    if commits:
        lines.append(f"## Recent Commits ({len(commits)})")
        for c in commits:
            entry = f"- {c['hash'][:8]}: {c['message']}"
            if chars + len(entry) > max_chars:
                lines.append(f"  ... ({len(commits) - len([l for l in lines if l.startswith('- ')])} more)")
                break
            lines.append(entry)
            chars += len(entry)

    if mq_threads:
        lines.append("")
        lines.append(f"## Relevant Threads ({len(mq_threads)})")
        for t in mq_threads:
            entry = f"- [{t['from']}] {t['subject']}: {t['preview'][:120]}"
            if chars + len(entry) > max_chars:
                break
            lines.append(entry)
            chars += len(entry)

    if not commits and not mq_threads:
        lines.append("No matching context found for the given keywords.")

    return "\n".join(lines)


def extract(agent_id: str, keywords: list, days: int = 7,
            max_tokens: int = 1500) -> dict:
    """Extract all relevant context for an agent."""
    all_commits = []
    worktrees = WORKTREE_PATHS.get(agent_id, [])
    for wt in worktrees:
        if wt.exists():
            all_commits.extend(get_git_commits(wt, days=days, keywords=keywords))

    mq_threads = get_mingqiao_threads(agent_id, keywords)
    context_text = format_context(agent_id, all_commits, mq_threads, keywords, max_tokens)

    return {
        "agent_id": agent_id,
        "keywords": keywords,
        "days_searched": days,
        "commits_found": len(all_commits),
        "mq_threads_found": len(mq_threads),
        "context": context_text,
        "token_estimate": len(context_text) // 4,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Extract agent context for colloquium briefing",
    )
    parser.add_argument("--agent", required=True,
                        help="Agent ID (luban, aleph, thales, mataya)")
    parser.add_argument("--keywords", required=True,
                        help="Comma-separated keywords to filter by")
    parser.add_argument("--days", type=int, default=7,
                        help="Days to look back (default: 7)")
    parser.add_argument("--max-tokens", type=int, default=1500,
                        help="Token budget (default: 1500)")
    parser.add_argument("--json", action="store_true",
                        help="Output as JSON")
    args = parser.parse_args()

    keywords = [k.strip() for k in args.keywords.split(",")]
    result = extract(
        agent_id=args.agent,
        keywords=keywords,
        days=args.days,
        max_tokens=args.max_tokens,
    )

    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(result.get("context", result.get("error", "")))
