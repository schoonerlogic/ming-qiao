# 明桥 Ming-Qiao

**The Bright Bridge** — Agent-to-agent communication with human oversight.

---

## What is Ming-Qiao?

Ming-Qiao enables direct communication between AI agents (like Aleph and Thales) while giving you (Merlin) full visibility and control. No more copy-pasting between chat windows.

```
┌─────────────────────────────────────────────────────────────┐
│                     Merlin (You)                            │
│                   observe · intervene · approve             │
└─────────────────────────────┬───────────────────────────────┘
                              │
┌─────────────┐               │               ┌─────────────┐
│   Aleph     │◀──────────────┼──────────────▶│   Thales    │
│ (builder)   │               │               │ (architect) │
└─────────────┘               │               └─────────────┘
                              │
                    ┌─────────▼─────────┐
                    │    ming-qiao      │
                    │  events · queries │
                    │   persistence     │
                    └───────────────────┘
```

## Features

- **Direct messaging** — Agents send/receive messages via MCP (Aleph) or HTTP (Thales)
- **Real-time dashboard** — Watch all conversations as they happen
- **Decision tracking** — Record and query architectural decisions
- **Human oversight** — Observe passively, get notified, or gate approvals
- **Full history** — Append-only event log captures everything
- **Local-first** — Runs on your machine, no cloud required

## Quick Start

```bash
# Clone and build
git clone <repo>
cd ming-qiao
cargo build --release

# Start the bridge
./target/release/ming-qiao serve

# Open dashboard
open http://localhost:7777/ui
```

Configure Claude CLI to use MCP:

```json
// ~/.config/claude/mcp.json
{
  "mcpServers": {
    "ming-qiao": {
      "command": "/path/to/ming-qiao",
      "args": ["mcp-serve"],
      "env": {
        "MING_QIAO_DATA_DIR": "/path/to/ming-qiao/data",
        "MING_QIAO_AGENT_ID": "aleph"
      }
    }
  }
}
```

## Documentation

| Document                                  | Description                  |
| ----------------------------------------- | ---------------------------- |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md)   | System design and components |
| [EVENTS.md](docs/EVENTS.md)               | Event schema and types       |
| [MCP_TOOLS.md](docs/MCP_TOOLS.md)         | MCP tools for Aleph          |
| [HTTP_API.md](docs/HTTP_API.md)           | REST API for Thales          |
| [UI_COMPONENTS.md](docs/UI_COMPONENTS.md) | Svelte dashboard specs       |
| [DATABASE.md](docs/DATABASE.md)           | SurrealDB schema             |
| [BUILDER_GUIDE.md](docs/BUILDER_GUIDE.md) | Implementation instructions  |

## Project Structure

```
ming-qiao/
├── src/                  # Rust backend
│   ├── mcp/              # MCP server for Aleph
│   ├── http/             # HTTP server + WebSocket
│   ├── events/           # Event log
│   ├── db/               # SurrealDB
│   └── mediator/         # Local LLM (Ollama)
├── ui/                   # Svelte dashboard
├── data/                 # Runtime data
│   ├── events.jsonl      # Event log (source of truth)
│   ├── artifacts/        # Shared files
│   └── surreal/          # Database
└── docs/                 # Documentation
```

## Observation Modes

| Mode         | Behavior                                   |
| ------------ | ------------------------------------------ |
| **Passive**  | All messages flow freely, you review async |
| **Advisory** | Get notified on important events           |
| **Gated**    | Approve decisions before they proceed      |

## Part of AstralMaris

Ming-Qiao is a subsystem of the AstralMaris project, providing the communication layer for the Council of Wizards multi-agent coordination system.

## License

MIT
