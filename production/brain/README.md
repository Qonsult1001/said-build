# said — Brain (Personal / Free tier) — production deliverable

Your portable personal memory in a single file. Store notes, ask questions, get them back — on the
terminal or through an AI agent. One `.said` brain file works both ways.

This folder is split by how you use the brain. Pick the one that fits you:

| Folder | For | What's inside |
|---|---|---|
| **[`cli/`](cli/)** | Using the brain yourself from a terminal | `said.exe` + install guide, a zero-to-first-success tutorial, and how-to guides |
| **[`mcp/`](mcp/)** | Letting an AI agent (Claude, Cursor, …) read and write the brain for you | `said-mcp.exe` + install guide, an agent-connect tutorial, and how-to guides |

**Same brain, either way.** A brain you build on the CLI opens in an agent, and vice versa — it's one
portable `.said` file. Many people use both: the agent saves things automatically during work, and the
CLI is there when you want to search or manage the file directly.

## What this is

- **A single-file memory** — no server, no database, no cloud. Your memories live in one `.said` file
  you own and can move anywhere.
- **Offline and private** — the binary never calls an LLM or the network. It runs entirely on your
  machine.
- **Small** — the whole engine (including the embedded encoder) is one self-contained executable.

## Recall, in plain terms

You store memories, and when you ask a question the brain returns the **most relevant few** (the top
handful) — the right memory is essentially always in that set. Through an agent, the agent reads those
and answers you directly; on the CLI, you read them yourself.

## Start here

1. **Install** — both folders include `how-to-install-said.md` (Windows / macOS / Linux).
2. **Pick your path** — [`cli/`](cli/) to drive it yourself, or [`mcp/`](mcp/) to connect an agent.
3. **Follow the tutorial** in that folder — each takes you from nothing to your first stored-and-recalled
   memory.

## Contents

```
brain/
├── cli/                        # terminal use — you type `said` commands
│   ├── said.exe
│   ├── README.md               # CLI quick-start + guide index
│   └── doc/                     # all CLI documentation
│       ├── how-to-install-said.md
│       ├── tutorial-your-first-brain.md
│       ├── cli-reference.md     # every command/flag
│       └── how-to-*.md          # find, import, recover, set-default, store/recall, versions
└── mcp/                        # agent use — an AI agent drives the brain
    ├── said-mcp.exe            # agent steering is built in (server sends it on connect)
    ├── README.md               # MCP quick-start, tool surface, connect config
    └── doc/                     # all MCP documentation
        ├── how-to-install-said.md
        ├── tutorial-connect-your-brain-to-an-agent.md
        ├── how-to-*.md          # store/recall via agent, move brain across agents
        └── verify-mcp.md        # how to verify the MCP server end-to-end
```
