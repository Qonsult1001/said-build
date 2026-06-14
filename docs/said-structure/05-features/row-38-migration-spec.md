# Row 38 — Migration adapters (spec)

**Status:** ⏳ planned / partially shipped. The contract is defined. Two of four target adapters ship — see [Row 48 Migration adapters](row-48-migration.md).

## What the roadmap entry says

> One-line migration from competitors (mem0, memvid, Zep, LangMem) — `said import --from <mem0|memvid|zep|langmem> <path>` pulls their on-disk/API exports into `.said` frames via a shared `MigrationAdapter` trait. Preserves timestamps, user_ids, and source metadata as tags.

## Status per adapter

| Adapter | Shipped? | Source format | Notes |
|---|---|---|---|
| memvid | ✅ | JSON array | Reference implementation; simplest |
| mem0 | ✅ | JSONL export | mem0's `export_memories()` output shape |
| mem0 SQLite | ❌ | SQLite dump | Not shipped — would pull `rusqlite` into sca-core |
| Zep | ❌ | Session JSON | Export format still in flux |
| LangMem | ❌ | JSONL | LangChain memory schema moved twice in 2026 |

## Shipped surface

See [Row 48](row-48-migration.md) for the full shipped contract. Overview:

```rust
pub trait MigrationAdapter {
    fn name(&self) -> &'static str;
    fn load(&self, path: &Path) -> Result<Vec<MigratedRecord>, String>;
}

pub fn run_migration(
    adapter: &dyn MigrationAdapter,
    source: &Path,
    brain: &mut SaidFile,
) -> Result<MigrationReport, String>;
```

## Why Zep / LangMem / mem0-SQLite aren't done

**mem0 SQLite:** pulls `rusqlite` + C deps (~1 MB) into sca-core. Better shipped as a separate opt-in crate (`said-migrate-mem0-sqlite`) so the core binary stays minimal. Users can run mem0's `export_memories()` → JSONL to use the shipped adapter today.

**Zep:** session export JSON format changed twice in early 2026. Waiting for stability before writing the adapter.

**LangMem:** LangChain's memory primitives are moving fast (breaking changes between 0.3 and 0.4). Adapter work would be rewrite bait.

## Planned work

- `said-migrate-mem0-sqlite` crate with feature-gated `rusqlite` dep
- `ZepAdapter` when session export stabilizes (~1 day of work given the shipped trait)
- `LangMemAdapter` when schema settles (~1 day)

## See also

- [Row 48 Migration adapters — shipped surface](row-48-migration.md)
- [Roadmap](../12-roadmap.md)
