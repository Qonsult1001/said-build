# said — MCP (AI agent) · v0.12.0

Connect your portable brain to an AI agent (Claude Desktop, Cursor, …) so the agent reads and writes it
for you — remembering facts automatically during your work and recalling them later, even in a fresh
chat. Runs as a local MCP server; no cloud, fully offline.

Ships with **`said-mcp.exe`** + docs — pair with the CLI bundle and a `brain.said` file for the full
free memory brain (terminal + agent, same file format).

## What's here

- **`said-mcp.exe`** — the brain MCP server binary (self-contained; the encoder is embedded). **The
  agent steering is built in** — on connect, the server sends the agent its own instructions ("recall
  with `ask` before answering, save memories with `remember`"), so there is nothing to paste or
  configure. Just point the agent at the server.
- **[`doc/`](doc/)** — all documentation:
  - **[`doc/how-to-install-said.md`](doc/how-to-install-said.md)** — install `said-mcp` on Windows,
    macOS, or Linux. **Do this first.**
  - **[`doc/how-to-connect-said-to-your-agent.md`](doc/how-to-connect-said-to-your-agent.md)** — wire
    `said-mcp` into Claude Desktop, Claude Code, or Cursor (per-agent config locations). **Do this
    second** — installing puts the server on your machine; connecting gives your agent the memory.
  - **[`doc/tutorial-connect-your-brain-to-an-agent.md`](doc/tutorial-connect-your-brain-to-an-agent.md)**
    — zero to first success end-to-end: create a brain, connect, have the agent remember a fact, then
    recall it in a new chat. The guided journey.
  - **How-to guides** — one real task each (all agent-driven — you talk to the agent, it calls the tools):
    - [`doc/how-to-store-and-recall-notes-with-an-agent.md`](doc/how-to-store-and-recall-notes-with-an-agent.md) — the everyday save-and-ask flow
    - [`doc/how-to-build-a-brain-by-talking-to-your-agent.md`](doc/how-to-build-a-brain-by-talking-to-your-agent.md) — fill a brain fast: paste a pile of info, the agent saves it one memory at a time
    - [`doc/how-to-import-your-own-data-with-an-agent.md`](doc/how-to-import-your-own-data-with-an-agent.md) — import your browser history or email as memories, then ask "what was the last website I visited?"
    - [`doc/how-to-organize-and-find-memories-with-an-agent.md`](doc/how-to-organize-and-find-memories-with-an-agent.md) — find one note, get everything on a topic, link memories with concepts, browse by tags
    - [`doc/how-to-track-versions-with-an-agent.md`](doc/how-to-track-versions-with-an-agent.md) — update a fact, see its history, roll it back
    - [`doc/how-to-clean-up-your-brain-with-an-agent.md`](doc/how-to-clean-up-your-brain-with-an-agent.md) — delete, recover from the recycle bin, set retention
    - [`doc/how-to-move-your-brain-to-another-agent.md`](doc/how-to-move-your-brain-to-another-agent.md) — one file, any agent/machine; open a shared brain or switch to another existing file
    - [`doc/how-to-memory-vs-coding-brain.md`](doc/how-to-memory-vs-coding-brain.md) — pasted code vs real code search; when you need the coding build
  - **[`doc/verify-mcp.md`](doc/verify-mcp.md)** — how to drive the MCP server end-to-end and confirm each
    tool works (for review / acceptance).

## The tool surface (Personal / Free tier)

The brain MCP server exposes **memory-only** tools — this is the Personal tier, so the code/enterprise
tools are not present:

`ask` · `remember` · `get` · `list_concepts` · `list_tags` · `status` · `history` · `checkout` ·
`delete` · `compact` · `open` · `create` · `admin` · `import`

`import` pulls the user's **own** personal data in as memories — `source=browser` (Chromium history,
offline and read-only) or `source=email` (a local `.mbox` / `.emlx` file). No login, no cloud. See
[`doc/how-to-import-your-own-data-with-an-agent.md`](doc/how-to-import-your-own-data-with-an-agent.md).

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
handful) and returns them to the agent. Each result shows its **tags** so you can see which facet each
memory belongs to. When several results genuinely tie (close scores, no clear winner), `ask` appends a
**close matches** note with tag counts and scoping hints — the agent is steered to re-ask with a tag
filter instead of guessing.

The **agent then reads those and answers you** (or picks the right one) — the memory system surfaces the
top-K, the LLM does the final reasoning. That's why the right memory being *in the returned set* is what
matters, and it essentially always is. When results are noisy at scale, see
[`doc/how-to-organize-and-find-memories-with-an-agent.md`](doc/how-to-organize-and-find-memories-with-an-agent.md).
