# How to install `said` for use with an AI agent (MCP)

> Goal: get the `said-mcp` server installed on your computer so your AI agent (Claude Desktop, Claude
> Code, Cursor, …) can use it as a memory. When you're done, you'll have `said-mcp` on your machine and
> be ready to connect it to your agent.

For the MCP path you install the same package as everyone else — it contains **`said-mcp`** (the memory
server your agent talks to) alongside `said` (the terminal command, which you won't need for MCP). You
don't run `said-mcp` by hand; your agent launches it for you once you connect it (next guide). Fully
offline, no cloud.

## Option A — one-line install (recommended)

**Linux / macOS** — paste into a terminal:

    curl -fsSL https://github.com/Qonsult1001/said-build/releases/latest/download/install.sh | sh

**Windows** — paste into PowerShell:

    irm https://github.com/Qonsult1001/said-build/releases/latest/download/install.ps1 | iex

This installs both `said-mcp` and `said`, adds them to your PATH, **and auto-connects the brain to any
AI agent it finds** — Claude Code, Claude Desktop, Cursor, and GitHub Copilot. It prints which agents it
wired up, e.g.:

    said: connected the brain to your agent(s): claude-code, cursor
      brain file: ~/.said/brain.said   (restart / reload each agent to pick it up)

**Restart / reload that agent** and it already has your memory — nothing to hand-configure. Then check
the install landed:

    said-mcp --version

You should see:

    said-mcp 0.11.7

> **Don't want it touching your agent config?** Set `SAID_NO_CONNECT=1` before running the installer and
> it installs the binaries only, then you connect manually (next guide). To use a specific brain file,
> set `SAID_BRAIN=/full/path/to/your-brain.said`.

If an agent was auto-connected, you can skip straight to the **[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)**
(start at "Tell the agent something to remember"). If not, or to wire a different agent by hand, see
**[Connect your brain to your agent](how-to-connect-said-to-your-agent.md)**.

## Option B — native installer (double-click)

Download and run the installer for your OS from the
[releases page](https://github.com/Qonsult1001/said-build/releases/latest):

- **Windows** → `said-setup-<version>-x64.exe` — run it; installs `said-mcp` + `said` and adds them to
  your PATH. Uninstall from *Add or Remove Programs*.
- **macOS** (Apple Silicon) → `said-<version>-arm64.pkg` — open it; installs into `/usr/local/bin`.
  (Unsigned for now: if macOS blocks it, right-click the `.pkg` → **Open**.)
- **Linux** (Debian/Ubuntu) → `said_<version>_amd64.deb` — `sudo apt install ./said_<version>_amd64.deb`.

Then verify with `said-mcp --version` (should print `said-mcp 0.11.7`) and continue to the connect guide.

## Option C — download the zip manually

From the [releases page](https://github.com/Qonsult1001/said-build/releases/latest), download the
**`brain`** zip for your machine:

- **Windows** → `said-brain-windows-x64.zip`
- **macOS** (Apple Silicon) → `said-brain-macos-arm64.zip`
- **Linux** (64-bit) → `said-brain-linux-x64.zip`

Unzip it. Inside you get `said-mcp` (+ `said`). Put them somewhere permanent and note the **full path to
`said-mcp`** — you'll paste that path into your agent's config in the next guide. (You don't need
`said-mcp` on your PATH for MCP; the agent runs it by its path.)

> macOS only — the first time a downloaded binary runs, macOS may block it. Clear it once with:
> `xattr -d com.apple.quarantine /path/to/said-mcp`

## Which one should I pick?

- Just want it working → **Option A** (the one-liner). It handles PATH and both binaries.
- Prefer a click-through installer → **Option B**.
- Want to control exactly where the files live → **Option C**.

## Next step

- **[Connect your brain to your agent](how-to-connect-said-to-your-agent.md)** — wire `said-mcp` into
  Claude Desktop, Claude Code, or Cursor. **This is the important one** — installing only puts the server
  on your machine; connecting is what gives your agent the memory.
- Then the **[MCP tutorial](tutorial-connect-your-brain-to-an-agent.md)** walks the whole thing end to
  end (create a brain → connect → remember → recall).
