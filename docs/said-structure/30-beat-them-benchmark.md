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

## Honest scope

This is a NEW benchmark on a DIFFERENT axis than LoCoMo/LongMemEval (which we can still run for parity on
fact-recall). The compounding axis is where the moat is and where no competitor is measured. Build the
metric harnesses first (some exist), then the work-state-continuity feature, then run the full comparison.
Status: DEFINED (this doc). Harnesses: effort + cross-task recall + consolidation EXIST; cross-tool
portable continuity is the one to build.
