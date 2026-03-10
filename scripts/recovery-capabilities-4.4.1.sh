#! /bin/bash

echo "=== Agents in agent-capabilities.toml ==="
grep '^\[agents\.' /Users/proteus/astralmaris/ming-qiao/main/config/agent-capabilities.toml | \
  sed 's/\[agents\.\(.*\)\]/  \1/'

echo ""
echo "=== Expected agents ==="
echo "  aleph, luban, ogma, mataya, laozi-jung, thales, merlin"
```

If an agent is missing, add it to `agent-capabilities.toml` and restart the awakener:

```bash
launchctl kickstart -k gui/$(id -u)/com.astralmaris.council-awakener
