#!/usr/bin/env python3
"""
Generate MCP Streamable HTTP configs for all agents.

Reads agent-capabilities.toml and generates:
- OpenCode configs (opencode.json) for Luban + Jikimi
- Kimi configs (kimi-mcp-http.json) for Mataya + Laozi-Jung

Usage:
    python3 scripts/generate-streamable-http-configs.py [--output-dir ./output]

Agent: Luban
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

DEFAULT_MCP_URL = "http://localhost:7777/mcp"
AGENT_CAPABILITIES_PATH = (
    Path(__file__).parent.parent.parent / "main" / "config" / "agent-capabilities.toml"
)


def parse_toml_simple(path: Path) -> dict[str, dict[str, Any]]:
    """Simple TOML parser for agent-capabilities format."""
    agents = {}
    current_agent = None

    content = path.read_text()

    for line in content.split("\n"):
        line = line.strip()

        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1]
            if "." not in section:
                current_agent = section
                agents[current_agent] = {}
        elif "=" in line and current_agent:
            key, _, value = line.partition("=")
            key = key.strip()
            value = value.strip().strip('"').strip("'")
            agents[current_agent][key] = value

    return agents


def generate_opencode_config(agent_id: str, mcp_url: str) -> dict[str, Any]:
    """Generate OpenCode config for Streamable HTTP MCP."""
    return {
        "$schema": "https://opencode.ai/config.json",
        "model": "ollama/qwen3:8b" if agent_id == "jikimi" else "glm-5",
        "permission": {"edit": "ask", "bash": {"*": "allow"}},
        "provider": {"ollama": {"options": {"baseURL": "http://localhost:11434/v1"}}},
        "mcp": {"ming-qiao": {"type": "remote", "url": mcp_url}},
    }


def generate_kimi_config(agent_id: str, mcp_url: str) -> dict[str, Any]:
    """Generate Kimi config for Streamable HTTP MCP."""
    return {"mcpServers": {"ming-qiao": {"transport": "http", "url": mcp_url}}}


def main():
    parser = argparse.ArgumentParser(description="Generate MCP Streamable HTTP configs")
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("./output"),
        help="Output directory for generated configs",
    )
    parser.add_argument(
        "--mcp-url", default=DEFAULT_MCP_URL, help="MCP Streamable HTTP URL"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="Print configs without writing files"
    )
    args = parser.parse_args()

    if not AGENT_CAPABILITIES_PATH.exists():
        print(f"Error: agent-capabilities.toml not found at {AGENT_CAPABILITIES_PATH}")
        sys.exit(1)

    agents = parse_toml_simple(AGENT_CAPABILITIES_PATH)

    opencode_agents = ["luban", "jikimi"]
    kimi_agents = ["mataya", "laozi-jung"]

    if not args.dry_run:
        args.output_dir.mkdir(parents=True, exist_ok=True)
        (args.output_dir / "opencode").mkdir(exist_ok=True)
        (args.output_dir / "kimi").mkdir(exist_ok=True)

    print(f"Generating configs for MCP URL: {args.mcp_url}")
    print()

    for agent_id in opencode_agents:
        if agent_id not in agents:
            print(f"Warning: {agent_id} not found in agent-capabilities.toml, skipping")
            continue

        config = generate_opencode_config(agent_id, args.mcp_url)
        config_path = args.output_dir / "opencode" / f"{agent_id}-opencode.json"

        if args.dry_run:
            print(f"\n--- {agent_id} OpenCode config ---")
            print(json.dumps(config, indent=2))
        else:
            config_path.write_text(json.dumps(config, indent=2))
            print(f"Written: {config_path}")

    for agent_id in kimi_agents:
        if agent_id not in agents:
            print(f"Warning: {agent_id} not found in agent-capabilities.toml, skipping")
            continue

        config = generate_kimi_config(agent_id, args.mcp_url)
        config_path = args.output_dir / "kimi" / f"{agent_id}-kimi-mcp.json"

        if args.dry_run:
            print(f"\n--- {agent_id} Kimi config ---")
            print(json.dumps(config, indent=2))
        else:
            config_path.write_text(json.dumps(config, indent=2))
            print(f"Written: {config_path}")

    print()
    print("Done. To apply configs:")
    print()
    print("OpenCode (Luban, Jikimi):")
    print("  cp output/opencode/{agent}-opencode.json ~/.config/opencode/config.json")
    print()
    print("Kimi (Mataya, Laozi-Jung):")
    print("  kimi mcp add --transport http ming-qiao http://localhost:7777/mcp")
    print()


if __name__ == "__main__":
    main()
