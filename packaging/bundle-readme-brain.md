# said — your portable memory in one file

Thanks for downloading **said** (brain / free tier). This archive contains everything you need to run it:

| File | What it is |
|---|---|
| `said` / `said.exe` | the command-line tool — store notes and ask questions from a terminal |
| `said-mcp` / `said-mcp.exe` | the memory server — lets an AI agent (Claude, Cursor, …) use the brain for you |
| `said-setup-*.exe` (Windows) | one-click installer (optional — you can also just run the binaries) |

Your memory lives in a single `.said` file you create. No server, no database, no cloud — the binary
never calls the network. It's yours, and it moves anywhere.

## 60-second first success (terminal)

    said create my-brain.said
    said --path my-brain.said add "Wifi password is sunflower-42" --id wifi
    said --path my-brain.said ask "what is the wifi password"

You'll see the brain return the note you saved — found by meaning, not exact words. That's the whole idea:
the chat forgets, the file remembers.

Tip: run `said use my-brain.said` once, then you can drop `--path` on every command.

## Use it through an AI agent instead

Point your agent (Claude Desktop, Claude Code, Cursor, …) at the memory server by adding this to its MCP
config, using the **full path** to your brain file:

    {
      "mcpServers": {
        "said": { "command": "said-mcp", "args": ["--path", "/full/path/to/my-brain.said"] }
      }
    }

Restart the agent, then just talk to it — it saves what's worth keeping and recalls it later, even in a
new chat. (The one-line installers below wire this up for you automatically.)

## Install (optional — adds `said` to your PATH and connects your agents)

- **Windows** (PowerShell): `irm https://github.com/Qonsult1001/said-build/releases/latest/download/install.ps1 | iex`
- **Linux / macOS**: `curl -fsSL https://github.com/Qonsult1001/said-build/releases/latest/download/install.sh | sh`

Or run the `said-setup-*.exe` in this archive (Windows), or just put these binaries somewhere on your PATH.

## Full step-by-step guides

The complete tutorials and how-to guides live online (they're kept in sync with each release):

- **CLI guides** — install, your-first-brain tutorial, command reference, find / import / recover / tag:
  <https://github.com/Qonsult1001/said-build/tree/master/production/brain/cli/doc>
- **Agent (MCP) guides** — connect your brain to an agent, store/recall through it, organize with tags,
  move it between agents, clean up:
  <https://github.com/Qonsult1001/said-build/tree/master/production/brain/mcp/doc>

## What this brain does (and doesn't)

- ✅ Stores **text memories** — notes, facts, decisions, preferences — and finds them by meaning.
- ✅ Tags + concepts to organize (`list-tags`, `list-concepts`), version history, a recycle bin.
- ❌ Does **not** index source code (symbol/AST search over a repo) — that's the *coding* build of said.
  Pasting code here saves it as a text note you can recall by meaning, not as searchable code.

Questions or issues: <https://github.com/Qonsult1001/said-build/issues>
