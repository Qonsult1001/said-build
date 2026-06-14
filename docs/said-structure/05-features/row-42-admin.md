# Row 42 — Admin CLI

**Status:** ✅ shipped 2026-04-22. Step 10 in the original 12-step plan.

## What it does

`said admin <action>` exposes the tombstone lineage + compliance surface:

| Subcommand | Purpose |
|---|---|
| `list-tombstones [--like <substr>]` | Recycle Bin — every non-active frame newest-first |
| `restore <doc_id>` | Flip newest tombstone for `doc_id` back to Active; demote previous head |
| `who-deleted <doc_id>` | Full lineage trail with `superseded_by` + user/session attribution tags |
| `legal-hold-add <doc_id> <case>` | Tag `legal_hold:<case>` blocks retention sweeps |
| `legal-hold-release <doc_id> <case>` | Strip the hold |
| `retention-sweep [--older-than-days N] [--keep-per-doc N]` | Reap aged tombstones; honor holds |
| `audit [--verify] [--actor <a>] [--kind <k>]` | Show or verify the AUDT chain |

## Where it lives

- [`crates/sca-core/src/frames.rs`](../../../crates/sca-core/src/frames.rs) — `admin_tombstone_records`, `admin_restore_tombstoned`, `admin_add_legal_hold`, `admin_release_legal_hold`, `mark_frame_deleted`, `is_under_legal_hold`, `drop_tombstones` (now legal-hold-aware)
- [`crates/sca-core/src/said_file.rs`](../../../crates/sca-core/src/said_file.rs) — thin `SaidFile::admin_*` wrappers + audit hooks
- [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) — `AdminAction` enum + `cmd_admin`

## Inputs

Subcommand + flags (see list above). CLI-wide `--json` emits JSON; default is human-readable.

## Outputs

All mutating actions:
1. Modify frames / tags
2. Append an audit entry via `brain.audit_mut().append(...)`
3. Call `brain.save()` to persist atomically

Stdout:
- `list-tombstones` — table or JSON array
- `restore` — `✓ Restored doc_id '...' as frame #N. Previous active head (frame #M) demoted.`
- `who-deleted` — lineage walk with per-frame status + timestamp + attribution
- `legal-hold-add` / `release` — `✓ Placed/Released legal hold 'CASE' on N frame(s).`
- `retention-sweep` — `✓ Dropped N tombstones older than D days (kept K per doc). Legal holds honored.`
- `audit` — entry list or `✓ Audit chain intact (N entries, BLAKE3-verified).`

## How to test

End-to-end (verified 2026-04-22 on a fresh brain with a multi-version doc_id):

```
$ said create tmp_admin.said --mode portable
Created: tmp_admin.said (mode: portable, immutable)

$ said --path tmp_admin.said add "v1" --id memo
$ said --path tmp_admin.said add "v2" --id memo

$ said --path tmp_admin.said admin list-tombstones
Tombstoned frames (1):
  memo (frame #0, 3 bytes, created_at=...) superseded_by=#1

$ said --path tmp_admin.said admin restore memo
✓ Restored doc_id 'memo' as frame #0.
  Previous active head (frame #1) demoted to tombstone.

$ said --path tmp_admin.said admin legal-hold-add memo CASE-42
✓ Placed legal hold 'CASE-42' on 2 frame(s) for doc_id 'memo'.

$ said --path tmp_admin.said admin audit --verify
✓ Audit chain intact (5 entries, BLAKE3-verified).
```

## How to extend

New admin subcommand:
1. Add variant to `AdminAction` enum in [said-cli/src/main.rs](../../../crates/said-cli/src/main.rs)
2. Add match arm in `cmd_admin` that calls into `SaidFile` / `FrameStore`
3. Add audit hook (`brain.audit_mut().append(kind, target, detail)`) if mutating
4. Mirror in MCP `handle_admin` — see [Row 44](row-44-mcp-admin.md)
5. Document in this page

## Known limitations

- `retention-sweep`'s age filter uses wall-clock seconds, not calendar days per UTC boundary. See [Known limitations](../11-known-limitations.md).
- Attribution tags (`user_id:`, `session:`, `deleted_by:`) are honored by `who-deleted` but must be set at write time by the caller — no automatic extraction.

## See also

- [Row 44 MCP admin tool](row-44-mcp-admin.md)
- [Row 45 Audit section](row-45-audit.md)
- [3.4 FrameStore](../03-core-subsystems/3.4-framestore.md)
