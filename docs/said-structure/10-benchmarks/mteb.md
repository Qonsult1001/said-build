# MTEB long-context benchmarks

The public yardstick. Every retrieval change in SAID is gated against MTEB LongEmbed and two long-form QA datasets. Current scores — measured 2026-04-16 after the pipeline cleanup + ART commit, held stable through pillar + dream + admin + audit shipping:

| Benchmark | Score | Notes |
|-----------|-------|-------|
| **LongEmbed — Needle** | **1.00000** | 10/10 passkeys retrieved at any depth |
| **LongEmbed — Passkey** | **1.00000** | 10/10 needles retrieved |
| **LongEmbed — WikimQA** | **1.00000** | All 500 queries top-1 |
| **SummScreenFD** | **0.97974** | 329 / 336 top-1 retrievals |
| **QMSum** | **0.89220** | Query-focused meeting summaries |
| **NarrativeQA retrieval** | **0.72100** | Baseline — room to improve with multi-hop bridge |

Harness: [`crates/sca-core/examples/mteb_rust.rs`](../../../crates/sca-core/examples/mteb_rust.rs).

## Running

```bash
cd crates/sca-core
cargo run --release --example mteb_rust --features static-embed -- <task>
```

Tasks: `needle`, `passkey`, `wikimqa`, `summscreen`, `qmsum`, `narrativeqa`, or `all`.

## What each one exercises

### Needle / Passkey — fingerprint precision
Dataset: LongEmbed needle-in-haystack. A single sentence ("the passkey is …") is planted at varying depths inside a 32k-token context. Success = retrieve the sentence.

What it tests: SCA top-50 + trigram index precision. If the fingerprint is blurry or the trigram index drops the plant, this regresses. Scored 1.0 since the 64-dim static encoder went in.

### WikimQA — deep hop coverage
Dataset: multi-hop Wikipedia queries. 500 queries, each needing one specific Wikipedia paragraph.

What it tests: SCA + entity boost + token-overlap scoring all together. Scored 1.0 since the entity-speaker boost (row 5) landed.

### SummScreenFD — noisy document retrieval
Dataset: 336 TV show episode summaries, each paired with a paraphrased query.

What it tests: semantic-only retrieval (no keyword overlap survives the paraphrase). 0.97974 = 329 / 336 — the 7 misses are queries whose entities don't appear in the gold doc, a known upper bound on static embedding + no-LLM systems.

### QMSum — query-focused summarization retrieval
Dataset: meeting transcripts + extractive queries.

What it tests: multi-turn context extraction + hyphen-asymmetric query handling (meeting transcripts have lots of "re-" / "un-" / "co-" hyphens).

### NarrativeQA retrieval
Dataset: long-form book/movie-script retrieval. Queries are free-form; gold is a single passage.

What it tests: the graph fan-out + multi-hop bridge path. 0.721 is the current ceiling — improving this is on the roadmap.

## What must not regress

From `SAID_MVP_PLAN.md` rows 1-6:

- Needle: 1.00000
- WikimQA: ≥ 0.99 (budget 0.01 for run-to-run variance; observed 1.0 every run)
- SummScreenFD: ≥ 0.97
- QMSum: ≥ 0.88
- NarrativeQA: ≥ 0.70

Any PR that lands below these floors needs an explicit waiver.

## History

- **2026-04-11** — SummScreenFD first crossed 0.95 after chunk-aware scoring fix (335/336 top-10).
- **2026-04-12** — Full MTEB sweep: 3× perfect (Needle/Passkey/Wiki), SummFD 0.9808, QMSum 0.8931, NarrQA 0.7210.
- **2026-04-16** — Pipeline cleanup + ART retuning: Wiki 1.0, SummFD 0.9831, QMSum 0.9003.
- **2026-04-21** — Post-pillar retrieval shipped (Decision 2): identical scores confirmed, zero regression from pillar filter.

## Related memory

- [project_mteb_scores_apr12.md](../../../memory/project_mteb_scores_apr12.md)
- [project_mteb_scores_apr16.md](../../../memory/project_mteb_scores_apr16.md)

## See also

- [3.5 Retrieval pipeline](../03-core-subsystems/3.5-retrieval-pipeline.md) — how the 9 layers compose to hit these scores
- [LoCoMo](locomo.md) — the separate conversational-memory track
