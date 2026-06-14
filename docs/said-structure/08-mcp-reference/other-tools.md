# Other MCP tools

Reference for every remaining MCP tool not covered by individual pages.

## delete

Soft-delete a frame by doc_id.

```json
{"name": "delete", "arguments": {"doc_id": "string, required"}}
```

Calls `brain.forget(doc_id)` → flips status to `Deleted`. Still recoverable via `admin restore` until the next compact.

Audit-logged with kind `delete`.

## checkout

Restore a past version as the new HEAD.

```json
{"name": "checkout", "arguments": {
  "name": "string, required — symbol or doc_id",
  "version": "integer, optional — v0/v1/... index from history",
  "frame": "integer, optional — explicit frame_id"
}}
```

Same semantics as CLI [said checkout](../07-cli-reference/other-commands.md#said-checkout).

## history

Lineage trail for a symbol or doc_id.

```json
{"name": "history", "arguments": {"name": "string, required"}}
```

Returns the version-indexed list:
```
v0 [genesis]       frame #0
v1 [delta=0.12]    frame #1
v2 [current HEAD]  frame #2
```

## session_end

Flush a session summary as one Episodic frame.

```json
{"name": "session_end", "arguments": {"summary": "string, required"}}
```

Tags: `pillar:episodic`, `session_end:true`.

## tool_completion

Per-tool-call event.

```json
{"name": "tool_completion", "arguments": {
  "tool": "string (tool name)",
  "args": "string (serialized args)",
  "result": "string (tool output)"
}}
```

Tags: `pillar:episodic`, `tool_completion:true`, `tool:<name>`.

## salience

Standalone salience scoring without writing a frame.

```json
{"name": "salience", "arguments": {
  "text": "string, required",
  "pillar": "string, optional"
}}
```

Returns the Salience `{score, band, tags}` as text. Useful for agents that want to preview a score before deciding whether to `remember`.

## dream

Back-compat stub. Per Decision 5 v2 (Row 35), `run_dream_content` is a no-op — content consolidation moved to caller-side LLM at read time.

```json
{"name": "dream", "arguments": {}}
```

Returns a zero-count DreamReport. Kept for back-compat; may be removed in a future version.

## discover

Detect modules in a monolithic codebase.

```json
{"name": "discover", "arguments": {}}
```

Runs the module-detection heuristic (naming-convention prefixes, table co-occurrence, FK graph walks). Output is a list of detected modules with member counts.

Mainly useful for SQL schemas; less useful on typical code repos.

## overview

List detected modules (post-discover).

```json
{"name": "overview", "arguments": {"check": "string, optional — specific module to check"}}
```

## snapshot

Extract a module from a monolithic codebase into its own folder + a lens ("glass view") onto the parent brain. Full semantics: [CLI snapshot](../07-cli-reference/snapshot.md).

```json
{"name": "snapshot", "arguments": {
  "module": "string, required",
  "output": "string, optional — output directory"
}}
```

Output includes `Exclusive/`, `Shared/`, `BOUNDARY.md`, `MODULE_MAP.md`, and a lens `.said` file that reads from the parent brain at query time (no frame copies). Parent stays read-only; re-run to pick up new parent frames.

## sandbox

Deploy a snapshotted module against a real SQL Server 2022 container for cross-module interaction testing. Full semantics: [CLI sandbox](../07-cli-reference/sandbox.md).

```json
{"name": "sandbox", "arguments": {
  "module": "string, required — primary module",
  "modules": "array of strings, optional — additional co-deployed modules",
  "port": "integer, optional — default 1433",
  "label": "string, optional — distinguishes A/B runs",
  "up": "boolean, optional — default true (brings container up after generation)"
}}
```

Writes `sandbox/{docker-compose.yml, schema.sql, seed-data.sql, run.sh}` under the snapshot workspace. Schema is dependency-sorted: functions → tables (FK-topo) → views → procs (module-scoped) → triggers. Multi-module mode deploys all listed modules into one container for realistic trigger cascade testing.

## sync

Detect files missing from disk vs `source:<path>` tags; offer to tombstone orphaned frames.

```json
{"name": "sync", "arguments": {"dry_run": "boolean, optional, default false"}}
```

`dry_run=true` reports what would be tombstoned without actually doing it.

## clean

Remove dangling state (orphaned pending frames, partial-compact leftovers).

```json
{"name": "clean", "arguments": {}}
```

Rarely needed; compact handles most of this automatically.

## journal

Append a timestamped journal entry as an Episodic frame.

```json
{"name": "journal", "arguments": {"entry": "string, required"}}
```

Convenience wrapper — same as `remember` with pillar=episodic + title including a timestamp.

## See also

- [ask](ask.md) / [search](search.md) / [get](get.md) / [sym](sym.md) — retrieval
- [remember](remember.md) / [ingest](ingest.md) / [init](init.md) — writing
- [admin](admin.md) / [create](create.md) / [open](open.md) / [status](status.md) — detailed pages
- [CLI reference](../07-cli-reference/README.md) — same operations via terminal
