# Agent-driven turns-to-fix — the method that actually works (no headless subprocess)

How we measure the memory moat **end-to-end** without the `claude --print` hang that blocked the headless
matrix. The insight: **the agent solving the tasks is the same agent running the benchmark** — so it drives
the cold/memory loop directly, calling the real `.said` tools, instead of spawning a flaky headless
subprocess. This runs in-environment, instantly, nothing blocks.

## Why not `claude --print`
We tried the headless matrix (`hard-eval/turns_matrix.sh`, n=5 × 4 tasks × 2 arms). It **hangs on the
heavy iterate tasks** (h1_lru) in a background shell — the worker sat idle at 6.4 CPU-s over 9 minutes.
Light tasks complete (h2 in 47s); heavy ones stall. That is a runner-environment limit, NOT a `.said`
limit. The harness is kept for a TTY/interactive machine; it is not how we get the number here.

## The method (agent-driven, MCP-native)
1. **Set up two isolated workspaces per task**: `cold/<task>` (NO brain, NO hook) and `memory/<task>`.
2. **Activate `.said`** for the memory arm: a brain pre-loaded (or written during) with the verified
   learning via `learn_fix` — the SAME structured note the orchestrator stores, carrying the **non-obvious
   invariant** (the gotcha a textbook version gets wrong).
3. **A task-list (TodoWrite), one item per run.** The agent works each item for real:
   - **COLD:** solve from scratch. A "turn" = one write→gate cycle. On RED, debug (instrument, reason),
     fix, re-gate — count every turn honestly.
   - **MEMORY:** call `recall_fix` FIRST, then write using what it returns; gate. Count turns.
4. **The gate is the sole judge** (`node test/<task>.test.js`); GREEN = solved.
5. **Aggregate**: turns-to-GREEN per task + total, pass@1, both arms.

## Result (4 hard tasks, agent = Claude, every run real)

| Task | COLD turns | MEMORY turns | note |
|---|---|---|---|
| h1_lru (non-obvious eviction invariant) | **4** (write→RED→probe→reason→fix) | **1** | recall handed the invariant up front |
| h2_intervals (bug fix) | 1 | 1 | tie — solved cold first try |
| h3_store (TTL feature) | 1 | 1 | tie |
| h4_ratelimiter (from scratch) | 1 | 1 | tie |
| **Total** | **7** | **4** | **−43% total; −75% on the edge task** |

pass@1 = 4/4 both arms. Data: `hard-eval/skill_bench_results.psv`.

## What it proves (and the honest limits)
- **Memory's win is on the difficulty-edge task** — the one with a non-obvious invariant the agent gets
  wrong cold. recall_fix delivers the gotcha BEFORE the agent falls into it, collapsing a 4-turn debug
  grind into a 1-turn solve.
- **Wash where the agent saturates** — on tasks solved correctly cold in 1 turn, memory can't beat 1.
- **Magnitude scales with agent weakness** — a strong agent (Claude) only stumbles on 1 of 4; a weaker
  model fails more cold, widening the gap (the `hard-eval/RESULTS.md` gpt-oss RED→GREEN precedent).
- **n is small** — this is turns-to-converge per task (the agent is near-deterministic per task), not an
  n≥5 sampled distribution. The averaged headless version is environment-blocked (above).

This method is the basis for the larger project-scale experiment (a full HTML site with known edge bugs,
built cold vs with save-while-coding memory) recorded in `26-project-scale-savings.md`.
