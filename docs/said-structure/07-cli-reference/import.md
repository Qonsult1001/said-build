# said import

Migrate memories from competitor systems (mem0, memvid) into a `.said` brain.

## Usage

```
said [--path FILE] import --from <ADAPTER> --source <PATH>
said import --list
```

## Arguments

- `--from <ADAPTER>` — source system; one of `memvid`, `mem0`
- `--source <PATH>` — path to the competitor's export file
- `--list` — print registered adapters and exit

## Behavior

1. Resolve the adapter via `sca_core::migrate::adapter_for(name)`
2. Open the target brain
3. Call `run_migration(adapter, source_path, &mut brain)`:
   - Adapter parses its format → `Vec<MigratedRecord>`
   - Driver maps each record to the right pillar (see per-adapter mapping below)
   - Writes each via `remember_with_pillar` (gets audit log, surprise detection, S_slow accumulation)
4. `brain.build_index()` + `brain.save()`

## Per-adapter expectations

### memvid

Expects a JSON array of objects:

```json
[
  {"id": "a", "content": "Alice prefers dark mode", "timestamp": 1700000000,
   "metadata": {"user": "alice"}},
  ...
]
```

Mapping:
- Every record → `Pillar::Episodic`
- `doc_id = "memvid:<id>"`
- `timestamp` → `ingested_at:<unix>` tag
- `metadata.<key>` → `<key>:<value>` tag (string values only)

### mem0

Expects JSONL (one object per line) from mem0's `export_memories()`:

```jsonl
{"id":"m1","memory":"User is vegetarian","user_id":"u42","categories":["preference"]}
{"id":"m2","memory":"Deploy sequence","categories":["procedure"]}
```

Mapping by category:
- `preference` (default) → Semantic
- `turn` / `dialog` / `conversation` → Episodic
- `plan` / `action` / `procedure` / `recipe` → Procedural
- `document` / `file` / `reference` → External
- other → Semantic

Tags: `user_id:<id>`, `ingested_at:<iso>`, `category:<name>` per record. Plus `imported_from:mem0`.

## Output

```
✓ Imported from memvid:
  read:    147
  written: 147
  pillars: episodic=147
```

mem0 mixed:
```
✓ Imported from mem0:
  read:    4217
  written: 4217
  pillars: semantic=2100, episodic=1820, procedural=140, external=157
```

JSON mode:

```json
{
  "source_system": "mem0",
  "records_read": 4217,
  "records_written": 4217,
  "records_skipped": 0,
  "per_pillar": {"semantic": 2100, "episodic": 1820, "procedural": 140, "external": 157},
  "errors": []
}
```

## Enterprise mode guard

Enterprise brains refuse content-bearing imports that aren't External-pillar. A mem0 dump with `turn` category rows gets rejected unless the target brain is Portable:

```
✗ skipped 'mem0:m3' — Enterprise brain refuses Episodic content embed
```

The `errors` field of the report lists per-record skip reasons so ops can see why the import didn't write everything.

## Examples

```bash
# List adapters
said import --list
# → Registered migration adapters:
#     - memvid
#     - mem0

# Import mem0 JSONL export into a fresh brain
said create migrated.said --mode portable
said --path migrated.said import --from mem0 --source ~/memories.jsonl

# Import memvid into an existing brain (merges with current content)
said --path my-brain.said import --from memvid --source ~/memvid-backup.json
```

## How to extend

New adapter:
1. Create a struct implementing `MigrationAdapter` in [`crates/sca-core/src/migrate.rs`](../../../crates/sca-core/src/migrate.rs)
2. Register in `adapter_for(name)` + add to `registered_adapters()` list
3. Add unit test for the format mapping
4. Document in [Row 48 Migration adapters](../05-features/row-48-migration.md) and [Row 38 Migration spec](../05-features/row-38-migration-spec.md)

## Known limitations

- mem0 SQLite export not supported (users must run `export_memories()` to JSONL first)
- Zep / LangMem adapters not yet shipped
- No MCP `import` tool — migrations are CLI-only today

## See also

- [Row 48 Migration adapters](../05-features/row-48-migration.md)
- [Row 38 Migration spec](../05-features/row-38-migration-spec.md)
