# said admin

Enterprise Recycle Bin + compliance surface. Seven subcommands grouped under `said admin`.

## Global usage

```
said [--path FILE] [--json] admin <ACTION> [args]
```

All admin actions:
- Audit-log the action (every mutation gets a BLAKE3-chained entry)
- `brain.save()` on success
- Honor legal holds (tags starting with `legal_hold:`)

## list-tombstones

Recycle Bin view. Every non-Active frame newest-first.

```
said admin list-tombstones [--like <substring>]
```

Output:
```
Tombstoned frames (305):
  doc_42 (frame #875, 128 bytes, created_at=1711200000) superseded_by=#876 [legal_hold:CASE-42]
  doc_17 (frame #870, 512 bytes, created_at=1711180000) superseded_by=#871
  ...
```

`--like <substr>` filters to doc_ids containing the substring (case-insensitive).

## restore

Flip the newest tombstone for `doc_id` back to Active; demote the current Active head.

```
said admin restore <DOC_ID>
```

Output:
```
✓ Restored doc_id 'memo' as frame #0.
  Previous active head (frame #1) demoted to tombstone.
```

Error paths:
- `no tombstone found for doc_id 'X'` — no prior version in the brain
- Legal holds do NOT block restore (restore makes historical content live again; holds block deletion, not re-activation)

## who-deleted

Full lineage trail with `superseded_by` + attribution tags.

```
said admin who-deleted <DOC_ID>
```

Output:
```
Deletion / lineage trail for 'memo':
  #2 [active] created_at=1711400000  [user_id:alice session:s2]
  #1 [tombstoned] created_at=1711200000 → superseded by #2  [user_id:bob]
  #0 [tombstoned] created_at=1711000000 → superseded by #1  [user_id:alice]
```

Attribution tags (`user_id:`, `session:`, `deleted_by:`, `actor:`) are surfaced automatically when the caller set them at write time. If no one set them, the trail shows just timestamps.

## legal-hold-add / legal-hold-release

Block retention sweeps from reaping a `doc_id`.

```
said admin legal-hold-add <DOC_ID> <CASE>
said admin legal-hold-release <DOC_ID> <CASE>
```

Tag `legal_hold:<case>` is applied to every frame (Active + Tombstone + Deleted) with that doc_id. Multiple holds can stack:

```bash
said admin legal-hold-add doc_42 CASE-A
said admin legal-hold-add doc_42 CASE-B
# Both tags present; either one blocks retention sweep
said admin legal-hold-release doc_42 CASE-A
# CASE-B still holds; frame stays safe
said admin legal-hold-release doc_42 CASE-B
# All holds released; next retention-sweep can reap
```

Output:
```
✓ Placed legal hold 'CASE-42' on 3 frame(s) for doc_id 'doc_42'.
✓ Released legal hold 'CASE-42' from 3 frame(s) for doc_id 'doc_42'.
```

## retention-sweep

Drop aged tombstones; keep most recent N per doc_id; honor holds.

```
said admin retention-sweep [--older-than-days N] [--keep-per-doc N]
```

- `--older-than-days` default 365
- `--keep-per-doc` default 1

Algorithm:
1. Collect every Tombstone frame
2. Filter: NOT `legal_hold:*` AND `created_at < now - older_than_days × 86400`
3. Group by doc_id, sort newest-first, skip the first `keep_per_doc`
4. Mark remainder as Deleted (still recoverable until next `compact`)

Output:
```
✓ Retention sweep: dropped 47 tombstones older than 365 days (kept 1 per doc_id).
  Legal holds honored — no held frame was touched.
  Run `said compact` to physically reclaim the freed bytes.
```

## audit

View or verify the append-only BLAKE3-chained audit log.

```
said admin audit [--verify] [--actor <a>] [--kind <k>]
```

`--verify` — just walk the chain and print `ok` or the first break point. Exit non-zero on failure.

`--actor <a>` — filter to entries whose actor contains this substring.

`--kind <k>` — filter to entries with exactly this kind (`remember` / `delete` / `restore` / `legal_hold_add` / …).

Default (no flags):

```
Audit log (142 of 142 entries):
  #0      [1711000000] remember        actor=owner        target=doc_42   frame #0 pillar=Semantic
  #1      [1711100000] remember        actor=owner        target=doc_17   frame #1 pillar=Episodic
  #2      [1711200000] legal_hold_add  actor=alice        target=doc_42   case=CASE-A frames_tagged=1
  ...
```

`--verify`:
```
✓ Audit chain intact (142 entries, BLAKE3-verified).
```

## Matching MCP tool

[MCP `admin` tool](../08-mcp-reference/admin.md) — single tool with `action` discriminator. Full parity.

## See also

- [Row 42 Admin CLI feature](../05-features/row-42-admin.md)
- [Row 45 Audit section](../05-features/row-45-audit.md)
- [3.7 Audit log](../03-core-subsystems/3.7-audit-log.md)
