#!/usr/bin/env python3
"""ORACLE Research Intelligence — Graph Query CLI

Query the Graphiti MCP knowledge graph from the terminal.
Supports natural language queries against ORACLE's entity/fact graph.

Usage:
    oracle-query.py nodes "transformer attention mechanisms"
    oracle-query.py facts "LoRA training techniques"
    oracle-query.py facts "adapter composition" --center-node <uuid>
    oracle-query.py episodes
    oracle-query.py status
    oracle-query.py search "knowledge distillation"   # search both nodes + facts

Requires: Graphiti MCP server running at localhost:8000 (via docker-compose-oracle.yml)
"""

import argparse
import json
import sys
import textwrap
import urllib.error
import urllib.request
import uuid as uuid_mod

MCP_URL = "http://localhost:8000/mcp"
DEFAULT_GROUP = "oracle-main"

# Terminal colors (ANSI)
BOLD = "\033[1m"
DIM = "\033[2m"
CYAN = "\033[36m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
RED = "\033[31m"
MAGENTA = "\033[35m"
RESET = "\033[0m"

# MCP session state
_session_id = None


def _parse_sse_response(body: str) -> dict | None:
    """Extract JSON-RPC response from SSE stream body."""
    for line in body.split("\n"):
        line = line.strip()
        if line.startswith("data: "):
            try:
                return json.loads(line[6:])
            except json.JSONDecodeError:
                continue
    return None


def _mcp_post(payload: dict, timeout: int = 30) -> tuple[dict | None, str | None]:
    """POST to MCP endpoint, return (parsed_response, session_id)."""
    global _session_id
    data = json.dumps(payload).encode()
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    if _session_id:
        headers["Mcp-Session-Id"] = _session_id

    req = urllib.request.Request(MCP_URL, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        # Capture session ID from response headers
        sid = resp.headers.get("Mcp-Session-Id")
        if sid:
            _session_id = sid

        body = resp.read().decode()
        # Try JSON first, then SSE
        try:
            return json.loads(body), _session_id
        except json.JSONDecodeError:
            parsed = _parse_sse_response(body)
            return parsed, _session_id


def mcp_initialize() -> bool:
    """Initialize MCP session (required before tool calls)."""
    global _session_id

    # Step 1: initialize
    init_payload = {
        "jsonrpc": "2.0",
        "id": str(uuid_mod.uuid4()),
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "oracle-query", "version": "1.0"},
        },
    }
    result, _ = _mcp_post(init_payload)
    if not result or "error" in result:
        return False

    # Step 2: send initialized notification
    notif_payload = {
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
    }
    _mcp_post(notif_payload, timeout=5)
    return True


def call_tool(tool_name: str, arguments: dict) -> dict:
    """Call an MCP tool and return the parsed result."""
    global _session_id

    # Ensure session is initialized
    if not _session_id:
        if not mcp_initialize():
            return {"error": "Failed to initialize MCP session"}

    payload = {
        "jsonrpc": "2.0",
        "id": str(uuid_mod.uuid4()),
        "method": "tools/call",
        "params": {"name": tool_name, "arguments": arguments},
    }

    try:
        result, _ = _mcp_post(payload, timeout=60)
    except urllib.error.URLError as e:
        return {"error": str(e)}
    except Exception as e:
        return {"error": str(e)}

    if not result:
        return {"error": "Empty response from MCP server"}

    if "error" in result:
        err = result["error"]
        return {"error": err.get("message", str(err)) if isinstance(err, dict) else str(err)}

    # Extract content from MCP tool result
    content = result.get("result", {})
    if isinstance(content, dict) and "content" in content:
        for item in content["content"]:
            if isinstance(item, dict) and item.get("type") == "text":
                try:
                    return json.loads(item["text"])
                except (json.JSONDecodeError, KeyError):
                    return {"message": item.get("text", "")}
    return content


def format_node(node: dict, index: int) -> str:
    """Format a single node for terminal display."""
    labels = ", ".join(node.get("labels", []))
    name = node.get("name", "?")
    summary = node.get("summary", "")
    group = node.get("group_id", "")
    uid = node.get("uuid", "")[:12]

    lines = [f"  {BOLD}{CYAN}[{index}]{RESET} {BOLD}{name}{RESET}"]
    if labels:
        lines.append(f"      {DIM}Type:{RESET} {YELLOW}{labels}{RESET}")
    if summary:
        wrapped = textwrap.fill(
            summary, width=72,
            initial_indent="      ", subsequent_indent="      ",
        )
        lines.append(wrapped)
    if uid:
        lines.append(f"      {DIM}uuid: {uid}…  group: {group}{RESET}")
    return "\n".join(lines)


