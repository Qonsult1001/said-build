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
- report **abstention F1** (computed: `bench_aggregate.sh` scores TP/FP/FN over every abstain-gold row —
  the generic `__ABSTAIN__` marker, not a hardcoded task, so it works for N probes).
- **risk–coverage / AURC: DEFERRED.** AURC needs a calibrated per-sample confidence to sweep coverage;
  a single `claude --print` run exposes none (no logprobs, no seed/temperature knob). We do NOT fake it
  with a proxy (turns / recall-fired) — that would be a misleading curve. Revisit if a real confidence
  signal is added (model self-rating, or in-process scoring).
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

## Sample sizes (and why there is NO seed loop here)

- **n ≥ 5** samples/task/arm (so pass@1/5 are estimable); the field uses up to n=200 for stable pass@k.
  `bench_research.sh` **enforces** n≥5 for a real run (refuses lower unless `SMOKE=1`).
- **NO RNG "seed" sweep — by design, grounded in this project's methodology.** The generation-eval
  literature varies a random seed to expose model-sampling instability ([arXiv:2504.07086],
  [arXiv:2512.12066]). That does not transfer here: (a) **`.said` retrieval is DETERMINISTIC** — the
  [10-benchmarks](10-benchmarks/) harnesses diff EXACT scores across runs and treat any divergence as a
  *bug* (`realworld-probes.md`: "Divergent results = a non-deterministic ranking"); there is nothing to
  seed. (b) **The LLM is external and unseeded** — the BYO-LLM contract keeps every LLM call OUTSIDE the
  `.said` process (`10-benchmarks/README.md`, `13-integrations.md`), and `claude --print` exposes no
  seed/temperature knob. So the only variance in an agent A/B is the external model's run-to-run
  nondeterminism, which **the n≥5 unseeded samples already capture.** `bench_aggregate.sh` reports the
  **across-sample mean ± SD** as the stability metric — the honest analogue of "report distributions,
  not single runs," matching MTEB's run-to-run variance *budget* (`10-benchmarks/mteb.md`) rather than a
  seed average. A separate seed loop would vary confounds (brain rebuild, task order), not a seed.
- Stratify tasks by difficulty so the edge stratum (where memory should move accuracy) is visible.

## The correctness oracle (regex default + opt-in LLM grader)

The gold tokens are **lenient substrings** of the verified learning (a paraphrase still matches). But a
strict regex **undercounts** — every docs/20 version saw it score 1–2/5 when both arms answered ~4/5
(lesson L4). So `bench_research.sh` adds an **opt-in LLM-graded oracle** (`SAID_LLM_GRADE=1`): a
**separate, blind** `claude --print` grader (no arm label, anti-bias) that judges whether the answer
conveys the gold fact. This is **not a new mechanism** — it reuses the project's existing external-oracle
pattern (the LoCoMo / `competitor_bench` Opus-4.7 / GPT-4o-mini F1 judge, `10-benchmarks/README.md:38`,
`locomo.md:49`), and it honors **BYO-LLM** (the grader runs outside `.said`). Regex is the deterministic
default for cheap smokes; the LLM oracle is reserved for the trustworthy verdict run.

## What a WORLD-CLASS memory benchmark must cover (beyond fix-recall)

The three axes above measure whether a recalled coding fix helps an agent. But a *world-class memory* —
the bar set by LongMemEval, LoCoMo, Mem0, Letta/MemGPT, ReasoningBank — is judged on **memory-specific
abilities** that a single fix-recall A/B does not exercise. To claim world-class, the benchmark suite must
also cover these (each maps to a capability `.said` already has a test or harness for — cross-linked):

