# Row 32 — Episodic writer + tool hooks

**Status:** ✅ shipped 2026-04-21 (Decision 3)

## What it does

Three entry points populate Episodic frames:

1. **Explicit `remember`** — user/agent calls MCP `remember` tool or the Rust API
2. **`session_end` hook** — agent flushes a session summary at end of conversation
3. **`tool_completion` hook** — per-tool-call event (agent ran tool X, got result Y)

All three route through `SaidFile::remember_with_pillar(..., Pillar::Episodic, ...)`.

## Where it lives

- [`SaidFile::remember_with_pillar`](../../../crates/sca-core/src/said_file.rs)
- MCP tools: `RememberTool`, `SessionEndTool`, `ToolCompletionTool` in [`said-mcp/src/tools.rs`](../../../crates/said-mcp/src/tools.rs)
- MCP handlers: `handle_remember`, `handle_session_end`, `handle_tool_completion` in [`said-mcp/src/handler.rs`](../../../crates/said-mcp/src/handler.rs)

## Inputs

### `remember`
```json
{"name": "remember", "arguments": {
  "content": "Alice asked about the launch timeline",
  "id": "turn_142",        // optional
  "title": "session-2026-04", // optional
  "pillar": "episodic",    // optional, defaults to episodic
  "tags": ["user:alice"]   // optional
}}
```

### `session_end`
```json
{"name": "session_end", "arguments": {
  "summary": "User confirmed Friday launch; action items: deploy staging Wed."
}}
```

### `tool_completion`
```json
{"name": "tool_completion", "arguments": {
  "tool": "bash",
  "args": "ls -la",
  "result": "total 42..."
}}
```

## Outputs

One new Episodic frame with tags:
- `pillar:episodic`
- Caller-supplied tags
- `session_end:true` (session_end path)
- `tool_completion:true` + `tool:<name>` (tool_completion path)

MCP response includes the new frame id, pillar label, and total frame count; when `remember_with_salience` triggers (default for `remember`), surprise detection also surfaces:

```
✓ Saved to brain (frame #142, pillar=episodic). salience=35 (medium)
Brain now has 1847 total frames...
```

If the content contradicts a prior frame:
```
✓ Saved to brain (frame #142, pillar=episodic). salience=35 (medium) · ⚠ contradicts prior frame `fact_veg`
```

## How to test

See [`examples/surprise_probe.rs`](../../../crates/sca-core/examples/surprise_probe.rs). End-to-end: create brain, write a frame via MCP remember, verify `brain.frames.get_meta(doc_id)` returns a frame with `pillar:episodic` tag.

## How to extend

To add a new automatic Episodic writer (e.g. `/commit-end` hook):
1. Add a new `*Tool` struct in `tools.rs`
2. Register in the `tool_box!` macro
3. Add a handler in `handler.rs` that calls `remember_with_pillar(Pillar::Episodic, ...)` with a conventionally-tagged body
4. Document in [four-pillars/episodic](../04-four-pillars/episodic.md)

## Known limitations

- The salience scorer (Row 33) is deterministic heuristics; a trained v1 is planned (docs/SAID_MVP_PLAN.md step 4).

## See also

- [Row 33 Salience scorer](row-33-salience.md)
- [Row 41 Surprise detector](row-41-surprise.md)
- [Episodic pillar](../04-four-pillars/episodic.md)
