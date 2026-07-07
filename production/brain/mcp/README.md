# said — MCP (AI agent)

Connect your portable brain to an AI agent (Claude Desktop, Cursor, …) so the agent reads and writes it
for you — remembering facts automatically during your work and recalling them later, even in a fresh
chat. Runs as a local MCP server; no cloud, fully offline.

## What's here

- **`said-mcp.exe`** — the brain MCP server binary (self-contained; the encoder is embedded). **The
  agent steering is built in** — on connect, the server sends the agent its own instructions ("recall
  with `ask` before answering, save memories with `remember`"), so there is nothing to paste or
  configure. Just point the agent at the server.
- **[`doc/`](doc/)** — all documentation:
  - **[`doc/how-to-install-said.md`](doc/how-to-install-said.md)** — download and set up `said` on
    Windows, macOS, or Linux. **Do this first.**
  - **[`doc/tutorial-connect-your-brain-to-an-agent.md`](doc/tutorial-connect-your-brain-to-an-agent.md)**
    — zero to first success: point your agent at the MCP server, have it remember a fact, then recall it
    in a new chat. Start here once installed.
  - **How-to guides** — one real task each:
    - [`doc/how-to-store-and-recall-notes-with-an-agent.md`](doc/how-to-store-and-recall-notes-with-an-agent.md) — the everyday flow where the agent saves and recalls for you
    - [`doc/how-to-move-your-brain-to-another-agent.md`](doc/how-to-move-your-brain-to-another-agent.md) — one file, any MCP agent or machine
  - **[`doc/verify-mcp.md`](doc/verify-mcp.md)** — how to drive the MCP server end-to-end and confirm each
    tool works (for review / acceptance).

## The tool surface (Personal / Free tier)

The brain MCP server exposes **memory-only** tools — this is the Personal tier, so the code/enterprise
tools are not present:

`ask` · `remember` · `get` · `list_concepts` · `status` · `history` · `checkout` · `delete` · `open` ·
`create` · `admin`

## Connect it (example: an MCP client config)

```json
{
  "mcpServers": {
    "said-brain": {
      "command": "C:/path/to/said-mcp.exe",
      "args": ["--path", "C:/path/to/my-brain.said"]
    }
  }
}
```

That's it — on connect the server hands the agent its steering automatically (recall before answering,
save at session end); there's nothing to paste. Full walkthrough in
[`doc/tutorial-connect-your-brain-to-an-agent.md`](doc/tutorial-connect-your-brain-to-an-agent.md).

## How recall works through an agent

`.said` does the hard work — it narrows thousands of memories down to the **most relevant few** (the top
handful) and returns them to the agent. The **agent then reads those and answers you** (or picks the
right one) — the memory system surfaces the top-K, the LLM does the final reasoning. That's why the
right memory being *in the returned set* is what matters, and it essentially always is.
