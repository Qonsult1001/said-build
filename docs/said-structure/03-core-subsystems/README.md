# Core subsystems

The ten engines inside `.said`. Each has its own section; together they produce the brain.

## Contents

- [3.1 SCA engine](3.1-sca-engine.md) — 1-bit fingerprints, Hamming distance, holographic 16-view
- [3.2 Static encoder](3.2-static-encoder.md) — `said-lam-static`, 4.8 MB, 64-dim Matryoshka-truncated static embedding
- [3.3 Brain](3.3-brain.md) — S_slow tensor, recall-weight, dream cycles
- [3.4 FrameStore](3.4-framestore.md) — compression, blocks, dedup, lineage, tombstones
- [3.5 Retrieval pipeline](3.5-retrieval-pipeline.md) — `recall_fused`, BM25, graph fan-out, ask fusion
- [3.6 Trigram + symbol index](3.6-trigram-symbol-index.md) — grep accelerator + exact symbol lookup
- [3.7 Audit log](3.7-audit-log.md) — append-only BLAKE3-chained log of mutations
- [3.8 Latent space](3.8-latent-space.md) — the math + measured speed that ties 3.1–3.3 together
- [3.9 Graph layer](3.9-graph-layer.md) — retrieval-time entity graph, Layer-6 bridge walk, how it differs from Cognee/Zep KGs
- [3.10 Prompt architecture](3.10-prompt-architecture.md) — `said-prompts` crate, Anthropic-aligned principles, single source of truth across WASM / MCP / native

## How they fit together

```
                  ┌──────────── said ask / MCP ask / MCP search ─────────┐
                  │                                                      │
                  ▼                                                      │
          ┌───────────────┐                                              │
          │ 3.5 retrieval │ ◄── 3.1 SCA ◄── 3.2 static encoder            │
          │   pipeline    │                                              │
          │ recall_fused  │ ◄── 3.6 trigram + symbol index                │
          │     BM25      │                                              │
          │  graph fan-out│ ◄── 3.3 brain (recall-weight boost)           │
          │   ask fusion  │                                              │
          └───────┬───────┘                                              │
                  │ top-k results                                        │
                  └──────────────────────────────────────────────────────┘

                  ┌──────────── said remember / MCP remember / admin ────┐
                  │                                                      │
                  ▼                                                      │
          ┌───────────────┐                                              │
          │ 3.4 FrameStore│ ◄── 3.2 encoder (fingerprint for SCRM)        │
          │ put_with      │ ◄── 3.1 SCA (fingerprint into SCRM)           │
          │ put_with_pillar                                              │
          │ admin_*       │                                              │
          └───────┬───────┘                                              │
                  │                                                      │
                  ▼                                                      │
          ┌───────────────┐                                              │
          │ 3.7 audit log │ ◄── every mutating path auto-appends         │
          └───────┬───────┘                                              │
                  │                                                      │
                  ▼ brain.dream (corpus-mean drift)                      │
          ┌───────────────┐                                              │
          │ 3.3 brain     │                                              │
          │ s_slow accum  │                                              │
          │ recall decay  │                                              │
          └───────────────┘
```

## Where the code lives

- [`crates/sca-core/src/crystalline.rs`](../../../crates/sca-core/src/crystalline.rs) — SCA 1-bit core (~4700 lines)
- [`crates/sca-core/src/engine.rs`](../../../crates/sca-core/src/engine.rs) — `ScaEngine` wrapper with encoder + brain
- [`crates/sca-core/src/brain.rs`](../../../crates/sca-core/src/brain.rs) — brain state
- [`crates/sca-core/src/frames.rs`](../../../crates/sca-core/src/frames.rs) — `FrameStore`
- [`crates/sca-core/src/said_file.rs`](../../../crates/sca-core/src/said_file.rs) — `SaidFile` aggregating everything
- [`crates/sca-core/src/recall.rs`](../../../crates/sca-core/src/recall.rs) — unified retrieval entry point
- [`crates/sca-core/src/trigram_index.rs`](../../../crates/sca-core/src/trigram_index.rs)
- [`crates/sca-core/src/symbol_index.rs`](../../../crates/sca-core/src/symbol_index.rs)
- [`crates/sca-core/src/audit.rs`](../../../crates/sca-core/src/audit.rs)
- [`crates/sca-core/src/latent_cluster.rs`](../../../crates/sca-core/src/latent_cluster.rs) — content-addressable dedup
- [`crates/said-prompts/`](../../../crates/said-prompts/) — agent prompt source of truth (consumed by WASM browser agent + MCP server + native callers)
