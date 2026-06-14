# Row 39 — Competitor benchmark sweep (spec)

**Status:** ⏳ planned; harness shell shipped — see [Row 49](row-49-competitor-bench.md).

## What the roadmap entry says

> Scripted harness that runs `.said` against mem0 (OSS + paid cloud), Zep, LangMem, Letta, memvid, pgvector, ChromaDB, HippoRAG, GraphRAG-lite, Cognee, LightRAG on LoCoMo + MTEB + BEIR + realworld probe. Result matrix lives in `docs/competitor_benchmark.md` with honest per-system summaries ("where they beat us / where we win / where we match"). Refreshed quarterly. No hand-tuned demos — every number reproducible from a checked-in script.

## Shipped: harness shell

[Row 49](row-49-competitor-bench.md) ships the harness itself — a Rust example that writes `docs/competitor_benchmark.json` with SAID-ECHO's live numbers and placeholder rows for every competitor. Running it populates a matrix like:

```
system                  LoCoMo F1     Needle      WikimQA    SummSFD    QMSum
──────────────────────────────────────────────────────────────────────────────
SAID-ECHO                   0.856      1.000       1.000      0.980    0.890
mem0 (OSS)                  0.669          —           —          —        —
mem0 (with-graph)           0.684          —           —          —        —
Zep                             —          —           —          —        —
...
```

## Gap: competitor rows not populated

All non-SAID-ECHO rows are `null` (or hand-encoded published numbers for mem0). Filling them requires:

1. Containerize each competitor (Docker Compose or similar)
2. Scripted ingest of the same benchmark corpora (LoCoMo, MTEB LongEmbed datasets, BEIR tasks)
3. Scripted query pass recording NDCG@10 / F1 / latency
4. Emit one JSON line per (system, benchmark) to be aggregated into the matrix

This is an ops task — not a core code task. Lives outside the `sca-core` crate. Could live in a sibling `bench/` directory at the repo root with per-competitor Dockerfiles.

## Deliverables (when this ships)

- `docs/competitor_benchmark.md` — human-readable matrix
- `docs/competitor_benchmark.json` — machine-readable (already partially shipped by Row 49)
- Reproducible scripts in `bench/` — one per competitor stack
- CI job that regenerates the matrix quarterly

## Why it matters

Benchmarks are the #1 question from enterprise buyers evaluating `.said` vs mem0/Zep/etc. An empty matrix looks defensive; a populated one is how we win (or honestly lose) evaluations.

## See also

- [Row 49 Competitor benchmark harness](row-49-competitor-bench.md)
- [10 Benchmarks we run](../10-benchmarks/README.md)
- [Roadmap](../12-roadmap.md)
