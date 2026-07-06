# Temporal + recall hardening — session record (2026-07-06)

A single overview of the free-brain (Personal tier) recall/temporal work done this session, so the
thread isn't lost. Each item links to the durable doc/code that carries the detail. Read this first;
follow the links for depth. Pattern mirrors [24-memory-benchmark-session-record.md](24-memory-benchmark-session-record.md).

## The arc (what we set out to do)
Prove `.said` is a world-class portable memory for the **Personal (free) tier** and close its temporal
gap — measured honestly on BOTH the CLI and the live MCP surface, at 1000 records. Three correctness
fixes plus one new feature landed; one real bug was surfaced by the benchmark and root-caused.

## 1. Three recall-correctness fixes (all in shared `sca_core`, so CLI + MCP benefit identically)
See [FIXES-LOG.md](FIXES-LOG.md) #9–#11 for the full symptom → root-cause → fix → verification.
- **#9 Determinism** (`c0ba138`) — `ask` ranking sorted by confidence with no tie-break over a HashMap,
  so ties resolved by per-process-random order (same query → different memory each run, recall@1
  ~20–33% unstable). Fixed with a `doc_id` tie-break on every ranking sort. Guard:
  `test_recall_determinism.rs`.
- **#10 OKF single-word entities** (`c0ba138`) — personal memories now link on single-word proper nouns
  ("Rotterdam", "Carol") so the Engine-D concept bridge fires, but ONLY for mid-sentence names (a
  positional guard) — the first "any capitalised word" version turned sentence-openers into spurious
  hubs and REGRESSED recall (multi-hop r@5 1.00→0.50). See [3.9-graph-layer.md](03-core-subsystems/3.9-graph-layer.md).
  Guard: `test_recall_quality_volume.rs` (10/10 categories at 400).
- CLI `add` id-collision (`c0ba138`) — auto doc_id was `doc_{unix_seconds}`; two adds in the same second
  overwrote each other (5 rapid adds → 1 memory). Now `mem_{counter}`.

## 2. Temporal grounding — new feature, research-grounded
See [FIXES-LOG.md](FIXES-LOG.md) #11 and [11-known-limitations.md §1.1](11-known-limitations.md).
- **Research** — verified the leaders in the LOCAL Mem0 source (`G:\development\SAID-ECHO\research\mem0`,
  not just docs). The proven win (Mem0 `configs/prompts.py`; LoCoMo 86→90, LongMemEval 90→95) is
  WRITE-TIME grounding — resolve relative phrases to absolute dates in the stored text — NOT a query-side
  resolver. Zep/Graphiti's bi-temporal graph is the enterprise version, overkill here.
- **Built** (`bd3503b`) — `time_compat::ground_relative_dates`, deterministic/no-LLM, `today` passed in.
  "Last year I…" → "…(around 2025)"; last year / this year / last quarter / last month with year-boundary
  wrapping; additive + idempotent. Applied at the single write chokepoint `remember_with_salience` (MCP)
  and the CLI `add` layer. 9/9 unit tests (`test_temporal_grounding.rs`); no recall regression.

## 3. 1000-record CLI-vs-MCP benchmark (temporal + personal memory)
Harness: `hard-eval/mcp-harness/bench-1000.js` (scratch). 42 gold (personal-fact, preference, temporal
relative, temporal absolute) + 958 filler = 1000 memories, identical through both surfaces.

| Surface | Memories | Avg write | Avg ask | Deterministic |
|---|---|---|---|---|
| CLI | 1000 | ~75ms | ~72ms | YES |
| MCP | 1000 | ~28ms | **~9ms** | YES |

Recall (CLI = the VALID temporal measurement; see the bug below): preference **100%**, personal-fact
70–85%, temporal-rel 50% @1 / 75% @10, temporal-abs 63% @1 / 88% @10. Temporal @1 is depressed by the
"what did I do last X" phrasing-tie (many temporal memories share that stem → tie at @1, recover at
@5/@10); an agent reading the top-K resolves it. No slowdown at 1000 records (ask stays <10ms on MCP).

## 4. Bug surfaced + root-caused (honest)
See [FIXES-LOG.md](FIXES-LOG.md) #12. The benchmark caught that MCP temporal grounding does NOT persist
to disk in a large single-session save-per-write batch (CLI is immune; the feature is correct in every
isolated test and live single writes). Root-caused via an after-save probe to the block-compaction save
path (same family as #8) — the intervening read in the probe itself masked it (Heisenbug). Deferred as a
focused fix (hot serialization path; needs a guard test first, as #8 taught). MCP temporal is trustworthy
for interactive single writes (the real usage); the failure is specific to a scripted 1000-write session.

## Durable artifacts
- Fixes: [FIXES-LOG.md](FIXES-LOG.md) #9–#12.
- Limitation status: [11-known-limitations.md §1.1](11-known-limitations.md) (temporal now PARTIALLY SHIPPED).
- Code: `crates/sca-core/src/{ask.rs, said_file.rs, time_compat.rs}`, `crates/said-mcp/src/handler.rs`,
  `crates/said-cli/src/main.rs`.
- Tests: `test_recall_determinism.rs`, `test_recall_quality_volume.rs`, `test_temporal_grounding.rs`.
