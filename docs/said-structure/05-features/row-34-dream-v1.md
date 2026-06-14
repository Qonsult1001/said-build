# Row 34 — Dream pipeline v1 (deprecated)

**Status:** ⚠ shipped as plumbing 2026-04-21 (Decision 5 v1); **content-consolidation path disabled** 2026-04-22 (Decision 5 v2 — see [Row 35](row-35-brain-state-dream.md)).

## What it was

Decision 5 v1 shipped a content-consolidation pipeline: cluster Episodic frames by fingerprint similarity, write Semantic summary frames. The goal was to turn raw dialogue turns into clean facts automatically.

**It didn't work.** Deterministic clustering without language understanding produces noisy summaries. Measurement showed the pipeline hurt LoCoMo R@10 by ~2 points.

## What shipped today

The **API surface remains** so callers don't break:

```rust
pub fn run_dream_content(
    &mut self,
    cycle: crate::dream::DreamCycle,
    _params: &crate::dream::DreamParams,
) -> crate::dream::DreamReport;
```

But the body is a no-op: it returns a zero-count `DreamReport` and doesn't write any frames.

```rust
// From said_file.rs
pub fn run_dream_content(...) -> crate::dream::DreamReport {
    crate::dream::DreamReport {
        cycle,
        ..Default::default()
    }
}
```

## Where the real dream lives now

See [Row 35 Brain-state auto-dream](row-35-brain-state-dream.md). The genuinely-valuable dream components (fingerprint-threshold drift, S_slow tensor accumulation, recall-weight reconsolidation) auto-fire on every `ask` / `search` call.

## Why content-consolidation was dropped

Under BYO-LLM, content distillation is the caller's LLM's job, not `.said`'s. A deterministic clusterer can decide *that* a group of frames is related but can't decide *what* the summary should say. LLM-at-write-time (mem0's approach) works but violates BYO-LLM. So:

- **`.said` binary:** never calls an LLM. Brain-state dream runs automatically — pure math.
- **Caller:** runs their LLM at read time. `said ask --deep "summarize this week"` pulls top-100 Episodic frames, the caller's LLM produces a summary, and if the caller wants it persisted they call `remember_with_pillar(Pillar::Semantic, ...)`.

Explicit, not implicit. Works well in practice.

## How to test

The stub:
```rust
let report = brain.run_dream_content(DreamCycle::Default, &DreamParams::default());
assert_eq!(report.candidates, 0);
assert_eq!(report.clusters_formed, 0);
assert_eq!(report.semantic_frames_created, 0);
```

## Known limitations

- MCP `dream` tool still exists and calls this no-op. The tool should be deprecated or rebranded for Row 35 once the UI sprint ships.

## See also

- [Row 35 Brain-state auto-dream](row-35-brain-state-dream.md)
- [3.3 Brain](../03-core-subsystems/3.3-brain.md)
