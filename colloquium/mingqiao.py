"""Ming-qiao HTTP client for colloquium voice adapters."""

import json
from pathlib import Path

import aiohttp

MINGQIAO_URL = "http://localhost:7777"
# Shared token file — all agents' tokens in one file, loaded by agent_id
TOKEN_FILE = Path.home() / "astralmaris" / "ming-qiao" / "main" / "config" / "agent-tokens.json"
# Fallback to aleph's config copy if main isn't available
TOKEN_FILE_FALLBACK = Path.home() / "astralmaris" / "ming-qiao" / "aleph" / "config" / "agent-tokens.json"


def load_token(agent_id: str) -> str:
    tf = TOKEN_FILE if TOKEN_FILE.exists() else TOKEN_FILE_FALLBACK
    data = json.loads(tf.read_text())
    tokens = data.get("tokens", data)
    token = tokens.get(agent_id)
    if not token:
        raise ValueError(f"No token for agent {agent_id}")
    return token


async def read_thread(thread_id: str, token: str) -> dict:
    async with aiohttp.ClientSession() as session:
        headers = {"Authorization": f"Bearer {token}"}
        async with session.get(f"{MINGQIAO_URL}/api/thread/{thread_id}", headers=headers) as resp:
            resp.raise_for_status()
            return await resp.json()


async def post_reply(
    thread_id: str,
    agent_id: str,
    token: str,
    content: str,
    intent: str = "inform",
) -> dict:
    async with aiohttp.ClientSession() as session:
        headers = {
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        }
        payload = {
            "from": agent_id,
            "to": "council",
            "content": content,
            "intent": intent,
        }
        async with session.post(
            f"{MINGQIAO_URL}/api/thread/{thread_id}/reply",
            json=payload,
            headers=headers,
        ) as resp:
            resp.raise_for_status()
            return await resp.json()