def format_fact(fact: dict, index: int) -> str:
    """Format a single fact (edge) for terminal display."""
    src = fact.get("source_node", {})
    tgt = fact.get("target_node", {})
    rel = fact.get("relationship", "?")
    src_name = src.get("name", "?")
    tgt_name = tgt.get("name", "?")
    invalid = fact.get("is_invalid", False)
    uid = fact.get("uuid", "")[:12]

    status = f" {RED}[SUPERSEDED]{RESET}" if invalid else ""
    lines = [
        f"  {BOLD}{GREEN}[{index}]{RESET} {BOLD}{src_name}{RESET} "
        f"—{MAGENTA}[{rel}]{RESET}→ {BOLD}{tgt_name}{RESET}{status}"
    ]
    attrs = fact.get("attributes", {})
    if attrs:
        for k, v in attrs.items():
            if k not in ("embedding", "fact_embedding"):
                lines.append(f"      {DIM}{k}:{RESET} {v}")
    if uid:
        lines.append(f"      {DIM}uuid: {uid}…{RESET}")
    return "\n".join(lines)


def format_episode(ep: dict, index: int) -> str:
    """Format a single episode for terminal display."""
    name = ep.get("name", "?")
    source = ep.get("source", "?")
    desc = ep.get("source_description", "")
    created = ep.get("created_at", "")
    content = ep.get("content", "")
    uid = ep.get("uuid", "")[:12]

    lines = [f"  {BOLD}{YELLOW}[{index}]{RESET} {BOLD}{name}{RESET}"]
    lines.append(f"      {DIM}Source:{RESET} {source}" + (f" ({desc})" if desc else ""))
    if created:
        lines.append(f"      {DIM}Created:{RESET} {created}")
    if content:
        preview = content[:120] + ("…" if len(content) > 120 else "")
        lines.append(f"      {preview}")
    if uid:
        lines.append(f"      {DIM}uuid: {uid}…{RESET}")
    return "\n".join(lines)


def cmd_search_nodes(args):
    """Search for entity nodes."""
    params = {"query": args.query, "max_nodes": args.max}
    if args.group:
        params["group_ids"] = [args.group]
    if args.entity_type:
        params["entity_types"] = args.entity_type

    print(f"\n{BOLD}Searching nodes:{RESET} {args.query}\n")
    result = call_tool("search_nodes", params)

    if "error" in result:
        print(f"{RED}Error:{RESET} {result['error']}")
        return 1

    nodes = result.get("nodes", [])
    if not nodes:
        print(f"  {DIM}No nodes found.{RESET}")
        return 0

    print(f"  {DIM}Found {len(nodes)} node(s):{RESET}\n")
    for i, node in enumerate(nodes, 1):
        print(format_node(node, i))
        print()
    return 0


def cmd_search_facts(args):
    """Search for facts (edges) between entities."""
    params = {"query": args.query, "max_facts": args.max}
    if args.group:
        params["group_ids"] = [args.group]
    if args.center_node:
        params["center_node_uuid"] = args.center_node

    print(f"\n{BOLD}Searching facts:{RESET} {args.query}\n")
    result = call_tool("search_memory_facts", params)

    if "error" in result:
        print(f"{RED}Error:{RESET} {result['error']}")
        return 1

    facts = result.get("facts", [])
    if not facts:
        print(f"  {DIM}No facts found.{RESET}")
        return 0

    print(f"  {DIM}Found {len(facts)} fact(s):{RESET}\n")
    for i, fact in enumerate(facts, 1):
        print(format_fact(fact, i))
        print()
    return 0


def cmd_search_all(args):
    """Search both nodes and facts."""
    params_nodes = {"query": args.query, "max_nodes": args.max}
    params_facts = {"query": args.query, "max_facts": args.max}
    if args.group:
        params_nodes["group_ids"] = [args.group]
        params_facts["group_ids"] = [args.group]

    print(f"\n{BOLD}Searching graph:{RESET} {args.query}\n")

    # Nodes
    print(f"{BOLD}{CYAN}── Entities ──{RESET}\n")
    node_result = call_tool("search_nodes", params_nodes)
    if "error" in node_result:
        print(f"  {RED}Error:{RESET} {node_result['error']}")
    else:
        nodes = node_result.get("nodes", [])
        if not nodes:
            print(f"  {DIM}No nodes found.{RESET}")
        else:
            for i, node in enumerate(nodes, 1):
                print(format_node(node, i))
                print()

    # Facts
    print(f"\n{BOLD}{GREEN}── Facts ──{RESET}\n")
    fact_result = call_tool("search_memory_facts", params_facts)
    if "error" in fact_result:
        print(f"  {RED}Error:{RESET} {fact_result['error']}")
    else:
        facts = fact_result.get("facts", [])
        if not facts:
            print(f"  {DIM}No facts found.{RESET}")
        else:
            for i, fact in enumerate(facts, 1):
                print(format_fact(fact, i))
                print()

    return 0


def cmd_episodes(args):
    """List episodes in the graph."""
    params = {"max_episodes": args.max}
    if args.group:
        params["group_ids"] = [args.group]

    print(f"\n{BOLD}Episodes:{RESET}\n")
    result = call_tool("get_episodes", params)

    if "error" in result:
        print(f"{RED}Error:{RESET} {result['error']}")
        return 1

    episodes = result.get("episodes", [])
    if not episodes:
        print(f"  {DIM}No episodes found.{RESET}")
        return 0

    print(f"  {DIM}Found {len(episodes)} episode(s):{RESET}\n")
    for i, ep in enumerate(episodes, 1):
        print(format_episode(ep, i))
        print()
    return 0


