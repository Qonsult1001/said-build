# Row 35 — Brain-state auto-dream (Decision 5 v2)

**Status:** ✅ shipped 2026-04-22. Replaces Row 34's content-consolidation path with the genuinely-valuable brain-state components.

## What it does

Three automatic brain-state operations run on every `ask` / `search` call:

1. **S_slow tensor** — accumulates every remembered doc's embedding as a 64×64 outer product with `decay=0.999`. Supports cross-doc synthesis scoring at query time.
2. **Recall-weight reconsolidation** — each doc's `recall_weight` bumps on access, decays on idle. Warm docs rank higher next time.
3. **Fingerprint-threshold drift** — query embeddings accumulate; when count crosses `dynamic_dream_threshold(active_frames)`, the brain drifts its corpus mean/std toward the query distribution.

**Zero LLM. Pure math. Auto-fires. No user action.**

## Where it lives

- [`crates/sca-core/src/brain.rs`](../../../crates/sca-core/src/brain.rs) — `s_slow_write`, `s_slow_read`, `consolidate`, `dream`
- [`crates/sca-core/src/ask.rs`](../../../crates/sca-core/src/ask.rs) — `dynamic_dream_threshold(active_frames)`
- [`crates/sca-core/src/said_file.rs`](../../../crates/sca-core/src/said_file.rs) — **`maybe_dream()`: the single auto-fire trigger**, called at the end of `ask()` and `recall_by_pillar()`

**Auto-fire is in core (2026-06-22).** Dreaming is intrinsic to recall — `maybe_dream()`
fires inside `sca_core::ask::ask` and `SaidFile::recall_by_pillar`, so **every** caller
gets it: `said ask` (CLI), MCP `ask` + `search`, the Rust API, and the orchestrator.
The CLI/MCP handlers no longer trigger dream themselves (the old per-caller triggers in
`cmd_ask` / `handle_ask` / `handle_search` are removed — one source of truth, no drift).

## Inputs

None from the caller. Driven internally by write activity + query count.

## Dynamic threshold

```rust
pub fn dynamic_dream_threshold(active_frames: usize) -> u64 {
    let scaled = (active_frames / 10).max(50).min(500);
    scaled as u64
}
```

- Small brain (< 500 frames) → every 50 queries
- Medium brain (500-50k) → every `frames / 10`
- Large brain (> 50k) → every 500 queries

## Outputs

Persisted in BRAN section. Visible via `said stats`:

```
Brain State:
  Dream cycles:      3
  s_slow magnitude:  42.1729  (cross-doc synthesis signal)
  Pending dream:     34 queries  (auto-dreams at 500, threshold scales with corpus)
```

## How to test

```rust
let threshold = sca_core::ask::dynamic_dream_threshold(active_frames);
for n in 0..threshold+1 {
    // ask() auto-dreams in core — no manual trigger needed
    let _ = sca_core::ask::ask(&mut sf, "some query", 10, false, None);
}
// brain.consolidation_cycles should have incremented
assert!(sf.engine.brain.consolidation_cycles > 0);
```

> Note: the bare `SaidFile::recall()` Rust API does **not** auto-dream — only the
> recall-path entry points `ask()` and `recall_by_pillar()` call `maybe_dream()`. Drive
> those (as CLI/MCP do) to exercise dreaming.

Regression test: [`crates/sca-core/tests/test_dream_brain_state.rs`](../../../crates/sca-core/tests/test_dream_brain_state.rs).
End-to-end verified by 55 `said ask` calls → 1 dream cycle, s_slow 104.2.

## How to extend

- Change threshold curve: edit `dynamic_dream_threshold` in `sca_core::ask`
- Add a new brain-state component (e.g. an attention-head on top of S_slow): implement inside `Brain`, call from the auto-fire sites

## Known limitations

- MCP `dream` tool still exists as a no-op (from Row 34). Not harmful; could be removed.

## See also

- [Row 34 Dream v1 (deprecated)](row-34-dream-v1.md)
- [3.3 Brain](../03-core-subsystems/3.3-brain.md)
