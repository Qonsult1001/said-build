# Row 49 — Competitor benchmark harness

**Status:** ✅ shipped 2026-04-22. Harness shell + live SAID-ECHO numbers. External-stack population tracked in [Row 39 spec](row-39-competitor-sweep-spec.md).

## What it does

Rust example that writes `docs/competitor_benchmark.json` with SAID-ECHO's live MTEB + LoCoMo numbers and placeholder rows for every significant competitor (mem0 OSS + graph, Zep, LangMem, Letta, memvid, pgvector, ChromaDB, HippoRAG, LightRAG, Cognee). Prints a human-readable matrix alongside.

## Where it lives

[`crates/sca-core/examples/competitor_bench.rs`](../../../crates/sca-core/examples/competitor_bench.rs)

Registered in `Cargo.toml`:

```toml
[[example]]
name = "competitor_bench"
required-features = ["static-embed"]
```

## Inputs

```
cargo run --release -p sca-core --example competitor_bench --features "static-embed" -- \
    [--out <path>] [--mteb-json <path>] [--locomo-f1 <f>]
```

- `--out <path>` — JSON output, default `docs/competitor_benchmark.json`
- `--mteb-json <path>` — optional live MTEB results (one JSON line per task); if absent, uses committed reference numbers
- `--locomo-f1 <f>` — override LoCoMo F1 number (default 0.8555 — last verified run)

## Outputs

### `docs/competitor_benchmark.json` schema

```json
{
  "generated_at": 1713840000,
  "benchmarks": {
    "locomo_f1": "F1 on LoCoMo conv-26 (20 QAs, Claude Opus 4.7 reader)",
    "mteb_needle": "MTEB LEMBNeedleRetrieval NDCG@10",
    "mteb_wikimqa": "MTEB LEMBWikimQARetrieval NDCG@10",
    "mteb_summscreenfd": "MTEB LEMBSummScreenFDRetrieval NDCG@10",
    "mteb_qmsum": "MTEB LEMBQMSumRetrieval NDCG@10"
  },
  "rows": [
    {
      "name": "SAID-ECHO",
      "implementation": "Rust — 1-bit SCA fingerprints + BM25 + graph fan-out",
      "offline": true,
      "ingest_calls_llm": false,
      "locomo_f1": 0.8555,
      "mteb_needle": 1.0,
      "mteb_wikimqa": 1.0,
      "mteb_summscreenfd": 0.98,
      "mteb_qmsum": 0.89,
      "notes": "Single-file portable brain; LLM optional at caller-side read time only."
    },
    {
      "name": "mem0 (OSS)",
      "locomo_f1": 0.669,
      "notes": "Published LoCoMo F1 (no-graph): 0.669. Requires OpenAI API at ingest.",
      ...
    },
    ...
  ]
}
```

### Stdout summary
```
Competitor benchmark matrix written to docs/competitor_benchmark.json

system                  LoCoMo F1     Needle      WikimQA    SummSFD    QMSum
────────────────────────────────────────────────────────────────────────────────
SAID-ECHO                   0.856      1.000       1.000      0.980    0.890
mem0 (OSS)                  0.669          —           —          —        —
mem0 (with-graph)           0.684          —           —          —        —
Zep                             —          —           —          —        —
...
```

## How to test

```
cargo run --release -p sca-core --example competitor_bench --features "static-embed"
```

Check `docs/competitor_benchmark.json` exists and contains SAID-ECHO row with `locomo_f1 = 0.856`, `mteb_needle = 1.0`, `mteb_wikimqa = 1.0`.

Verified 2026-04-22 — output matrix as expected.

## How to extend

### Add a new competitor row
Append to `placeholder_rows()` in `competitor_bench.rs` with the same `SystemRow` shape. `null`/`None` for unknown scores; `notes` column tells readers what's measured and what isn't.

### Fill in a competitor row with real numbers
Run the competitor against LoCoMo + MTEB datasets, record the JSON, feed back into this harness. The shape is intentionally flat so fills are copy-paste-able.

### Add a new benchmark dimension
Add a field to `SystemRow`, update the JSON schema docs at the top of the example, update the printed matrix header.

## Known limitations

### External stacks unpopulated
Rows for Zep, LangMem, Letta, memvid, pgvector, ChromaDB, HippoRAG, LightRAG, Cognee are all `null`. Populating requires running each stack against the same datasets — ops task, not code. Tracked in [Row 39](row-39-competitor-sweep-spec.md).

### Published numbers hand-encoded
Mem0's `0.669` / `0.684` are taken from their paper, not reproduced locally. Reproducing them would require running mem0's OSS pipeline end-to-end with the same benchmark data. Noted as a quarterly refresh item.

### `--mteb-json` consumption format undocumented outside source
The flag reads JSON lines with `task` + `ndcg_at_10` fields. Format contract lives in the example source, not in this doc. Stabilize when the CI bench job ships.

## See also

- [Row 39 Competitor sweep (spec)](row-39-competitor-sweep-spec.md)
- [10 Benchmarks we run](../10-benchmarks/README.md)
- [`docs/competitor_benchmark.json`](../../../docs/competitor_benchmark.json) — current matrix
