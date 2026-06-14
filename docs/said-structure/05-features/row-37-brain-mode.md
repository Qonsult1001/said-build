# Row 37 — Immutable brain deployment mode

**Status:** ✅ shipped 2026-04-22

## What it does

At `said create`, the brain is tagged **Portable** (embeds content, offline-friendly) or **Enterprise** (pointer-only, refuses content embeds). **Mode is IMMUTABLE** — Portable and Enterprise are licensed separately; swapping is blocked by design.

## Where it lives

- [`BrainMode`](../../../crates/sca-core/src/said_file.rs) — enum + `create_with_mode`, `mode()`, `ensure_content_ingest_allowed`
- On-disk — `MODE` section (8 bytes); absent = Portable (back-compat)
- CLI — `said create <file> --mode {portable|enterprise}` and stats header
- MCP — `CreateTool.mode` field, `handle_create` routes to `create_with_mode`

## Inputs

- `--mode portable` (default) or `--mode enterprise` at create time
- `mode` string field on MCP `create` tool

After create: no mode-switch surface. No `said mode` command. No `BrainMode::set_mode` public API.

## Outputs

- `brain.mode()` returns `BrainMode::{Portable, Enterprise}`
- `said stats` shows `Brain mode: portable | enterprise`
- MCP `status` tool surfaces `Mode:` line
- `ensure_content_ingest_allowed()` returns `Err` on Enterprise → content-embedding ingest is refused

## Error on violation

```
This brain is in ENTERPRISE mode — content-embedding ingests are refused.
Use `--pointer` to register a searchable pointer without embedding content.
Enterprise and Portable are licensed separately and cannot be swapped —
create a fresh brain with `said create <file> --mode portable` if you need
full content embedding.
```

## MODE section layout

8 bytes, magic-scanned (not header-indexed):

```
[0..4]  b"MODE"
[4]     mode byte (0 = Portable, 1 = Enterprise)
[5..8]  reserved (zero)
```

## Back-compat

Any `.said` file written before 2026-04-22 has no MODE section. Readers default to Portable. Old readers ignore MODE entirely.

## How to test

```rust
let brain_e = SaidFile::create_with_mode("ent.said", BrainMode::Enterprise);
assert_eq!(brain_e.mode(), BrainMode::Enterprise);
assert!(brain_e.ensure_content_ingest_allowed().is_err());

let brain_p = SaidFile::create_with_mode("port.said", BrainMode::Portable);
assert_eq!(brain_p.mode(), BrainMode::Portable);
assert!(brain_p.ensure_content_ingest_allowed().is_ok());
```

CLI:
```
$ said create ent.said --mode enterprise
Created: ent.said (mode: enterprise, immutable)
$ said --path ent.said ingest doc.pdf
Error: This brain is in ENTERPRISE mode — ...
$ said --path ent.said ingest doc.pdf --pointer
Ingesting 1 file(s) as pointers...
✓ Pointer ingest complete
```

## How to extend

- Adding a new mode (e.g. `Archive`): enum variant + persistence byte + CLI `--mode` value + enforcement sites. Keep in mind the licensing contract — new modes should be thought through for distribution rights.

## Known limitations

- No "read-only" mode. An Archive-like immutable read state could be added.
- No way to enforce mode-appropriate **queries** yet (e.g. Enterprise could refuse any retrieval that would reveal a content body). Only enforces at write.

## See also

- [External pillar](../04-four-pillars/external.md) — what Enterprise ingests look like
- [Row 36 Enterprise pointer mode](row-36-external-pointer.md)
- [2.2 Sections](../02-file-format/2.2-sections.md#mode-deployment-mode)
