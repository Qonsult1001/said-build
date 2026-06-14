# MCP tool: admin

Single MCP tool with an `action` discriminator. Mirrors CLI [`said admin`](../07-cli-reference/admin.md).

## Schema

```json
{
  "name": "admin",
  "arguments": {
    "action": "list-tombstones | restore | who-deleted | legal-hold-add | legal-hold-release | retention-sweep | audit",
    "doc_id": "string, optional (required by restore / who-deleted / legal-hold-*)",
    "like": "string, optional (filter for list-tombstones)",
    "case": "string, optional (required by legal-hold-* and reused as kind-filter for audit)",
    "older_than_days": "integer, optional, default 365 (retention-sweep)",
    "keep_per_doc": "integer, optional, default 1 (retention-sweep)"
  }
}
```

## Description

> Administrative operations on the attached brain — the enterprise Recycle Bin, lineage audit, restore, legal-hold, and retention sweeps. Mirrors `said admin <action>` in the CLI so web/desktop UIs (Tauri Brain Explorer, admin dashboards) get parity with terminal users.

## Actions

| action | Required args | Does |
|---|---|---|
| `list-tombstones` | (optional `like`) | Returns every non-Active frame newest-first |
| `restore` | `doc_id` | Flip newest tombstone → Active; demote old head |
| `who-deleted` | `doc_id` | Full lineage trail |
| `legal-hold-add` | `doc_id`, `case` | Tag with `legal_hold:<case>` |
| `legal-hold-release` | `doc_id`, `case` | Strip `legal_hold:<case>` |
| `retention-sweep` | (optional age / keep knobs) | Mark aged tombstones Deleted; honor holds |
| `audit` | (optional `doc_id` = actor filter, `case` = kind filter) | Show / verify audit chain |

## Example: list tombstones

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
  "name":"admin",
  "arguments":{"action":"list-tombstones","like":"memo"}
}}
```

Response:
```
Tombstoned frames (3):
  memo_v1 (frame #0, 128 bytes, created_at=1711000000) superseded_by=#1
  memo_v2 (frame #1, 156 bytes, created_at=1711100000) superseded_by=#2
  memo_old (frame #5, 64 bytes, created_at=1710500000) [legal_hold:CASE-42]
```

## Example: restore

```json
{"method":"tools/call","params":{"name":"admin","arguments":{
  "action":"restore","doc_id":"memo"
}}}
```

Response:
```
✓ Restored doc_id 'memo' as frame #0.
  Previous active head (frame #1) demoted to tombstone.
```

## Example: audit with filter

```json
{"method":"tools/call","params":{"name":"admin","arguments":{
  "action":"audit",
  "doc_id":"alice",         // filter actor contains "alice"
  "case":"remember"         // filter kind == "remember"
}}}
```

Response:
```
Audit log (7 of 142 entries, chain verified):
  #12 [1711200000] remember actor=alice target=doc_42 frame #42 pillar=Semantic
  ...
```

The doc_id / case field re-use for audit is documented in the tool description so agents can read it.

## Error paths

- Missing required arg → clear error ("restore requires `doc_id`")
- Unknown action → list of valid actions
- Audit chain broken on verify → error with break seq

Every mutating action `brain.save()`s on success. Failed saves propagate as errors.

## Auto-log

Admin actions auto-append audit entries:

| Action | Audit kind |
|---|---|
| `restore` | `restore` |
| `legal-hold-add` | `legal_hold_add` |
| `legal-hold-release` | `legal_hold_release` |
| `retention-sweep` | `retention_sweep` (per frame marked deleted) |

List / who-deleted / audit actions don't mutate so don't log.

## Known limitation

MCP dispatch doesn't yet set audit `actor` from session context — all MCP-originated admin actions log as `actor=owner`. See [Row 45](../05-features/row-45-audit.md) for the gap + fix.

## See also

- [CLI said admin](../07-cli-reference/admin.md)
- [Row 42 Admin CLI](../05-features/row-42-admin.md)
- [Row 44 MCP admin tool feature](../05-features/row-44-mcp-admin.md)
- [Row 45 Audit section](../05-features/row-45-audit.md)
