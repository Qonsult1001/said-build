# Row 45 — Audit section (AUDT) + AppGrant

**Status:** ✅ shipped 2026-04-22. Step 11 in the original 12-step plan.

## What it does

Append-only BLAKE3-chained log of every mutating operation. Entries carry `{seq, timestamp, actor, kind, target, detail}`; each entry's hash depends on the previous entry's hash so tampering breaks the chain. Plus `AppGrant` + strict-mode enforcement for Enterprise brains.

See [3.7 Audit log](../03-core-subsystems/3.7-audit-log.md) for the full subsystem writeup; this page focuses on the feature contract and limitations.

## Where it lives

- [`crates/sca-core/src/audit.rs`](../../../crates/sca-core/src/audit.rs) — `AuditLog`, `AuditEntry`, `AppGrant`, `AppGrantRegistry`
- [`crates/sca-core/src/said_file.rs`](../../../crates/sca-core/src/said_file.rs) — hooks in `remember_with_pillar`, `forget`, all `admin_*`
- On-disk — `AUDT` section (magic-scanned, absent = empty log for back-compat)
- CLI — `said admin audit [--verify] [--actor] [--kind]`
- MCP — `admin action=audit`

## Inputs

### Write side (implicit)
Every mutating path in `SaidFile` auto-hooks. Mutations outside the canonical paths (direct `FrameStore::put_with`) bypass the log; see limitations.

### Actor override
```rust
brain.audit_mut().set_actor("app:slack-pack");
brain.remember_with_pillar(...);
// entry has actor="app:slack-pack"
```

### Read side
- `brain.audit() → &AuditLog`
- `AuditLog::verify() → Result<(), String>`
- `AuditLog::entries() → &[AuditEntry]`
- CLI + MCP as listed above

## Outputs

### Audit entry shape
```rust
pub struct AuditEntry {
    pub seq: u64,
    pub timestamp: u64,     // unix seconds
    pub actor: String,      // "owner" by default
    pub kind: String,       // "remember" / "delete" / "restore" / "legal_hold_add" / ...
    pub target: String,     // doc_id or empty
    pub detail: String,     // freeform
    pub hash: [u8; 32],     // BLAKE3 chain hash
}
```

### On-disk layout
```
[0..4]       b"AUDT"
[4..8]       u32 n_entries
per entry:
  u64 seq, u64 timestamp,
  u16 actor_len + bytes,
  u16 kind_len + bytes,
  u16 target_len + bytes,
  u32 detail_len + bytes,
  [u8; 32] hash
```

### Chain hash
```
hash_n = BLAKE3(hash_{n-1} || seq || ts || actor || 0x00 || kind || 0x00 || target || 0x00 || detail)
```

First entry uses `hash_0 = [0; 32]`.

## How to test

4 unit tests in [`audit.rs`](../../../crates/sca-core/src/audit.rs):

1. `chain_is_consistent` — append 3 entries, verify passes
2. `chain_detects_tampering` — mutate an entry's field after the fact, verify returns the break seq
3. `roundtrip_serialize` — serialize + deserialize preserves entries; chain still verifies
4. `grant_check` — `AppGrantRegistry` strict mode refuses unknown apps; wildcard matches all

All green as of 2026-04-22.

End-to-end functional test (on an Enterprise brain):

```
$ said create test.said --mode enterprise
$ said --path test.said admin legal-hold-add doc1 CASE-X
✓ Placed legal hold 'CASE-X' on 0 frame(s) for doc_id 'doc1'.

$ said --path test.said admin audit --verify
✓ Audit chain intact (1 entries, BLAKE3-verified).

$ said --path test.said admin audit
Audit log (1 of 1 entries):
  #0 [timestamp] legal_hold_add actor=owner target=doc1 case=CASE-X frames_tagged=0
```

## How to extend

### New audit kind
Add a `brain.audit_mut().append(kind, target, detail)` call at the new mutating site. Kinds are freeform strings; document new ones in [3.7 Audit log](../03-core-subsystems/3.7-audit-log.md)'s "Known kinds" list.

### Cross-brain audit export
Use `AuditLog::serialize()` / `deserialize()` directly. The serialized blob is self-contained.

### New AppGrant scheme
Extend `AppGrant` with more fields (e.g. `pillar_whitelist`, `doc_id_pattern`) and add new check functions. `AppGrantRegistry::check` stays the dispatch point.

## Known limitations

### Actor defaults to "owner" for MCP-originated actions
MCP dispatch doesn't extract `app_id` from request context yet. All MCP-originated mutating actions log with `actor="owner"`. Wiring: extract session/app-id at the top of every `handle_*`, call `brain.audit_mut().set_actor(id)` before the action, restore to `"owner"` after.

### AppGrant registry not wired into MCP dispatch
`AppGrantRegistry` exists + tested, but MCP server doesn't construct one for Enterprise brains or call `.check()` before mutating actions. Deployments get the audit trail but not the enforcement. Fix: TOML grant file at server start, `registry.set_strict(true)` for Enterprise, `registry.check(app_id, action)?` at the top of every mutating handler.

### Direct `put_with` callers bypass audit
Callers that use `FrameStore::put_with` directly (document_ingest, code_search, whisper_ingest) skip `remember_with_pillar`'s audit hook. Same sweep as the pillar persistence issue — see [Known limitations](../11-known-limitations.md).

## See also

- [3.7 Audit log](../03-core-subsystems/3.7-audit-log.md) — full subsystem writeup
- [Row 42 CLI admin](row-42-admin.md) — audit as a subcommand
- [Row 44 MCP admin](row-44-mcp-admin.md) — audit as an MCP action
