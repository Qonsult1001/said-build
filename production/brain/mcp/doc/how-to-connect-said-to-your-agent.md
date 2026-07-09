# How to connect your brain to your agent (Claude Desktop, Claude Code, or Cursor)

**Goal:** wire the `said-mcp` server into your AI agent so it can save and recall memories for you.

> **You may not need this guide.** The one-line installer (see the
> **[install guide](how-to-install-said.md)**) already auto-connects the brain to any agent it found and
> tells you which ones. If it said e.g. *"connected … claude-code, cursor"*, you're done — just restart
> that agent. Use this guide to connect an agent it **didn't** auto-detect, or to do it by hand.

**Before you start:** you've installed `said-mcp` (**[install guide](how-to-install-said.md)** — check
with `said-mcp --version`), and you know the **full path to a brain file** you want to use. Don't have a
brain yet? Create one first with the terminal command `said create /full/path/to/my-brain.said`, or let
the config below point at a path that doesn't exist yet — the server creates it empty on first run.

All three agents use the **same config block** — only *where* you put it differs. The block:

    {
      "mcpServers": {
        "said-brain": {
          "command": "said-mcp",
          "args": ["--path", "/full/path/to/my-brain.said"]
        }
      }
    }

Two things to get right in that block:

- **`command`** — if `said-mcp` is on your PATH (the one-line installer and native installers do this),
  leave it as `"said-mcp"`. If you installed the zip manually, use the **full path** to the binary
  instead, e.g. `"C:\\said\\said-mcp.exe"` or `"/Users/you/said/said-mcp"`.
- **`--path`** — the **full, absolute** path to your `.said` brain file (not a relative path).

Now pick your agent:

## Claude Desktop

1. Open the config file (create it if it doesn't exist):
   - **macOS** → `~/Library/Application Support/Claude/claude_desktop_config.json`
   - **Windows** → `%APPDATA%\Claude\claude_desktop_config.json`
2. Paste the config block above into it. If the file already has other `mcpServers`, add `said-brain`
   as another entry inside the existing `"mcpServers": { … }` object (don't create a second one).
3. **Fully quit and reopen Claude Desktop** (not just close the window).
4. **You should see:** a tools/MCP indicator showing **said-brain** connected (Claude Desktop shows
   connected MCP servers in its UI). If it's not there, see *Troubleshooting* below.

## Claude Code (CLI / IDE)

Claude Code reads MCP servers from a `.mcp.json` in your project root (or your user config). Easiest:

1. From your project folder, run:

       claude mcp add said-brain said-mcp -- --path /full/path/to/my-brain.said

   (Or create `.mcp.json` in the project root with the config block above.)
2. **You should see:** `said-brain` listed when you run `claude mcp list`, marked connected.

## Cursor

1. Open Cursor's MCP settings: **Settings → MCP** (or edit `~/.cursor/mcp.json`).
2. Add the same config block (the `said-brain` entry under `mcpServers`).
3. **Reload Cursor** (Command Palette → *Reload Window*).
4. **You should see:** `said-brain` in Cursor's MCP servers list, connected.

## Confirm it actually works

You don't need to configure any steering — the server tells the agent how to use itself on connect. Just
test it in a normal chat with your agent:

    Remember that my wifi password is sunflower-42.

You should see the agent confirm it saved that. Then, to prove it stuck, **open a fresh chat** and ask:

    What's my wifi password?

The agent should answer **sunflower-42**, recalled from the brain. That's connected and working.

## Troubleshooting

- **The server doesn't appear / shows as failed.** Almost always the `--path` or `command` is wrong.
  Use **absolute** paths for both. Test the binary runs at all: in a terminal, `said-mcp --version`
  should print a version (if "command not found", it's not on PATH — use its full path in `command`).
- **"connected" but the agent doesn't save/recall.** Ask it directly: "use the said-brain memory —
  remember X." Some agents need you to reference the tool once before they lean on it.
- **You edited the config but nothing changed.** You must fully restart / reload the agent after editing
  its MCP config — a new chat is not enough.
- **Windows paths.** In JSON, backslashes must be doubled: `"C:\\Users\\you\\my-brain.said"` (or use
  forward slashes: `"C:/Users/you/my-brain.said"`).

## Next

- **[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)** — the full walk-through from zero.
- **[Store and recall notes through your agent](how-to-store-and-recall-notes-with-an-agent.md)** — the
  everyday pattern once you're connected.