def cmd_status(args):
    """Check MCP server and database status."""
    print(f"\n{BOLD}ORACLE Status:{RESET}\n")

    # Health check
    try:
        req = urllib.request.Request("http://localhost:8000/health")
        with urllib.request.urlopen(req, timeout=5) as resp:
            print(f"  HTTP health: {GREEN}OK{RESET} ({resp.status})")
    except Exception as e:
        print(f"  HTTP health: {RED}FAIL{RESET} ({e})")
        return 1

    # MCP status tool
    result = call_tool("get_status", {})
    if "error" in result:
        print(f"  MCP status: {RED}{result['error']}{RESET}")
    else:
        status = result.get("status", "unknown")
        msg = result.get("message", "")
        color = GREEN if status == "ok" else RED
        print(f"  MCP status: {color}{status}{RESET} — {msg}")

    # Ollama models
    print(f"\n  {BOLD}Ollama models:{RESET}")
    try:
        req = urllib.request.Request("http://localhost:11434/api/tags")
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read())
            for m in data.get("models", []):
                name = m.get("name", "?")
                size_gb = m.get("size", 0) / 1024 / 1024 / 1024
                print(f"    {name} ({size_gb:.1f} GB)")
    except Exception as e:
        print(f"    {RED}Ollama unavailable:{RESET} {e}")

    # FalkorDB
    print(f"\n  {BOLD}FalkorDB:{RESET}")
    try:
        import socket
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(2)
        s.connect(("localhost", 6379))
        s.sendall(b"PING\r\n")
        reply = s.recv(32).decode().strip()
        s.close()
        if "PONG" in reply:
            print(f"    Redis port 6379: {GREEN}PONG{RESET}")
        else:
            print(f"    Redis port 6379: {YELLOW}{reply}{RESET}")
    except Exception as e:
        print(f"    Redis port 6379: {RED}not reachable{RESET} ({e})")

    # FalkorDB memory (via INFO command)
    try:
        import socket
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(2)
        s.connect(("localhost", 6379))
        s.sendall(b"INFO memory\r\n")
        reply = b""
        while True:
            chunk = s.recv(4096)
            if not chunk:
                break
            reply += chunk
            if b"\r\n\r\n" in reply:
                break
        s.close()
        text = reply.decode()
        for line in text.split("\r\n"):
            if line.startswith("used_memory_human:"):
                mem = line.split(":")[1]
                print(f"    Memory used: {mem}")
            elif line.startswith("used_memory_peak_human:"):
                mem = line.split(":")[1]
                print(f"    Memory peak: {mem}")
    except Exception:
        pass

    print()
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="ORACLE Research Intelligence — Graph Query CLI",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=textwrap.dedent("""\
            Examples:
              %(prog)s nodes "transformer architectures"
              %(prog)s facts "LoRA training" --max 5
              %(prog)s search "knowledge distillation"
              %(prog)s episodes --max 20
              %(prog)s status
        """),
    )
    parser.add_argument(
        "--group", default=DEFAULT_GROUP,
        help=f"Graph group ID (default: {DEFAULT_GROUP})",
    )
    parser.add_argument(
        "--no-color", action="store_true",
        help="Disable colored output",
    )

    sub = parser.add_subparsers(dest="command", required=True)

    # nodes
    p_nodes = sub.add_parser("nodes", help="Search entity nodes")
    p_nodes.add_argument("query", help="Natural language search query")
    p_nodes.add_argument("--max", type=int, default=10, help="Max results (default: 10)")
    p_nodes.add_argument("--entity-type", action="append", help="Filter by entity type (repeatable)")

    # facts
    p_facts = sub.add_parser("facts", help="Search facts (relationships)")
    p_facts.add_argument("query", help="Natural language search query")
    p_facts.add_argument("--max", type=int, default=10, help="Max results (default: 10)")
    p_facts.add_argument("--center-node", help="Center search around a node UUID")

    # search (both)
    p_search = sub.add_parser("search", help="Search nodes + facts together")
    p_search.add_argument("query", help="Natural language search query")
    p_search.add_argument("--max", type=int, default=5, help="Max results per type (default: 5)")

    # episodes
    p_eps = sub.add_parser("episodes", help="List ingested episodes")
    p_eps.add_argument("--max", type=int, default=10, help="Max results (default: 10)")

    # status
    sub.add_parser("status", help="Check ORACLE system status")

    args = parser.parse_args()

    # Disable colors if requested or not a TTY
    if args.no_color or not sys.stdout.isatty():
        global BOLD, DIM, CYAN, GREEN, YELLOW, RED, MAGENTA, RESET
        BOLD = DIM = CYAN = GREEN = YELLOW = RED = MAGENTA = RESET = ""

    commands = {
        "nodes": cmd_search_nodes,
        "facts": cmd_search_facts,
        "search": cmd_search_all,
        "episodes": cmd_episodes,
        "status": cmd_status,
    }

    return commands[args.command](args)


if __name__ == "__main__":
    sys.exit(main() or 0)
