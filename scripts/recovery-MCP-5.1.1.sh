#! /bin/bash

echo "=== MCP config existence check ==="
for f in \
  /Users/proteus/astralmaris/astral-forge/aleph/.mcp.json \
  /Users/proteus/astralmaris/inference-kitchen/luban/.mcp.json \
  /Users/proteus/astralmaris/everwatch-spire/ogma/.mcp.json \
  /Users/proteus/astralmaris/latent-winds/mataya/.mcp.json \
  "/Users/proteus/Library/Application Support/Claude/claude_desktop_config.json" \
  /Users/proteus/.kimi/mcp.json \
; do
  if [ -f "$f" ]; then echo "  OK: $f"; else echo "  MISSING: $f"; fi
done
