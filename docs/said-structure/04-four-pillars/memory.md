# Pillar — Memory (legacy)

The default / catch-all pillar for content that didn't get classified explicitly. Everything written before Decision 1 shipped (2026-04-21) used this pillar by default.

## What goes here

- Any frame loaded from a pre-Decision-1 `.said` file (no pillar byte in the TOC)
- Explicit `remember(content)` calls that didn't specify a pillar
- Document-ingest chunks from the current `said ingest` path (until the pillar persistence sweep completes — see [Known limitations](../11-known-limitations.md))

## Why it exists

Forward-compatibility. A `.said` file written by a v7 writer (no pillar) must load cleanly on a v7_1 reader. The reader sets `Pillar::from_memory_type(memory_type)` for frames missing the byte, so:

- `MemoryType::Episodic` → `Pillar::Episodic`
- `MemoryType::Procedural` → `Pillar::Procedural`
- `MemoryType::Factual` → `Pillar::Semantic` (NOT `Memory`)
- `MemoryType::Relational` → `Pillar::Memory`
- `MemoryType::Meta` → `Pillar::Memory`

In practice, most legacy frames map cleanly to one of the 4 real pillars. `Memory` is the safety net for anything that doesn't.

## Writer APIs

```rust
// Explicit Memory pillar (rarely what you want — prefer Episodic / Semantic / Code)
brain.remember_with_pillar(doc_id, content, title, Pillar::Memory, extra_tags);

// Legacy API — falls back to MemoryType::Episodic → Pillar::Episodic, NOT Memory
brain.remember(content);     // → actually produces Pillar::Episodic
brain.remember_as(doc_id, content, title);  // → same
```

So `Pillar::Memory` is explicit-opt-in territory today. Most code paths produce one of the four specialized pillars.

## Tags applied automatically

- `pillar:memory`

(No other pillar-specific tags.)

## Migration path

The recommended move-off-Memory sequence for an existing brain:

1. Walk `frames.get_all_frames()` filtered to `pillar == Memory`
2. For each frame, decide the correct pillar based on tags / heuristics
3. Call `brain.frames.set_pillar(frame_id, new_pillar)`
4. Save

Or more simply: accept that Memory is legacy and rely on tag-scoped retrieval (every frame still carries the `pillar:<name>` tag, regardless of what the FrameMeta byte says).

## Retrieval ranking

Standard SCA + BM25 + graph fan-out, no special handling. Memory frames compete on an equal footing with every other pillar.

## How to test

```rust
let fid = sf.remember_with_pillar(
    Some("legacy_note"),
    "a free-form memory",
    None,
    Pillar::Memory,
    vec![],
);
assert_eq!(sf.frames.get_meta("legacy_note").unwrap().pillar, Pillar::Memory);
```

## Deprecation note

New callers should **prefer the specialized pillars** (Episodic / Semantic / Procedural / External / Code) because:

- Per-pillar retrieval scoping only works if frames are correctly classified
- Dream logic (when expanded) will consult pillar as a routing signal
- Future rankings (Episodic recency decay, Procedural task-match) target specific pillars

Memory is supported indefinitely for back-compat but it's the pillar you use when nothing else fits.

## See also

- [2.3 Frame layout](../02-file-format/2.3-frame-layout.md) — how pillar is serialized
- [3.4 FrameStore](../03-core-subsystems/3.4-framestore.md) — `set_pillar`, `from_memory_type` fallback
- [Known limitations](../11-known-limitations.md) — pillar persistence sweep
