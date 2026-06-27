# Benchmark methodology — how to measure whether memory makes the agent better

The research-correct protocol for proving `.said`'s memory helps a coding agent. **Single-run pass@1 is
the WRONG primary metric** (we used it through docs/20 v1–v7 and it produced noisy, misleading per-task
numbers). This doc records the methodology — grounded in the agent-memory literature — so future
benchmarks measure the right things.

## Why single-run pass@1 is wrong

- **High variance, hides capability.** pass@1 is one sample; Codex (arXiv:2107.03374) scored 28.8% at
  pass@1 but 70.2% at pass@100. A single number can't tell "can't do it" from "didn't this time."
- **It ignores the real question.** When the baseline *fails*, it usually solves the task *eventually* —
  the honest comparison is **how many turns/attempts to converge**, with vs without memory, not did-it-
  succeed-once.
- **It mis-scores abstention.** An agent that correctly gives up on an unanswerable task is doing the
  RIGHT thing, but pass@1 scores it as a failure.

## The three axes (report all, separately)

### 1. Eventual success — pass@k  [arXiv:2107.03374]
Generate **n ≥ 5** samples/task/arm; a task is solved@k if any of k pass. Unbiased estimator:
`pass@k = 1 − C(n−c, k) / C(n, k)` (c = #passing of n). pass@1 = c/n. Report pass@1/5 (and pass@k gap on
the hard stratum — a gap that persists at high k = memory solves what baseline can't). Optional **pass^k**
(arXiv:2406.12045) for reliability (all k succeed), not just luck.

### 2. Convergence cost on CO-SOLVED tasks — turns-to-first-success / Cost-of-Pass  [arXiv:2504.13359]
On the subset **both arms eventually solve**, compare **turns-to-first-success** and
**Cost-of-Pass = C/R** (expected attempts to first success = 1/R). This is the "baseline eventually
solves it too — count the turns" comparison. Reflexion (arXiv:2303.11366) is the precedent: success as a
**learning curve over trials**, not a single shot.

### 3. Abstention correctness — give up vs grind  [arXiv:2207.05221, 2006.09462]
An agent should **abstain when P(success)×value < cost of continuing.** Early abstention is
**Pareto-improving** (arXiv:2502.09054: +4.1% abstention bought −13% cost AND −5% error). Score:
- a **clean give-up on a genuinely unanswerable task = CORRECT** (abstain-correct),
- **grind-then-wrong = the WORST outcome** (max cost, negative value),
- report **risk–coverage / AURC** and **abstention F1**.
Caution (arXiv:2506.09038): reasoning-tuned models abstain ~24% worse — use an explicit gate
(turn-budget + "say you don't have it and stop"), don't trust the model to self-limit.

## Task partitioning (the key move)
Don't score every task as one pool. Partition by outcome across the n samples:
- **memory-only-solves** (baseline 0 at pass@k, memory > 0) → an **accuracy** win.
- **co-solved** (both solve) → compare **turns-to-first-success** → a **convergence** win.
- **neither** → **abstention candidates**: the right outcome is a clean give-up, not grinding.
Report the three buckets separately. (Memory's accuracy gains concentrate at the **difficulty edge / under
transfer** — Voyager 2305.16291, AWM 2409.07429, ReasoningBank 2509.25140; on easy tasks the base agent
saturates and memory buys only cost.)

## Sample sizes
- **n ≥ 5** samples/task/arm (so pass@1/5 are estimable); the field uses up to n=200 for stable pass@k.
- **≥ 3 seeds**; report distributions, not single runs (single runs are noise — our own repeated lesson).
- Stratify tasks by difficulty so the edge stratum (where memory should move accuracy) is visible.

## Does memory help ACCURACY or just COST?
Both, but **where the task sits relative to the agent's capability decides which**:
- **Accuracy** (solves what baseline never does): at the **hard/edge tier or under shift**, and from
  learning from **failures** (Reflexion, ExpeL 2308.10144, AWM, ReasoningBank).
- **Cost only** (same solves, cheaper): on **easy in-distribution** tasks the base agent saturates;
  memory just reduces tokens/turns (MemGPT, Mem0).
Report them separately; expect accuracy gains at the edge, cost gains everywhere.

## Our harness
`scratchpad/bench_research.sh`: 2 arms × tasks × n=5 samples, with a verify-bodies gate (#8 guard) before
running, an explicit abstention probe (a question answerable from NO file/fix — correct iff the agent
gives up cleanly), and the prompt suffix that invites a clean give-up (the turn-budget proxy in a single
`--print` run). Outputs per-sample turns/cost/correct/abstained → pass@k + co-solved turns + abstention F1.

**Citations:** pass@k 2107.03374 · pass^k 2406.12045 · Reflexion 2303.11366 · Cost-of-Pass 2504.13359 ·
P(IK) 2207.05221 · selective QA 2006.09462 · AbstentionBench 2506.09038 · early-abstention 2502.09054 ·
Voyager 2305.16291 · AWM 2409.07429 · ReasoningBank 2509.25140 · ExpeL 2308.10144.
