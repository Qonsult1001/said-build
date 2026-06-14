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
- [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) — `cmd_ask` auto-fires
- [`crates/said-mcp/src/handler.rs`](../../../crates/said-mcp/src/handler.rs) — `handle_ask` + `handle_search` auto-fire

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
for _ in 0..threshold+1 {
    let _ = sf.recall("some query", 10);
}
// brain.consolidation_cycles should have incremented
assert!(sf.engine.brain.consolidation_cycles > 0);
```

End-to-end verified by repeated `said ask` calls on `willie.said` (19,149 frames → threshold 500).

## How to extend

- Change threshold curve: edit `dynamic_dream_threshold` in `sca_core::ask`
- Add a new brain-state component (e.g. an attention-head on top of S_slow): implement inside `Brain`, call from the auto-fire sites

## Known limitations

- MCP `dream` tool still exists as a no-op (from Row 34). Not harmful; could be removed.

## See also

- [Row 34 Dream v1 (deprecated)](row-34-dream-v1.md)
- [3.3 Brain](../03-core-subsystems/3.3-brain.md)
