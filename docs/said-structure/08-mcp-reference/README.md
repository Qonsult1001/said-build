# MCP reference

Every MCP tool shipped by [`crates/said-mcp`](../../../crates/said-mcp/). The MCP server is a stdio-mode JSON-RPC process that exposes `.said` brain operations as tools callable by any MCP client (Cursor, Claude Desktop, custom agents).

## Launch

```
said-mcp
```

By default attaches to a sibling `.said` file (`./brain.said`, auto-promoting to the most-populated brain in the directory). Override via `--said <path>` or the `open` tool once running.

## Tool list (25 tools default; 31 with `--features forge`, as of 2026-04-23)

### Core retrieval
- [ask](ask.md) — 3-engine smart fusion (Sym + Grep + SCA) + auto-dream
- [search](search.md) — pure SCA semantic search (deprecated in favor of ask)
- [sym](sym.md) — exact symbol lookup
- [get](get.md) — read full content by doc_id

### Writing
- [remember](remember.md) — store text with pillar hints + salience + surprise detection
- [ingest](ingest.md) — single file / folder with optional `--pointer` equivalent
- [init](init.md) — bulk-ingest a directory (tree-sitter AST chunking)

### File lifecycle
- [create](create.md) — create empty `.said` with chosen mode
- [open](open.md) — switch the MCP server to a different `.said` file
- [delete](other-tools.md#delete) — soft-delete a frame
- [checkout](other-tools.md#checkout) — restore a past version
- [history](other-tools.md#history) — lineage trail

### Admin / compliance
- [admin](admin.md) — single tool with `action` discriminator covering 7 subcommands

### Pillar + memory model
- [session_end](other-tools.md#session_end) — flush a session summary as Episodic
- [tool_completion](other-tools.md#tool_completion) — per-tool-call event
- [salience](other-tools.md#salience) — standalone salience scoring
- [dream](other-tools.md#dream) — kept for back-compat; now no-op per Decision 5 v2

### Diagnostics
- [status](status.md) — brain state + mode + frame counts
- [overview](other-tools.md#overview) — detected module list
- [discover](other-tools.md#discover) — detect modules in a monolithic codebase

### Maintenance
- [sync](other-tools.md#sync) — detect + tombstone orphaned source files
- [clean](other-tools.md#clean) — remove dangling state
- [journal](other-tools.md#journal) — timestamped journal entry
- [snapshot](other-tools.md#snapshot) — extract a module's frames into a sub-brain
- [sandbox](other-tools.md#sandbox) — SQL module sandbox workflow

### Forge — spec-driven workspace generator (feature-gated, +6 tools)
Available when `said-mcp` is built with `--features forge`. See [forge-tools.md](forge-tools.md) for the full reference.
- `forge_list` — list stories extracted from the current directive
- `forge_get` — bundled story + plan + tasks + brain markdown (25k-token cap)
- `forge_status` — per-story run state
- `forge_load` — load a new directive (requires `confirm:true`)
- `forge_run` — generate stories — **MVP: deferred to CLI** (MutexGuard !Send over `.await`)
- `forge_reset` — tombstone frames + remove projection

## Protocol

Standard MCP stdio JSON-RPC. Messages:

- `initialize` / `notifications/initialized` (handshake)
- `tools/list` (enumerate tools)
- `tools/call` (invoke a tool)

Example:
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ask","arguments":{"query":"how does compact work"}}}
```

## Tool_box macro

All tools are declared in [`crates/said-mcp/src/tools.rs`](../../../crates/said-mcp/src/tools.rs) via the `#[mcp_tool(name="...", description="...")]` macro. The `tool_box!` macro at the bottom generates the dispatch enum:

```rust
tool_box!(SaidTools, [
    SearchTool, AskTool, GetTool, IngestTool, OpenTool, CreateTool, InitTool,
    SyncTool, RememberTool, JournalTool, StatusTool, SymTool, HistoryTool,
    CheckoutTool, DeleteTool, DiscoverTool, OverviewTool, SnapshotTool,
    SandboxTool, CleanTool, SessionEndTool, ToolCompletionTool, SalienceTool,
    DreamTool, AdminTool
]);
```

Dispatch lives in [`handler.rs`](../../../crates/said-mcp/src/handler.rs) via a `match` on the enum variant.

## Tool-description contract

Every tool's `description` field is read by MCP clients to build argument forms and documentation UIs. Our descriptions state:

1. What the tool does (1-2 sentences)
2. When to use it
3. Key arguments with defaults and constraints
4. For multi-action tools (like `admin`), the full action list

LLM-driven clients (Cursor, Claude Desktop) consume these descriptions as prompt context, so they're kept honest and explicit.

## Enterprise vs portable

The MCP server honors `BrainMode` from the attached brain:

- Portable — all tools available
- Enterprise — `ingest` without `pointer=true` is refused (same as CLI), `remember` always allowed (text only, not bulk ingest)

## Auto-dream auto-fires on ask + search

Every `ask` / `search` call checks the brain's pending-query count. When it crosses `dynamic_dream_threshold(active_frames)`, a brain-state dream fires silently and the `.said` file is updated via `save_brain_only()` (BRAN-only partial save). See [Row 35](../05-features/row-35-brain-state-dream.md).

## See also

- [CLI reference](../07-cli-reference/README.md) — same surface via terminal
- [Row 44 MCP admin tool](../05-features/row-44-mcp-admin.md)
- [Row 50 Plugin ecosystem trait](../05-features/row-50-plugin-trait.md) — how third-party packs hook in
