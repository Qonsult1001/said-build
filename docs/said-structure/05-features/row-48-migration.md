# Row 48 — Migration adapters (shipped surface)

**Status:** ✅ shipped 2026-04-22. Two adapters — memvid + mem0 JSONL — are live. Zep / LangMem / mem0-SQLite documented in [Row 38 spec](row-38-migration-spec.md).

## What it does

`said import from --from <adapter> --source <path>` reads a competitor's export file and writes matched records into `.said` frames via the right pillar. Source metadata (timestamps, user_ids, categories) survives as tags so admin tools can still query provenance after migration.

> **Syntax note:** migration is the `from` subcommand of `said import` (`said import from --from …`). The bare `said import --from …` form is obsolete — `import` now takes a subcommand (`browser` / `email` / `chatgpt` / `claude` / `from`) since personal-data import shipped. See [CLI import](../07-cli-reference/import.md).

## Where it lives

- [`crates/sca-core/src/migrate.rs`](../../../crates/sca-core/src/migrate.rs) — `MigrationAdapter` trait, `MigratedRecord`, `run_migration`, `MemvidAdapter`, `Mem0Adapter`, `adapter_for`, `registered_adapters`
- CLI — `said import from --from <name> --source <path>` in [`said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs)
- MCP — the `import` tool exists for **personal-data** import (browser/email), but **competitor migration adapters are CLI-only** — no `from`-style migration over MCP yet

## Inputs

```
said import from --from memvid --source /path/to/memories.json
said import from --from mem0 --source /path/to/memories.jsonl
said import from --list    # list registered adapters
```

Enterprise brains refuse content-bearing imports unless the adapter maps records to External pillar. Caller gets a clear error with remediation (switch to Portable or modify the adapter).

## Per-adapter contract

### MemvidAdapter — JSON array

Input file: single JSON array.

```json
[
  {"id": "a", "content": "Alice prefers dark mode", "timestamp": 1700000000,
   "metadata": {"user": "alice"}},
  {"id": "b", "content": "Bob's deadline is Friday", "metadata": {"user": "bob"}}
]
```

Mapping:
- `content` → frame body
- `id` → `doc_id = memvid:<id>`
- `timestamp` → `ingested_at:<unix>` tag
- `metadata.<key>` (string values only) → `<key>:<value>` tag
- All records → Pillar::Episodic

### Mem0Adapter — JSONL export

One JSON object per line (from mem0's `export_memories()`):

```jsonl
{"id":"m1","memory":"User is vegetarian","user_id":"u42","categories":["preference"]}
{"id":"m2","memory":"Deploy sequence: ...","categories":["procedure"]}
{"id":"m3","memory":"Turn transcript...","categories":["turn"]}
```

Mapping:
- `memory` → frame body
- `id` → `doc_id = mem0:<id>`
- `user_id` → `user_id:<id>` tag
- `created_at` → `ingested_at:<iso>` tag
- `categories` → `category:<name>` tags + pillar routing:

| Category | Pillar |
|---|---|
| `preference` (default) | Semantic |
| `turn` / `dialog` / `conversation` | Episodic |
| `plan` / `action` / `procedure` / `recipe` | Procedural |
| `document` / `file` / `reference` | External |
| anything else | Semantic |

## Outputs

```rust
pub struct MigrationReport {
    pub source_system: String,
    pub records_read: usize,
    pub records_written: usize,
    pub records_skipped: usize,
    pub per_pillar: HashMap<String, usize>,
    pub errors: Vec<String>,
}
```

CLI stdout:
```
✓ Imported from mem0:
  read:    4217
  written: 4217
  pillars: semantic=2100, episodic=1820, procedural=140, external=157
```

Every written frame carries `imported_from:<system>` so post-import queries can identify provenance.

## How to test

3 unit tests in [`migrate.rs`](../../../crates/sca-core/src/migrate.rs):

1. `memvid_round_trip` — fixture JSON → correct records + tags
2. `mem0_pillar_mapping` — categories route to the right pillars
3. `adapter_registry_resolves` — `adapter_for("MEM0")` (case-insensitive) works

All green as of 2026-04-22.

Functional smoke in [`examples/pillar_writers_probe.rs`](../../../crates/sca-core/examples/pillar_writers_probe.rs):
```
=== Migration adapter — memvid ===
  read=2 written=2 skipped=0 pillars={"episodic": 2}

=== Migration — mem0 JSONL ===
  read=3 written=3 pillars={"semantic": 1, "procedural": 1, "episodic": 1}
```

## How to extend

New adapter:
1. Create a struct (e.g. `ZepAdapter`)
2. `impl MigrationAdapter { fn name, fn load }`. `load` parses the export format into `Vec<MigratedRecord>`
3. Register in `migrate::adapter_for(name)` + add to `registered_adapters()` list
4. Add unit test for the format mapping
5. Document here + [Row 38](row-38-migration-spec.md)

Keep the competitor's id stable (prefix with system name like `zep:`) so provenance queries work.

## Known limitations

- mem0 SQLite not supported — users must export to JSONL first
- Zep / LangMem adapters not yet shipped — schemas in flux
- Competitor migration is CLI-only — the MCP `import` tool covers personal-data (browser/email) but not `from`-style migration adapters

## See also

- [Row 38 Migration adapters (spec)](row-38-migration-spec.md)
- [Semantic pillar](../04-four-pillars/semantic.md) — where mem0 preferences land
