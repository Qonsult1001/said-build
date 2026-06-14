# Row 44 — MCP admin tool

**Status:** ✅ shipped 2026-04-22. Full parity with CLI Row 42.

## What it does

Single MCP tool (`admin`) with an `action` discriminator exposing every subcommand from CLI `said admin`:

- `list-tombstones`
- `restore`
- `who-deleted`
- `legal-hold-add`
- `legal-hold-release`
- `retention-sweep`
- `audit`

Registered as tool #24 in the `tool_box!` macro.

## Where it lives

- [`crates/said-mcp/src/tools.rs`](../../../crates/said-mcp/src/tools.rs) — `AdminTool` struct
- [`crates/said-mcp/src/handler.rs`](../../../crates/said-mcp/src/handler.rs) — `handle_admin` matches on `t.action`

## Inputs

```json
{
  "action": "list-tombstones",       // required; one of the 7 above
  "doc_id": "...",                   // required by restore, who-deleted, legal-hold-*
  "like": "substring",               // optional filter for list-tombstones
  "case": "CASE-42",                 // required by legal-hold-add / legal-hold-release
  "older_than_days": 365,            // retention-sweep, default 365
  "keep_per_doc": 1                  // retention-sweep, default 1
}
```

For the `audit` action, fields are repurposed:
- `doc_id` → actor filter substring
- `case` → kind filter exact match

This repurposing is documented in the tool description so agents can read it.

## Outputs

`CallToolResult::text_content` with a human-readable block describing the result. For `list-tombstones` on a populated brain:

```
Tombstoned frames (305):
  JHW065_Confirmatory Affidavit_...docx::para_0062 (frame #14875, 8 bytes, created_at=...) superseded_by=#16871
  ...
```

For `audit` with verification:
```
Audit log (5 of 5 entries, chain verified):
  #0 [timestamp] legal_hold_add actor=owner target=doc_42 case=CASE-A frames_tagged=2
  #1 [timestamp] restore         actor=owner target=doc_42 restored frame #3
  ...
```

Up to 200 entries + overflow count for `audit`.

## Error paths

- Missing required arg → `CallToolError` with specific reason ("restore requires `doc_id`")
- Unknown action → list of valid actions
- Audit chain broken → `CallToolError` with break point

Every mutating action calls `brain.save()` on success.

## How to test

Launch MCP, send sequence:

```
{"method":"initialize","params":{"protocolVersion":"2024-11-05",...}}
{"method":"notifications/initialized"}
{"method":"tools/call","params":{"name":"open","arguments":{"path":"test.said"}}}
{"method":"tools/call","params":{"name":"admin","arguments":{"action":"list-tombstones"}}}
{"method":"tools/call","params":{"name":"admin","arguments":{"action":"restore","doc_id":"..."}}}
```

Verified via stdio integration on `willie.said` (305 tombstones listed correctly). Tool registration confirmed by `tools/list` showing `"name":"admin"`.

## How to extend

New admin action:
1. Add action name to the `match` in `handle_admin`
2. Reuse existing fields (`doc_id`, `case`, …) or add new fields to `AdminTool` struct
3. Update the tool description string — MCP clients read it to build argument forms
4. Mirror in CLI — see [Row 42](row-42-admin.md)

## Known limitations

- MCP dispatch doesn't yet extract app-id from request context to set audit actor. Actions log as `actor=owner`. See [Row 45](row-45-audit.md) for the gap.
- AppGrant registry isn't constructed / consulted by MCP dispatch. See [Row 45](row-45-audit.md).

## See also

- [Row 42 CLI admin](row-42-admin.md)
- [Row 45 Audit section](row-45-audit.md)
- [8 MCP reference](../08-mcp-reference/README.md)
