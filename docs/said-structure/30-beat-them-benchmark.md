# 30 — The "beat them" benchmark: compounding memory, not fact-recall

How `.said` (canon/blueprints + fixes + OKF wiki, self-growing, portable) out-smarts Claude / Cursor /
Kimi **at their own game**. Defined BEFORE building (metric-first), grounded in arXiv.

## THE KEY INSIGHT — why `.said` work-state beats Claude's `/compact` (the same-looking thing)

Claude's `/compact` captures the SAME content we do (task, next step, decisions, exact values, files,
blockers, plan). On the surface it looks identical — so the honest question is: **how are we actually
better, not just different?** Three real differences (everything else is cosmetic):

| Axis | Claude `/compact` | `.said` work-state | Why it wins |
|---|---|---|---|
| **WHO writes it / WHEN** | an LLM SUMMARIZES the whole history at ~95% capacity — ONCE, reactively, mid-task (context already degrading; users report "goes off the rails mid-task"). | the AGENT writes a note WHEN IT CONCLUDES something (a decision, an exact value), incrementally, while context is FRESH. | captured at the best moment (fresh + intentional), not the worst (a panic-summary at 95%). |
| **FIDELITY** | a GENERATED SUMMARY — it PARAPHRASES. arXiv + Claude's own docs: "loses technical specifics"; `threshold = size > 1` becomes "added a size check." | the agent's OWN WORDS stored VERBATIM, re-injected BYTE-EXACT. `size > 1, NOT >= 1` comes back identical, forever. | **THE difference**: a summary RE-DESCRIBES; we PRESERVE. The lossy fact-dense detail is exactly what they drop and we keep. |
| **WHERE it lives / SCOPE** | IN-BAND — the summary IS the next context window, so it is compacted AGAIN next time → CUMULATIVE loss. Session-only; gone on `/clear`, new session, tool switch. | OUT-OF-BAND — a durable file OUTSIDE the window. NEVER compacted (it's not in the window). Survives `/clear`, new sessions, TOOL SWITCH, MACHINE MOVE. | their memory degrades every cycle because it lives in the thing being degraded; ours doesn't degrade BECAUSE it's external. |

One line: **`/compact` is a lossy LLM summary that lives INSIDE the context window — it paraphrases the
exact detail, degrades more each compaction, and dies with the session. `.said` work-state is the agent's
OWN WORDS stored VERBATIM OUTSIDE the window — it never paraphrases, never degrades across compactions, and
survives session/tool/machine boundaries.** The fields look the same; the mechanism is OPPOSITE
(summarize-into-the-window, lossy/in-band/ephemeral  vs  preserve-outside-the-window, verbatim/out-of-band/
durable).

## WHAT FIXES EVERYTHING — free-form, core, verbatim (the design that made it work)

Three design choices, each learned the hard way earlier in this build, are why work-state actually works:

1. **CORE, not vault.** It lives in `sca-core::workstate` (like `ask`/`learn_coding_fix`/blueprint) —
   available to EVERY `.said` user on every surface. (The vault is a SEPARATE enterprise product; coupling
   a core capability to a paid tier was a mistake, corrected.)
2. **FREE-FORM, agent-authored — NOT a rigid field schema.** The agent writes ONE NL note in its OWN words.
   We do NOT force it into fixed fields (task/next/decisions/…) — that is the SAME rigidity trap as
   blueprint call-tokens, which 14.15 (Self-Spec) warns against: "don't impose a schema it fights." Fixed
   fields are offered only as an OPTIONAL hint (`WORKSTATE_HINT`). The agent's phrasing IS the memory.
3. **VERBATIM, out-of-band — not summarized, not re-encoded.** Stored as one frame `workstate::<project>`
   and read back by id = exact string roundtrip. No fuzzy match (keyed by project), no LLM rewrite. The
   note survives byte-for-byte — which is the whole point vs a lossy summary.

That combination — core + free-form + verbatim + out-of-band — is what turns "remember what I was doing"
from Claude's lossy in-band summary into a durable, exact, portable memory none of them can match.

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
| **COMPACTION SURVIVAL (the headline moat) — BUILT + PROVEN** | after the host compacts/summarizes (loses the last ~1M tokens of detail), can the agent RE-GROUND to exactly where it was — task, decisions, the precise values summarization discarded? | THIS is the #1 unfixable weakness of Claude/Cursor/Kimi: their working memory IS the context window, so compaction = amnesia ("goes stupid, doesn't know what happened"). `.said` is EXTERNAL + durable — it re-injects the exact work-state on the next turn, as if nothing disappeared. None of them can do this from inside the window. | **SHIPPED (CORE, not vault)**: `sca-core::workstate` (free-form note capture/load/resume) + CLI `said vault workstate-save/show/resume`. **PROVEN 7/7** via real `said.exe`: `hard-eval/beat-them/compaction-survival.js` (capture -> compaction -> re-ground EXACT values verbatim) |

## STATUS: compaction survival is BUILT + PROVEN (the moat is real, not just claimed)

Shipped as a CORE memory function in `sca-core::workstate` — like `ask`/`learn_coding_fix`/blueprint,
available to EVERY `.said` user on every surface. (NOT a vault feature: the vault is a separate
ENTERPRISE-only product for document compliance; work-state is core. It was prototyped in the vault first,
then moved.) The work-state is FREE-FORM, agent-authored — one NL note in the agent's OWN words, NOT a
rigid field schema (the blueprint-NL lesson + Self-Spec, 14.15: don't impose a schema it fights;
`WORKSTATE_HINT` offers a suggested shape as guidance only). Stored as one frame `workstate::<project>`
(Episodic, `kind:workstate`); read back by id = exact string roundtrip (no fuzzy match — keyed by project),
so the note survives VERBATIM. CLI: `said vault workstate-{save,show,resume}` (grouping; the impl is core).

PROVEN end-to-end 7/7 through the real `said.exe` (`hard-eval/beat-them/compaction-survival.js`, result in
`beat-them/results/compaction-survival.txt`): capture the agent's free-form mid-task note -> simulate host
compaction (detail gone) -> `workstate-resume` re-grounds the note verbatim, with the EXACT values intact
(`threshold = size > 1, NOT >= 1`; `MIN_COMMON_STEPS = 3`; `ask.rs:1542` formula; `commit fd2bf9e`; the
next step; the ruled-out dead end so it won't retry). The "like nothing ever disappeared" bar, demonstrated.
Unit test: `crates/sca-core/src/workstate.rs`. NEXT: wire the auto re-ground onto the host hook
(SessionStart / post-compaction UserPromptSubmit) so it fires without a manual command.

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
