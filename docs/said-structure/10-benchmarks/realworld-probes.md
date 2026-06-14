# Real-world probes

Mid-sized stress tests that sit between chambers (3-30 frames) and MTEB (500-30k frames). Probes ingest an actual personal or project corpus and check that recall survives the noise.

## What's in `crates/sca-core/examples/`

From `Cargo.toml`:

| Probe | Required features | Purpose |
|-------|-------------------|---------|
| `realworld_recall_probe` | `static-embed` | End-to-end retrieval sanity on a live brain |
| `test_folder_recall` | `docs static-embed` | Ingest a folder of PDFs + Markdown and query it |
| `diagnose_intent_miss` | `static-embed` | Trace which engine (SCA/Grep/Sym) missed a specific query |
| `check_query_routes` | `static-embed` | Log which routing path each query took |
| `surprise_probe` | `static-embed` | Exercise the Surprise detector on adversarial pairs |
| `competitor_bench` | `static-embed` | Side-by-side with mem0/memvid on an identical corpus |
| `pillar_writers_probe` | `static-embed` | Verify pillar tags land on the right frames via `remember_as_external/procedural/code` |
| `locomo_miss_trace` | `static-embed` | Walk specific LoCoMo R@10 misses to understand the failure mode |
| `locomo_sca_trace` | `static-embed` | Isolate SCA's contribution to LoCoMo |
| `locomo_ceiling_search_internal` | `static-embed` | Compute the theoretical retrieval ceiling given current indexes |
| `locomo_mem0_test` | `static-embed` | Run the mem0 migration adapter against LoCoMo inputs |

## Running a probe

```bash
cd crates/sca-core
cargo run --release --example realworld_recall_probe --features static-embed
```

Most probes read their corpus from a path passed on the command line or default to `./fixtures/<probe-name>/`. They print a JSON summary at the end.

## What real-world probes protect that chambers don't

- **Corpus-scale noise tolerance.** A chamber has 3-30 frames. A probe has 300-3000. Retrieval that works at chamber size can drown in a probe-sized corpus if the SCA fingerprint is too lossy or the trigram index is too permissive.
- **Cross-plugin interactions.** `test_folder_recall` ingests via `docs` → indexes via `code` → searches via `ask`. A probe failure often points at a plugin handoff bug that no single-plugin chamber catches.
- **Ranking stability across restarts.** Probes save/load the brain and rerun the same queries. Divergent results = a non-deterministic ranking (e.g. HashSet iteration order leaking into doc_id ordering).

## Competitor bench

`competitor_bench.rs` is the harness referenced in [Row 49](../05-features/row-49-competitor-bench.md). It:

1. Ingests the same corpus into SAID, mem0 (via their Python client), and memvid.
2. Runs an identical query set.
3. Writes `{retrieval_recall, latency_ms, binary_size}` per system.

Results from the last sweep (2026-04-21): SAID leads on retrieval quality (by 15+ percentage points) and on latency (no LLM hot path), loses to memvid on ingest throughput for short documents.

## Intent miss diagnostics

`diagnose_intent_miss` is the probe to run when `ask` returns the wrong answer and you don't know why. It takes a query + expected doc_id and prints a trace:

```
Query: "when did Alice say she'd ship"
Expected: episodic_2026_03_05_alice_ship

[Sym]    miss — query has no symbol-candidate spelling
[Grep]   miss — no trigram hit above threshold 0.40
[SCA]    hit at rank 7 (confidence 0.34, below relative cutoff 0.36)

Diagnosis: SCA fingerprint diluted by two-word overlap ("Alice" + "ship").
Suggested fix: boost via entity-speaker tag OR widen SCA_GUARANTEED floor from 3 to 5.
```

Invaluable when debugging a LoCoMo category miss or a real-user bug report.

## See also

- [3.5 Retrieval pipeline](../03-core-subsystems/3.5-retrieval-pipeline.md)
- [Chambers](chambers.md) — smaller-scale isolated tests
- [Row 49 — Competitor benchmark harness](../05-features/row-49-competitor-bench.md)
