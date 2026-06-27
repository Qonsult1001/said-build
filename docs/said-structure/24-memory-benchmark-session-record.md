# Memory-benchmark hardening + SAVE-axis proof — session record (2026-06-27)

A single overview of what was done in the benchmark/memory-correctness work, so the thread isn't lost.
Each item links to the durable doc/code that carries the detail. Read this first; follow the links for depth.

## The arc (what we set out to do)
Prove `.said`'s memory helps a coding agent, **correctly measured** — and make sure the code actually
implements the research methodology, the injection follows nudge, and the agent can SAVE memory itself.
Three explicit gates the owner set: (1) document the A1–A5 lessons, (2) test every step of the
methodology individually, (3) only then run a combined end-to-end test.

## 1. Methodology, documented + lessons captured
- **[23-benchmark-methodology.md](23-benchmark-methodology.md)** — the research-correct protocol: pass@k
  [2107.03374], turns-to-first-success on co-solved [2504.13359], abstention-as-correct [2207.05221,
  2006.09462, 2502.09054], 3-bucket task partitioning. Single-run pass@1 is the WRONG primary metric.
- **[20-learning-benchmark.md](20-learning-benchmark.md)** — v1–v7 history PLUS the new **LESSONS L1–L9**
  (the design contract): non-file gold (L1), init-floor both arms (L2), structured `learn_fix` not a lazy
  label (L3), lenient/LLM oracle (L4), A*→axis partition (L5), per-task abstention (L6), failed-run≠wrong
  (L7), n≥5 (L8), and **L9: HOW memory is injected is the lever — the nudge pattern** (ties the benchmark
  to [22-memory-injection-nudge-pattern.md](22-memory-injection-nudge-pattern.md)).

## 2. verify-docs on the methodology — code now implements every step
Ran `/verify-docs` on docs/23: traced every claim to code and executed the runnable ones. Result was
**8 verified · 3 orphaned · 1 broken · 1 drift**; after a grill to set intent (all decisions grounded in
the existing 10-benchmarks + BYO-LLM methodology), fixed the correct side:
- **#11 BROKEN → FIXED:** abstention was keyed on a dead task id (`QABSTAIN`); now scored **generically by
  the `__ABSTAIN__` gold marker** (harness emits an `abstain_gold` column; aggregator reads it). Supports
  N probes — required for F1/AURC over a set.
- **#8 ORPHANED → BUILT:** **abstention F1** (TP/FP/FN) in `bench_aggregate.sh`.
- **#10 ORPHANED → BUILT:** **LLM-graded oracle** (`SAID_LLM_GRADE=1`) — a BLIND, external grader reusing
  the project's existing oracle pattern (LoCoMo / `competitor_bench` Opus judge), BYO-LLM-honoring. Regex
  is the deterministic default.
- **#9 ORPHANED → DEFERRED (doc):** **risk-coverage/AURC** needs a calibrated per-sample confidence that
  `claude --print` doesn't expose; marked DEFERRED honestly rather than faking a proxy curve.
- **#12 DRIFT → DOC FIXED + n≥5 ENFORCED:** dropped "≥3 seeds" (`.said` retrieval is DETERMINISTIC per
  10-benchmarks; the LLM is external/unseeded per BYO-LLM — no RNG to vary); harness now **enforces n≥5**
  (SMOKE=1 escape) and reports **across-sample mean ± SD** as the stability metric.
- Offline validation (`gate2_validate.sh`): **25/25** every docs/23 step.

## 3. Injection confirmed nudge-correct (grounded)
`sca-core::steering::render_verified_fixes` uses the exact nudge pattern: plain-facts + solve-first lead
("Found prior work that may apply. Read this before repeating old debugging work…"), `<project_memory>`
block, **top-K=3**, UserPromptSubmit-trusted channel (PreToolUse for block/redirect only). Matches
[22-memory-injection-nudge-pattern.md](22-memory-injection-nudge-pattern.md) + L9. No change needed.

## 4. GATE 3 — A1–A5 on the MCP/CLI surface (n=5, LLM-graded)
`hard-eval/bench_research.sh` runs the real product surface (CLI `learn-fix`/`recall-fix` + `said setup`
hook + `claude --print`). Verdict (honest): memory pass@1 1.00 vs baseline 0.96; convergence
variance-dominated (A1 −18%, A3 −10%; A2 +43% outlier); abstention F1 1.00 both arms; ~9% pricier
aggregate. **The base agent SATURATES on this easy set (baseline 96%)** → memory has no accuracy headroom
here. This is exactly docs/23's prediction: memory's accuracy win is at the **difficulty EDGE**. The
methodology + surface are proven; the easy task set is the limiter, not the memory.

## 5. Save template aligned across ALL THREE surfaces (the key product fix)
The agent's `learn_fix` save guidance now matches the orchestration **`ITERATION_TEMPLATE`** that works
(problem / files / errors+corrections / **non-obvious INVARIANT** / key result + change-set; only after a
green gate). Applied to:
- **Orchestrator** (`said-orchestration::steps/learn.rs`) — the gold reference (unchanged).
- **CLI SKILL + `said setup` hook** (`said-prompts::steering` MCP_INSTRUCTIONS + SKILL_BODY).
- **Live MCP server** (`said-mcp::main.rs` constitution) — **this had NO `learn_fix` guidance at all
  before** (only `remember`); added a CODING MEMORY section. (Finding: the live MCP instructions are built
  in main.rs, independent of said-prompts.)
All three write through the ONE shared `sca_core::ask::learn_coding_fix`. Rebuilt `said-coding.exe` +
`said-mcp.exe` (coding bundle); guidance verified embedded; **regression 15/15 green**.

## 6. SAVE axis PROVEN (the agent writes correct, recallable memory itself)
`hard-eval/save_axis.sh`: a `claude --print` agent given 3 solvable JS tasks **solved 3/3, self-saved 3/3**
via the aligned `learn-fix` surface (intact bodies 371–447 ch, real invariants captured), **recalled 3/3**
next-session by PARAPHRASE (0.61–0.83, above the 0.45 floor), 3 distinct doc_ids, zero cross-contamination.
Token-savings sub-axis = already covered by GATE 3 (recall→reuse). Memory: `save-axis-proven`.

## Artifacts (durable)
- Harnesses: `hard-eval/bench_research.sh`, `hard-eval/bench_aggregate.sh`, `hard-eval/save_axis.sh`.
- Docs: this file, 20, 22, 23 (+ CLAIMS-COVERAGE for the live-proven claims).
- The orchestrator moat path was tried and **dropped** (it's the example loop; the product surface is
  MCP/CLI). The verified golden LRU lives at `hard-eval/results/h1_lru.verified.js` if an edge-tier moat
  run is wanted later (needs a brain seeded with that fix + a WEAK external model — the saturation finding).

## Open / next (honest)
- **Edge-tier accuracy proof:** the only way to show a memory ACCURACY win is harder tasks the baseline
  fails (docs/23 "stratify to the edge"). A1–A5 is below the agent's edge.
- AURC stays deferred until a calibrated confidence signal exists.
- Token-savings as its own n≥5 axis (currently folded into GATE 3).
