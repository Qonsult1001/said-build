# Row 43 — User-management UI for admin operations

**Status:** ⏳ planned (UI sprint, post-step-15). Backend ready.

## What the roadmap entry says

> The admin CLI / MCP is the BACKEND; the Brain Explorer UI (desktop / web) needs views that expose it.

## Planned views

### (a) Tombstone browser
Filter + restore + last-modified column. Calls MCP `admin action=list-tombstones` and `action=restore`.

### (b) Deletion-trail viewer
Version walk + side-by-side diff. For any `doc_id`, show the full lineage with timestamps, attribution, and content-hash differences between consecutive versions.

### (c) Legal-hold dashboard
Active holds + case IDs + frames held. Shows which doc_ids are protected, by which cases, placed by whom, when.

### (d) Retention-policy editor
Preview before apply: "This sweep would drop N frames; M are held." Lets admins test policy combinations without risk.

### (e) Per-user audit log
Once AUDT (Row 45) is wired with app_id attribution, this view surfaces the chain filtered by actor / kind / date range.

## Backend ready

Everything the UI needs already ships:

- [Row 42 CLI admin](row-42-admin.md) — every action has a CLI subcommand
- [Row 44 MCP admin](row-44-mcp-admin.md) — every action has an MCP tool action
- [Row 45 Audit](row-45-audit.md) — append-only log with BLAKE3 verification

## Why UI

Terminal + MCP suffice for developer ops. Enterprise customers (hospitals, law firms, government) need a point-and-click surface that non-developer admins use. GDPR data-subject-request flows need click-to-restore + click-to-export-audit rather than CLI invocation.

## Planned stack

- Cross-platform: **Tauri** (Rust backend, web-tech frontend)
- Talks to `.said` via **MCP tools only** — no new protocol
- UI code lives in a separate crate / sibling repo so the core stays CLI/MCP-first

## Enterprise-only

Licensing ties to `BrainMode::Enterprise` flag (immutable, set at create). Portable brains don't get the UI by license.

## Not done

No UI code exists yet. The backend API surface is stable and complete for what the UI will consume — every Row 42 / Row 44 / Row 45 action is deterministic, testable, and has a JSON output shape.

## See also

- [Row 42 CLI admin](row-42-admin.md)
- [Row 44 MCP admin](row-44-mcp-admin.md)
- [Row 45 Audit](row-45-audit.md)
- [Roadmap](../12-roadmap.md)
