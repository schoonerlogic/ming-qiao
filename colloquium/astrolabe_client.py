"""
ASTROLABE MCP client for colloquium context assembly.

Queries the Graphiti MCP server (http://localhost:8001/mcp) for nodes, facts,
and episodes relevant to a colloquium proposal's context tags.

Uses the same SSE-based MCP protocol as oracle-ingest.py.
"""

import json
from dataclasses import dataclass

import aiohttp

ASTROLABE_MCP_URL = "http://localhost:8001/mcp"
GRAPHITI_GROUP_ID = "oracle_main"


@dataclass
class AstrolabeBriefing:
    """Structured ASTROLABE query results for a colloquium briefing."""
    nodes: list[dict]
    facts: list[dict]
    raw_query: str
    available: bool = True
    error: str = ""

    def to_text(self, max_tokens: int = 2000) -> str:
        """Render briefing as text for the context package."""
        if not self.available:
            return f"[ASTROLABE unavailable: {self.error}]"

        if not self.nodes and not self.facts:
            return f"[ASTROLABE query '{self.raw_query}' returned no relevant results. The graph may lack coverage for this topic.]"

        parts = []

        if self.nodes:
            parts.append("### Relevant Entities")
            for n in self.nodes:
                name = n.get("name", "?")
                summary = n.get("summary", "")
                labels = ", ".join(n.get("labels", []))
                line = f"- **{name}**"
                if labels:
                    line += f" ({labels})"
                if summary:
                    line += f": {summary}"
                parts.append(line)

        if self.facts:
            parts.append("\n### Relevant Facts")
            for f in self.facts:
                fact_text = f.get("fact", "")
                if f.get("is_invalid"):
                    fact_text += " [SUPERSEDED]"
                parts.append(f"- {fact_text}")

        text = "\n".join(parts)
        # Rough token budget enforcement (1 token ~ 4 chars)
        char_limit = max_tokens * 4
        if len(text) > char_limit:
            text = text[:char_limit] + "\n\n[... truncated to fit token budget]"
        return text


async def _mcp_call(session: aiohttp.ClientSession, session_id: str | None, tool_name: str, arguments: dict) -> tuple[dict, str]:
    """Make a single MCP tools/call request. Returns (result, session_id)."""
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    if session_id:
        headers["Mcp-Session-Id"] = session_id

    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments,
        },
    }

    async with session.post(ASTROLABE_MCP_URL, json=payload, headers=headers) as resp:
        new_session_id = resp.headers.get("Mcp-Session-Id", session_id or "")

        body = await resp.text()
        result = None

        # Try plain JSON first (server may respond directly)
        try:
            data = json.loads(body)
            if "result" in data:
                return data["result"], new_session_id
        except json.JSONDecodeError:
            pass

        # SSE response — read all events, find the JSON-RPC result
        for line in body.split("\n"):
            line = line.strip()
            if line.startswith("data: "):
                try:
                    data = json.loads(line[6:])
                    if "result" in data:
                        result = data["result"]
                except json.JSONDecodeError:
                    continue

        return result or {}, new_session_id


async def _mcp_init(session: aiohttp.ClientSession) -> str:
    """Initialize MCP session, return session ID."""
    payload = {
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "colloquium-voice", "version": "0.1.0"},
        },
    }

    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    async with session.post(ASTROLABE_MCP_URL, json=payload, headers=headers) as resp:
        session_id = resp.headers.get("Mcp-Session-Id", "")
        return session_id


async def query_astrolabe(context_tags: list[str], max_nodes: int = 10, max_facts: int = 10) -> AstrolabeBriefing:
    """Query ASTROLABE for entities and facts relevant to context tags."""
    query = " ".join(context_tags)

    try:
        timeout = aiohttp.ClientTimeout(total=15)
        async with aiohttp.ClientSession(timeout=timeout) as session:
            session_id = await _mcp_init(session)

            # Fan out: nodes and facts queries
            nodes_result, session_id = await _mcp_call(session, session_id, "search_nodes", {
                "query": query,
                "group_ids": [GRAPHITI_GROUP_ID],
                "max_nodes": max_nodes,
            })

            facts_result, _ = await _mcp_call(session, session_id, "search_memory_facts", {
                "query": query,
                "group_ids": [GRAPHITI_GROUP_ID],
                "max_facts": max_facts,
            })

            # Extract from MCP content blocks
            nodes = _extract_content(nodes_result, "nodes")
            facts = _extract_content(facts_result, "facts")

            return AstrolabeBriefing(
                nodes=nodes,
                facts=facts,
                raw_query=query,
                available=True,
            )

    except Exception as e:
        return AstrolabeBriefing(
            nodes=[],
            facts=[],
            raw_query=query,
            available=False,
            error=str(e),
        )


def _extract_content(result: dict, key: str) -> list[dict]:
    """Extract structured data from MCP tool call result."""
    # MCP returns content as array of content blocks
    content = result.get("content", [])
    for block in content:
        if block.get("type") == "text":
            try:
                parsed = json.loads(block["text"])
                if isinstance(parsed, dict):
                    return parsed.get(key, [])
                if isinstance(parsed, list):
                    return parsed
            except (json.JSONDecodeError, KeyError):
                continue
    return []
