# Row 30 — Pillar enum on FrameMeta

**Status:** ✅ shipped 2026-04-21 (Decision 1)

## What it does

Every frame carries a `Pillar` label — Episodic, Semantic, Procedural, External, Code, or Memory. The label is the primary routing discriminator for retrieval scoping (Row 31), dream / admin tooling, and future per-pillar ranking.

## Where it lives

- [`crates/sca-core/src/frames.rs`](../../../crates/sca-core/src/frames.rs) — `enum Pillar`, `FrameMeta.pillar`, `Pillar::from_memory_type`
- On-disk — one byte per frame in the FTOC section (see [2.3 Frame layout](../02-file-format/2.3-frame-layout.md))

## Inputs

A `Pillar` value when writing a frame, passed to `remember_with_pillar` or one of the `remember_as_*` wrappers. Frames loaded from pre-Decision-1 files default to `Pillar::from_memory_type(memory_type)`.

## Outputs

- `FrameMeta.pillar` on every frame returned by store iteration APIs
- Auto-written `pillar:<name>` tag as a redundant tag-scoped discriminator

## How to test

```rust
let fid = sf.remember_with_pillar(
    Some("t1"),
    "test content",
    None,
    Pillar::Procedural,
    vec![],
);
assert_eq!(sf.frames.get_meta("t1").unwrap().pillar, Pillar::Procedural);
```

See [`examples/pillar_writers_probe.rs`](../../../crates/sca-core/examples/pillar_writers_probe.rs) for full fixture.

## How to extend

Adding a new pillar variant:
1. Add to `enum Pillar`
2. Update `Pillar::from_memory_type` mapping
3. Update `Pillar::from_byte` / `as_byte` serialization
4. Update `remember_with_pillar`'s `memory_type` match
5. Add retrieval ranking in `rerank_by_pillar` if the new pillar needs one
6. Document in [four-pillars](../04-four-pillars/) and this page

## Known limitations

- `Pillar::from_memory_type(Factual) → Semantic`, so direct `FrameStore::put_with` callers that want Code / Procedural / External get `Semantic` on disk unless they also call `set_pillar`. Fixed in the `remember_as_*` wrappers; direct callers (document_ingest, code_search, whisper_ingest) still need a sweep. See [Known limitations](../11-known-limitations.md).

## See also

- [Four-pillars intro](../04-four-pillars/README.md)
- [2.3 Frame layout](../02-file-format/2.3-frame-layout.md)