| Memory ability | What it tests | Where `.said` proves it |
|---|---|---|
| **Save fidelity** | the agent itself stores correct, recallable memory (not just seeded fixtures) | SAVE axis — `hard-eval/save_axis.sh` (proven 3/3); [24-…session-record](24-memory-benchmark-session-record.md) §6 |
| **Recall@k under scale** | the right memory returns at top-k as the store grows to 1000s | `hard-eval/recall-scale.sh` (recall@5 = 100% at N=1000); CLAIMS-COVERAGE |
| **Paraphrase / semantic recall** | recall by MEANING, not stored wording | `test_recall_quality_volume.rs` (10 categories); save_axis recalls by paraphrase @0.61–0.83 |
| **Adversarial precision** | the near-twin is NOT returned (LRU vs LFU; office 7 vs 17) | `test_twin_precision_volume.rs`; `recall-measure.sh` (7/7 + 5/5) |
| **Temporal / latest-wins** | a superseded memory yields the NEW version; in-content dates | `test_recall_quality_volume.rs` (Update/Temporal buckets) |
| **Multi-session accumulation** | session N reuses what N−1 concluded and never wrote to a file | docs/20 corrected run (non-file knowledge) — the ONLY design where memory can win |
| **Abstention / existence** | "do I have anything on X?" → clean no when absent | abstention axis (this doc, axis 3) + `test_recall_edge_cases.rs` |
| **Language / scope isolation** | a Python query never gets a C# fix; scope-filtered recall | `language-isolation-guarantee` (memory); `SAID_RECALL_LANG` |
| **No pollution under write-back** | auto-storing every green run must not degrade later recall | `learn.rs` dedup guard; `moat-edge-and-pollution-findings` (memory) |
| **Token economy** | recall costs far fewer tokens than reading the repo blind | `test_bug_location_e2e.rs` (345 vs 34,346 chars = 99.6× leaner) |

**Rule for a world-class claim:** the headline coding A/B (the three axes) is necessary but NOT
sufficient. A "world-class memory" claim requires GREEN evidence across this table too — most rows
already have a test (CLAIMS-COVERAGE maps each), and a gap here is a real finding, not a footnote. The
suite is only as strong as its weakest uncovered ability.

## Does memory help ACCURACY or just COST?
Both, but **where the task sits relative to the agent's capability decides which**:
- **Accuracy** (solves what baseline never does): at the **hard/edge tier or under shift**, and from
  learning from **failures** (Reflexion, ExpeL 2308.10144, AWM, ReasoningBank).
- **Cost only** (same solves, cheaper): on **easy in-distribution** tasks the base agent saturates;
  memory just reduces tokens/turns (MemGPT, Mem0).
Report them separately; expect accuracy gains at the edge, cost gains everywhere.

## Our harness
`hard-eval/bench_research.sh` (+ `hard-eval/bench_aggregate.sh`): 2 arms × tasks × n≥5 samples, with a
verify-bodies gate (#8 guard) before running, an explicit abstention probe (a question answerable from NO
file/fix — correct iff the agent gives up cleanly), and a PER-TASK prompt suffix (give-up hint ONLY on the
abstention probe; neutral elsewhere). A failed/timed-out run is retried then marked INVALID and EXCLUDED
(never scored correct=0). Outputs per-sample turns/cost/correct/abstained/valid →
`bench_aggregate.sh` computes pass@k (INVALID excluded) + co-solved turns + abstention, partitioned into
the three buckets.

## Lessons that produced this protocol (from docs/20 v1–v7, the A1–A5 runs)

This methodology is not abstract — it is the distillation of eight rules learned the hard way over seven
benchmark versions. The full write-up with evidence is in
[`20-learning-benchmark.md`](20-learning-benchmark.md) ("LESSONS LEARNED from A1–A5"). In brief:

1. **Non-file gold** — the answer must live in no source file, else you measure code-reading not memory.
2. **`init` is the floor in both arms** — the seeded learning is the only difference.
3. **Store as structured `learn_fix`** — a lazy `remember` one-liner buries below the indexed source.
4. **Lenient + LLM-graded oracle** — strict regex undercounts (scored 1–2/5 when both arms answered ~4/5).
5. **Partition A*→axis** — A3 = accuracy/memory-only-solves; A1/A2/A5 = convergence; A4 = abstention.
6. **Abstention is per-task** — the give-up hint only on the unanswerable probe, never globally.
7. **A failed run ≠ a wrong answer** — retry, then EXCLUDE as INVALID; never score it 0.
8. **n≥5, report mean±SD** — single runs and N=2 are noise; the per-task winner moves run-to-run. (No
   RNG seed loop: `.said` is deterministic and the LLM is external/unseeded — see "Sample sizes" above.)
9. **HOW memory is injected is the lever** — facts-not-claims + solve-first lead + decision-point
   re-injection + top-K (the nudge pattern, doc 22 / L9). Re-confirm the injection path before any run.

**Citations:** pass@k 2107.03374 · pass^k 2406.12045 · Reflexion 2303.11366 · Cost-of-Pass 2504.13359 ·
P(IK) 2207.05221 · selective QA 2006.09462 · AbstentionBench 2506.09038 · early-abstention 2502.09054 ·
Voyager 2305.16291 · AWM 2409.07429 · ReasoningBank 2509.25140 · ExpeL 2308.10144.
