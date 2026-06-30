# 35 — Production build: compile `.said` to run natively fast

How to compile the `.said` coding binaries so recall is **native-fast**, and why each lever matters. The
short version: **release + fat LTO + `target-cpu=native` + `simd` feature + the RESIDENT process model.**

```bash
bash scripts/build-production.sh
# == said-cli + said-mcp, profile=production, features=coding, RUSTFLAGS="-C target-cpu=native"
# -> target/production/said.exe + said-mcp.exe
```

Fingerprints are **bit-identical** to a normal build — this is purely a speed compile (same float ops,
better machine code).

## The four speed levers

| Lever | What it does | Where |
|---|---|---|
| **`profile = production`** | release + **fat LTO** (whole-program inlining across ALL crates — the encoder mean-pool + the recall fusion get inlined + vectorized across crate boundaries) + `panic=abort` (no unwind tables) + `strip`. | `Cargo.toml [profile.production]` |
| **`target-cpu=native`** | the SCA fingerprint Hamming popcount + the encoder's mean-pool loop compile to the **host's exact AVX2 / AVX-512** instructions instead of the generic baseline. | `RUSTFLAGS` (the build script) |
| **`simd` feature** (in `coding`) | pulls in **simsimd's runtime-dispatched** (AVX2 / AVX-512 auto-detected) binary-Hamming core for the top-50 fingerprint scan — the documented **3–4× speedup** on the retrieval hot path. Scalar `count_ones()` fallback remains for targets without it. | `--features coding` |
| **RESIDENT process** | load the 16 MB encoder + the indexes **ONCE**, serve many queries warm. The per-CLI-process spawn re-pays load every query; the resident MCP / `said serve` does not. | `said-mcp` server, `said serve` |

## Why DEBUG is the wrong thing to measure (the trap)

A debug build is ~**7.5× slower** than release for the encoder math and Hamming scan. Measuring recall on
`target/debug` (and worse, spawning a fresh CLI process per query on a cold file cache) is what produced
the bogus "3.9 s recall" earlier. The production binary on the resident path is the real number:

| | debug, per-CLI-process, cold | **production, resident MCP, warm** |
|---|---|---|
| recall on a 5,340-frame brain | ~3,900 ms | **~100 ms** |

Always benchmark the **production binary on the resident path**.

## SIMS — the symbol index (instant code lookup)

"SIMS" = the **`SYMS` section** ([3.6](03-core-subsystems/3.6-trigram-symbol-index.md)): the exact
symbol-name index. Built at `init`/compact, **serialized into the `.said` file**, mmap'd at read time. A
`said sym <name>` is a pure HashMap lookup — **sub-millisecond**, independent of corpus size. This is why
CODE recall is exact + instant: it does not go through the fingerprint scan at all (the PureLexical route,
[3.1](03-core-subsystems/3.1-sca-engine.md)). Together with the `TRGM` trigram index (also serialized),
the lexical paths are O(lookup), not O(corpus).

> **Optimization still open:** the `_fast` BM25 *word* index (entity matching) is NOT serialized
> (`INIT-ROUTE-TRACE.md`) — so the FIRST query in a fresh process rebuilds it. `SYMS`/`TRGM` ARE
> serialized; serializing the word index too would make even cold query-1 fast. Tracked.

## The resident model — the deployment that's actually fast

`.said` is designed to run as a **resident process that loads once**:

- **MCP server (`said-mcp`)** — the production deployment for coding agents. Loads the encoder + brain once
  at startup, then every `ask`/`search`/`learn_fix`/`remember` reuses the warm state. ~100 ms warm recall.
- **`said serve`** (resident CLI, [07-cli-reference](07-cli-reference/)) — load once, read queries from
  stdin, answer each warm. For hot loops / benchmarking / an agent that asks many questions without MCP.
- **`said ask` (one-shot CLI)** — fine for a single command; re-loads per invocation, so do NOT use it in
  a hot loop (that re-pays the ~140 ms load + word-index rebuild every time).

**Rule:** an agent integration (or any benchmark) uses the **MCP server or `said serve`**, never repeated
`said ask` spawns.

## Verify your production build

```bash
# warm recall on the resident path should be ~100ms (release), not seconds:
node hard-eval/beat-them/ultimate-stretch.js          # uses the binaries; see ultimate-stretch-result.txt
# or drive the MCP server directly (loads once, then fast) -- see hard-eval/beat-them/phase2-mcp-update-live.js
```

## See also

- [3.1 SCA engine](03-core-subsystems/3.1-sca-engine.md) — fingerprint Hamming (what SIMD accelerates).
- [3.2 static encoder](03-core-subsystems/3.2-static-encoder.md) — the 128-dim 4M encoder loaded once.
- [3.6 trigram + symbol index](03-core-subsystems/3.6-trigram-symbol-index.md) — SYMS/TRGM, serialized + mmap'd.
- [09 cargo features](09-cargo-features.md) — `simd`, `embed-model`, `coding`.
- [34 ultimate stretch](34-ultimate-stretch.md) — the corrected speed measurement (resident, release).
