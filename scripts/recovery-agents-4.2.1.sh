#! /bin/bash

echo "=== Claude Code agents (.mcp.json) ==="
for path in \
  /Users/proteus/astralmaris/astral-forge/aleph/.mcp.json \
  /Users/proteus/astralmaris/inference-kitchen/luban/.mcp.json \
  /Users/proteus/astralmaris/everwatch-spire/ogma/.mcp.json \
; do
  agent_id=$(python3 -c "import json; print(json.load(open('$path'))['mcpServers']['ming-qiao']['env']['MING_QIAO_AGENT_ID'])" 2>/dev/null || echo "MISSING")
  config=$(python3 -c "import json; print(json.load(open('$path'))['mcpServers']['ming-qiao']['env']['MING_QIAO_CONFIG'])" 2>/dev/null || echo "MISSING")
  echo "  $(basename $(dirname $(dirname $path)))/$(basename $(dirname $path)): agent_id=$agent_id config=$config"
done

echo ""
echo "=== Kimi global config ==="
kimi_id=$(python3 -c "import json; print(json.load(open('/Users/proteus/.kimi/mcp.json'))['mcpServers']['ming-qiao']['env']['MING_QIAO_AGENT_ID'])" 2>/dev/null || echo "MISSING")
echo "  ~/.kimi/mcp.json: agent_id=$kimi_id (should be laozi-jung)"

echo ""
echo "=== Claude Desktop ==="
desktop_id=$(python3 -c "
import json
c = json.load(open('/Users/proteus/Library/Application Support/Claude/claude_desktop_config.json'))
print(c['mcpServers']['ming-qiao']['env']['MING_QIAO_AGENT_ID'])
" 2>/dev/null || echo "MISSING")
echo "  Claude Desktop: agent_id=$desktop_id (should be thales)"
