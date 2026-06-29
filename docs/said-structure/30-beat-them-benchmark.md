# 30 — The "beat them" benchmark: compounding memory, not fact-recall

How `.said` (canon/blueprints + fixes + OKF wiki, self-growing, portable) out-smarts Claude / Cursor /
Kimi **at their own game**. Defined BEFORE building (metric-first), grounded in arXiv.

## The seam (their weakness, measured)

Existing memory systems and their benchmarks are tuned for **short-range fact recall**: *"94% of LoCoMo /
85% of LongMemEval questions need <= 2 previous sessions; current benchmarks emphasize relatively
short-range memory rather than sustained long-term accumulation"*
([LoCoMo / LongMemEval](https://www.emergentmind.com/topics/locomo-and-longmemeval-_s-benchmarks)). And
the open frontier they have NOT solved: *"current approaches primarily emphasize inference-time reuse
rather than principled CONSOLIDATION and GENERALIZATION"*
([Self-Consolidation for Self-Evolving Agents, arXiv:2602.01966]).

**That unsolved frontier is exactly what `.said` does**: canon + fixes that consolidate (keep-first +
verified promote) and compound across tasks/projects/tools. The self-improvement literature confirms the
winning property — *"self-improvements learned in one setting continue to accumulate in different settings
... cross-task generalization"* ([Trajectory-Informed Memory, arXiv:2603.10600]; Self-Improvement survey,
arXiv:2603.25681). So we do NOT compete on "recall a recent fact" (everyone does that); we compete on
**sustained accumulation** — where they are weakest and we are built for it.

## Why "they also use markdown" is not the contest

Their memory being markdown facts is a NON-issue for us: `.said` natively supports OKF wiki trees +
imports markdown facts/rules/plans (doc 29 adapters). Parity on storage is table stakes. The contest is
what the memory DOES over time: flat markdown is read-and-paste (no consolidation, per-tool, server-side
for Cursor/Kimi); `.said` is self-growing, deduped/promoted, semantic-recalled, and portable in one file.

## The metrics (the axis they're weak on — COMPOUNDING)

| Metric | Definition | Why `.said` wins | Harness |
|---|---|---|---|
| **Effort-decay** | turns/tokens to build entity #1 vs #5 vs #20 of a shape | canon makes the 80% free by #2; flat markdown never compounds | `run-bench.js` (extend to N entities) |
| **Cross-task transfer** | a fix/canon learned in project A correctly applied in project B | theirs is per-project/per-tool; ours federates via `project:` tags + recall | `recall-at-k-nl.js` + `test_project_scope.rs` |
| **Cross-tool portable continuity** | resume the SAME work-state Claude -> Cursor -> Kimi -> new machine | theirs is per-tool / server-side / dies on switch; ours is ONE file | new: capture -> switch -> resume e2e |
| **Consolidation quality** | re-encountering a shape UPDATES the canon (keep-first + verified promote), not duplicate pile-up | the "principled consolidation" the survey calls unsolved | `test_blueprint.rs` (keep-first/promote) |
| **Abstention** | refuse when nothing relevant (no confabulation) | already in `ask`; matches LongMemEval's abstention axis | existing recall gate |
| **Federation (cross-project)** | a fix/canon learned in project A surfaces in project B when opted in; isolated when not | theirs is per-project silos; ours federates via `project:` tags + `best_iterations_federated` | `test_project_scope.rs` + a 2-project e2e |
| **COMPACTION SURVIVAL (the headline moat)** | after the host compacts/summarizes (loses the last ~1M tokens of detail), can the agent RE-GROUND to exactly where it was — task, decisions, the precise values summarization discarded? | THIS is the #1 unfixable weakness of Claude/Cursor/Kimi: their working memory IS the context window, so compaction = amnesia ("goes stupid, doesn't know what happened"). `.said` is EXTERNAL + durable — it re-injects the exact work-state + fixes on the next turn, as if nothing disappeared. None of them can do this from inside the window. | new: simulate compaction -> SessionStart/UserPromptSubmit re-ground from `.said` -> agent continues correctly |

## THE headline: compaction survival — the weakness none of them can fix

Measured/evidenced failure of ALL three engines: *"LLM summarization can introduce hallucinations,
paraphrase exact details, and LOSE TECHNICAL SPECIFICS"*; *"context rot: measurable degradation simply
from increasing input length"*; *"cumulative information loss with multiple compactions... compounding
errors"*; *"compaction is inherently lossy for fact-dense content — specific numerical values, edge cases,
exceptions... is precisely what compression discards first"* (context-compaction research
gist badlogic/cd2ef65; Facts as First-Class Objects, arXiv:2603.17781; Active Context Compression,
arXiv:2601.07190). The owner's framing, confirmed: *"when they compact their tokens/memories they lose
context of where we were and then it's stupid again — it doesn't know what happened in the last 1M
tokens."*

Why `.said` UNIQUELY fixes it: the thing being compacted IS the context window — you cannot fix amnesia
with the memory that's being erased. `.said` lives OUTSIDE the window (durable mmap file), so the precise
work-state + verified fixes + canon survive every compaction and are re-grounded on the next turn through
the trusted injection channel (doc 16). The bar: **"working first time, like nothing ever disappeared."**
This is the single most valuable metric in this doc — the others are compounding wins; THIS is the one
that makes the agent not-stupid after compaction, which no competitor can offer.

### The work-state schema (what MUST survive compaction)

A work-state frame captures exactly the fact-dense detail summarization discards first (arXiv: "specific
numerical values, edge cases, exceptions"). Stored in the vault for exact (lossless) recall, re-grounded
after compaction:

- **task** — what I'm doing right now ("building compaction-survival in said-vault").
- **next_step** — the immediate next action (the thing the agent forgets after compaction).
- **decisions** — choices made + their exact form ("chose Postgres"; "threshold = size > 1, NOT >= 1").
- **exact_values** — the lossy detail: thresholds, IDs, flags, file:line, commit hashes, numbers.
- **files** — the working set (what was being edited).
- **blockers / ruled_out** — what's stuck + dead ends already tried (don't repeat them).
- **plan_status** — where in the plan we are (the bit Claude/Cursor lose on resume).

The proof gate: after a simulated compaction (drop this detail from context), `.said` re-injects these
**byte-exact** (assert exact values, not paraphrases) — that's the "like nothing ever disappeared" bar.

### The named problem we solve: "Self-Consolidation for Self-Evolving Agents"

The research names the exact unsolved frontier `.said` targets: **Self-Consolidation for Self-Evolving
Agents** ([arXiv:2602.01966](https://arxiv.org/pdf/2602.01966)) — *"current approaches primarily emphasize
inference-time reuse rather than principled CONSOLIDATION and GENERALIZATION ... managing memory dynamics
across intra-task and cross-task timescales."* That is precisely `.said`'s mechanism: consolidate (canon
keep-first + verified promote, fix dedup/supersede), generalize (cross-project federation), and persist
OUTSIDE the compactable window. We adopt this as the name for the capability — `.said` is a
**self-consolidating, self-evolving memory** — and the compaction-survival + federation + effort-decay
metrics above are how we MEASURE that it's solved, not just claimed.

## Honest scope

This is a NEW benchmark on a DIFFERENT axis than LoCoMo/LongMemEval (which we can still run for parity on
fact-recall). The compounding axis is where the moat is and where no competitor is measured. Build the
metric harnesses first (some exist), then the work-state-continuity feature, then run the full comparison.
Status: DEFINED (this doc). Harnesses: effort + cross-task recall + consolidation EXIST; cross-tool
portable continuity is the one to build.
