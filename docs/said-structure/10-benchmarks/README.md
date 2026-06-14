# Benchmarks

Everything shipped in `SAID-ECHO` that takes a reproducible measurement. These aren't marketing numbers — each page links to the source harness, the dataset, and the exact command that produces the score.

## Index

- [MTEB long-context](mteb.md) — WikimQA, Needle, SummScreenFD, QMSum, NarrativeQA-retrieval (rows 1-6)
- [LoCoMo](locomo.md) — long-context conversational memory F1/R@10 (rows 24-25, baseline + pillar experiments)
- [Real-world probes](realworld-probes.md) — 30-chamber stress tests, cross-domain recall sanity (rows 7, 19, 27)
- [BEIR short-doc](beir-short-doc.md) — sanity check the short-doc path is not regressing
- [Chambers](chambers.md) — what a "chamber" is, why we use them, how to add one

## Why we run every benchmark

Each row in `SAID_MVP_PLAN.md` has an associated benchmark that can block the release if it regresses. Every retrieval change — hyphen normalization, entity-speaker boost, BM25 fusion, pillar rerank — goes through this gate:

1. **MTEB LongEmbed** must stay at 1.0 / 0.98+ / 0.97+ on Needle / WikimQA / SummScreenFD.
2. **LoCoMo baseline** must stay at 0.554 R@10 or improve.
3. **Chamber pass-rate** must stay at 30/30 or improve.

A change that improves one but regresses another requires an explicit waiver in the commit message.

## How to run a benchmark

All harnesses live in `crates/sca-core/examples/`. They compile with `--features static-embed` and expect the `said-lam-static` folder to be adjacent. Common pattern:

```bash
cd crates/sca-core
cargo run --release --example mteb_rust --features static-embed -- needle
cargo run --release --example locomo_baseline --features static-embed
cargo run --release --example realworld_recall_probe --features static-embed
```

Each harness prints a JSON line at the end that's trivially grep-able / diff-able across runs.

## BYO-LLM constraint

These benchmarks measure retrieval only. `.said` never calls an LLM itself — see [project memory: byo_llm_harness](../../../memory/project_byo_llm_harness.md). When an external harness (e.g. the mem0 comparison) needs LLM judgment for F1 scoring, it uses a separate Claude Opus 4.7 oracle outside the `.said` process.
