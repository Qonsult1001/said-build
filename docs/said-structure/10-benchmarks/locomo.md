# LoCoMo — conversational memory benchmark

Separate benchmark track from MTEB. Purpose: measure long-context **conversational** recall — "given a chat history with turns about X, Y, Z spanning weeks, can the brain retrieve the right turn when asked about X later?"

We adopted LoCoMo to validate the four-pillar design (Episodic/Semantic/Procedural/External/Code). Pre-pillar baseline is the anchor; every post-pillar change is compared against it.

## Dataset

- 1982 QA pairs across 10 categories (multi-hop, temporal, adversarial, negation, …)
- Harness: `[crates/sca-core/examples/locomo_baseline.rs](../../../crates/sca-core/examples/locomo_baseline.rs)`
- Run: `cargo run --release --example locomo_baseline --features static-embed`

## Pre-pillar baseline (2026-04-21)

Recorded before any pillar/dream/surprise work shipped:


| Metric              | Value                 |
| ------------------- | --------------------- |
| Overall R@10        | **0.554**             |
| Cat 1 — single-hop  | 0.612                 |
| Cat 2 — multi-hop   | **0.617** (strongest) |
| Cat 3 — temporal    | **0.391** (weakest)   |
| Cat 4-10 — assorted | 0.50 – 0.58           |


Strong on multi-hop (thanks to graph fan-out), weak on temporal (no time-tag scoring yet). Memory recorded in [project_locomo_baseline.md](../../../memory/project_locomo_baseline.md).

## Regression guard

Every pillar shipment was measured against the 0.554 baseline. Actuals:


| Shipment                                             | LoCoMo R@10 | Delta      |
| ---------------------------------------------------- | ----------- | ---------- |
| Baseline                                             | 0.5540      | —          |
| Decision 1 — Pillar enum in FrameMeta                | 0.5540      | 0          |
| Decision 2 — Per-pillar retrieval scope              | 0.5540      | 0          |
| Decision 3 — Episodic writer + hooks                 | 0.5540      | 0          |
| Decision 4 v0 — Salience heuristic                   | 0.5540      | 0          |
| Decision 5 v1 — Dream content concatenation          | ~0.535      | **−0.019** |
| Decision 5 v2 — Brain-state dream (no content touch) | 0.5540      | 0          |


Decision 5 v1 (content-concatenation-as-distillation) regressed and got pulled. v2 keeps S_slow + recall_weight drift but does **not** touch frame content — zero regression, some upstream benefit on MTEB fingerprint stability. Memory: [project_dream_v1_shipped.md](../../../memory/project_dream_v1_shipped.md).

## Comparison against mem0

External harness (Prometheus-style: oracle LLM judges F1 externally; `.said` doesn't call an LLM):


| Metric                                | SAID       | mem0  |
| ------------------------------------- | ---------- | ----- |
| F1 (20 QAs sample, GPT-4o-mini judge) | **0.8555** | 0.684 |


Memory: [project_byo_llm_harness.md](../../../memory/project_byo_llm_harness.md).

This is a retrieval-quality comparison, not a latency comparison (SAID is also 100-1000× faster on the retrieval step because there's no LLM call in the hot path — but latency is measured elsewhere).

## Why LoCoMo belongs in its own track

LoCoMo is conversational; MTEB is document-based. They stress different parts of the system:

- **MTEB** exercises fingerprint precision + trigram + symbol index. Stable at near-perfect.
- **LoCoMo** exercises pillar routing + temporal reasoning + multi-turn graph hops. Currently at 0.55 R@10 — ceiling is higher once temporal/pillar distillation lands.

Separate tracks, separate floors, no mixing the numbers.

## What's next

The 0.391 temporal category is the current bottleneck. Two planned improvements (both on the roadmap, not yet shipped):

1. **Time-tag-aware scoring** — boost frames whose timestamp overlaps with the query's temporal range.
2. **Episodic timeline pillar distillation** — summarize long sequences of Episodic frames into daily/weekly anchors that rank highly on temporal queries.

See [12-roadmap.md](../12-roadmap.md).

## Related memory

- [project_locomo_baseline.md](../../../memory/project_locomo_baseline.md) — raw 1982-QA baseline
- [project_dream_v1_shipped.md](../../../memory/project_dream_v1_shipped.md) — what broke in v1, why v2 works
- [project_benchmarks_we_run.md](../../../memory/project_benchmarks_we_run.md) — why LoCoMo lives on a different track from MTEB

## See also

- [MTEB](mteb.md) — the document-retrieval yardstick
- [Row 31 — Per-pillar retrieval scope](../05-features/row-31-per-pillar-retrieval.md)
- [Row 35 — Brain-state dream](../05-features/row-35-brain-state-dream.md)

