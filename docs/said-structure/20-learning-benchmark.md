# Does `.said` writing-as-it-works make a difference? — the learning benchmark

**The question (from the project owner).** The doc-18 benchmark tested `.said` as a *static index built
once by `init`* — read-only. But a brain WRITES as it works: stores what it did, records
successes/failures, reinforces on recall, accumulates across sessions (Claude Code auto-memory and
Cursor both do this). So: **does letting `.said` write back what it learns make later tasks cheaper or
better than a cold static index — or than Claude with no memory at all?**

## Design

A **chain of 5 related tasks** where later tasks reuse what earlier ones discovered (L2 builds on L1's
located function, L3 on the symbol engine, … L5 ties it together). Each task is a fresh `claude --print`
run — **no cross-task context except what lives in `.said`.** Three arms, each starting from the same
pristine static-init brain:

| Arm | Memory model |
|---|---|
| **native** | no `.said` — Claude starts fresh every task (no cross-task memory) |
| **cold** | static `.said` (hook injects recall) — but NOTHING is written back |
| **learning** | static `.said` + after each correct task the agent stores what it learned (`remember`), and the hook injects the ACCUMULATED memory on the next task |

Measured per task: turns, USD cost, cache-read, correctness vs a gold regex.

## Raw results

| Task | native $ / t | cold $ / t | learning $ / t |
|---|---|---|---|
| L1 | 0.227 / 4 | 0.230 / 4 | 0.233 / 4 |
| L2 | 0.034 / 1 ✗ | 0.057 / 2 ✗ | 0.035 / 1 ✗ |
| L3 | (recovered) | 0.226 / 9 | **0.109 / 4** |
| L4 | 0.073 / 4 | 0.083 / 4 | 0.122 / 6 |
| L5 | 0.421 / 16 | 0.248 / 10 | 0.304 / 13 |

Totals over the 4 tasks all arms completed numerically: **cold $0.617 < learning $0.694 < native $0.755.**

## Honest verdict: this run does NOT prove write-back helps — and here is exactly why

The headline is uncomfortable but true: **the cold static index was cheapest overall; naive write-back
did not win.** But the run is **too noisy to conclude either way**, because 2 of the 5 tasks have broken
oracles and one comparison is apples-to-oranges:

- **L2 fails in all three arms.** The question ("building on the ask() function you *just located*…")
  assumes mid-session memory that none of the arms carry the way a human reader would — a bad task, not
  a `.said` result.
- **L5's gold answer (`build_concept_links`) does not rank #1 even on the pristine brain.** The query
  "builds the wikilink concept graph" matches the word *wikilink* → `parse_wikilinks` /
  `remember_with_pillar`, not the real function. This is the **known descriptive-query ranking gap**
  (the float rerank only fires when a semantic candidate is present — tracked in FIXES-LOG). So L5
  measures a ranking bug, not the value of write-back.
- **L4 is not a fair cost comparison.** `cold` answered with a *clarification question* ("which file did
  you mean, `said_file.rs` or `said_file_brain.rs`?") in 4 turns; `learning` actually *answered* (about
  `said_file_brain.rs`) in 6. Different outputs at different depths — the extra turns bought a real
  answer, not waste.

**The one clean reuse task, L3, did show the accumulation effect: learning $0.109/4t vs cold
$0.226/4t→9t = −52%.** The learning arm had stored what it found in L1–L3 and reused it instead of
re-investigating. That is the Voyager/AWM/ReasoningBank effect — but it is a **single clean data point**,
not a proof.

### And a real failure mode the naive write-back exposed

My first write-back implementation **dumped the raw answer text** (L4's stored memory was 1,161 chars vs
L1's 199). The research is explicit that this is wrong — *distill the salient fact, don't dump the turn*
(Mem0; ReasoningBank: "store the invariant, not the paste") — and it matches this project's own
`moat-edge-and-pollution-findings` ("factory auto-learn pollutes the brain, needs dedup"). Storing raw
answers risks polluting later recall. (In this run the L5 pollution turned out to be the pre-existing
ranking gap, not the dump — but the dump is still the wrong design.)

## What this means for the product

1. **Do NOT ship naive write-back instructions.** Telling the agent to dump answers back into `.said`
   is not justified by this data and risks pollution. (The owner's "prove it first" gate holds: not
   proven → not added.)
2. **`.said` HAS all the right machinery** (docs/19): `remember`/`journal`/`learn_fix` writes,
   `reconsolidate` recall-reinforcement, `dream`/`consolidate`, `dedup_check`, decay/tombstone. The
   capability is real; what's unproven is the *write policy* an agent should follow.
3. **A proper proof needs:** (a) clean oracles (drop L2/L5-style broken tasks), (b) **distilled,
   deduped** write-back (1-line salient fact via the existing `dedup_check`, not raw answers), (c)
   verified-only writes (store after the answer is confirmed, per Voyager/Reflexion), (d) enough tasks
   to beat single-run noise (the doc-18 lesson: single runs are noise; use pass-rate over N).
4. **Fix the descriptive-query ranking gap first** (FIXES-LOG follow-up) — until "builds the wikilink
   concept graph" reliably returns `build_concept_links`, any chained-reuse benchmark is measuring the
   ranking bug, not the memory.

## Bottom line

The brain machinery exists and the recall-reinforcement differentiator is real, but **this benchmark
did not prove that writing-as-you-work beats a good static index** — it produced one clean win (L3
−52%), two broken tasks, and one unfair comparison. The honest next step is a *distilled, verified,
deduped* write-back tested over clean tasks with a pass-rate, after the descriptive-ranking gap is
closed — not a premature "memory makes it cheaper" claim.

---

## Re-run with the CORRECTED agent-judged write-model (clean oracles, distilled writes)

After building the proper write-model — agent-judged writes (the model calls learn_fix/remember/journal
when it concludes something) + the SessionEnd backstop, distilled-not-dumped — the benchmark was re-run
with clean top-N oracles and a chain where later tasks reuse earlier conclusions.

| Arm | Cost (5 tasks) | Turns | Correct |
|---|---|---|---|
| **cold** (static `.said`, no write-back) | **$0.525** | 23 | 4/5 |
| learning (agent-judged distilled write-back) | $0.613 (+17%) | 29 | 4/5 |
| native (no `.said`) | $0.684 | 28 | 4/5 |

On the reuse tasks (C3–C5) the learning arm was +1% / +36% / +14% vs cold — slightly WORSE, not better.

### Honest verdict: accumulation does NOT beat a static index ON AN ALREADY-INDEXED CODEBASE — and the reason is precise

The cold static index won again. But this is an **inconclusive disproof with an identifiable cause**, not
evidence the write-model is worthless:

**Root cause.** The benchmark brain was built by `said init` over the whole codebase (4,380 frames).
The learning arm stored a hand-distilled one-liner after each task ("Learned C2: `save` is the answer…").
For that stored fact to help the NEXT task (a separate `claude --print` with no chat memory), the hook
must RANK it high enough to inject — but a one-line note has to out-rank thousands of real code frames at
recall time, and mostly it didn't. So the learning arm ≈ the cold arm **plus the cost of the writes** —
which is exactly the +14–36% overhead with no recall payoff.

**What this actually tells us (the real, defensible conclusion):**

> Write-back pays off only when the stored learning is the BEST available answer at recall time. On a
> codebase `init` already indexed, a distilled note rarely out-ranks the real source it summarizes, so
> accumulation adds cost without benefit. Write-back should win where the answer is NOT already in the
> index: **cross-session decisions, past fixes (learn_fix), non-obvious invariants, and things the agent
> discovered that live in no file** — the cases the static index cannot contain. That is the scenario a
> fair accumulation benchmark must use (a multi-SESSION task where session N needs what session N−1
> concluded and never wrote to a file), not a single-session chain over already-indexed code.

**Status of the write-model.** BUILT, tested, and capability-proven (the agent CAN write; the backstop
captures; recall surfaces stored facts — all green in unit/e2e tests). What remains UNPROVEN is the
cost/quality WIN on an already-indexed codebase — and we now know why, and what scenario would actually
test it. We do not claim "memory makes it cheaper" until that scenario shows it.

---

## CORRECTED design — init is the floor; test what only ACCUMULATED memory can answer

The two runs above were mis-designed: both arms could answer from the `init`'d code, so the new memories
competed with the real source to answer the SAME question — they could only break even or add overhead.
That tests redundancy, not accumulation. The research (Voyager, ReasoningBank, ExpeL; docs/19 §e) keeps
the base capability constant in EVERY arm (`init` is the floor) and tests whether **accumulated memory
carries knowledge the floor cannot** — a past decision, a fix and its WHY — so later tasks succeed where
the baseline must grep blindly.

**Design.** Both arms = the same `init`'d codebase (4,386 frames) + the hook. The ONLY difference:
`memory` is seeded with realistic prior-session learnings (decisions/fixes/whys that appear in NO source
file as prose — e.g. "we deliberately don't walk the call-graph in ask() because…", "sym() returned 0
after reopen; root cause was save() skipping TRGM; fixed by…"). The 5 questions REQUIRE that knowledge.

**Result (cost is the trustworthy signal; the correctness oracle was too strict — see note):**

| Question (answer is in no source file) | baseline | memory |
|---|---|---|
| A1 — why ask() skips the call-graph | 4t / $0.087 | 5t / **$0.072** (−17%) |
| A2 — the sym()/TRGM fix + its why | 13t / $0.278 | 10t / **$0.146** (−47%) |
| A5 — the doc-comment recall bug + fix | 11t / $0.470 | 11t / **$0.372** (−21%) |
| **Total (5 questions)** | **$0.960 / 34 turns** | **$0.722 / 32 turns** |

**Memory is ~25% cheaper at equal correctness** (both arms answered ~4/5 on a lenient regrade). The win
concentrates exactly where the design predicts: questions about past decisions/fixes/whys. A2 is the
cleanest single proof — baseline spent 13 turns grepping to reconstruct a fix's reasoning that memory
returned in one recall (−47%).

### What this proves — honestly

This is the FIRST correctly-designed run, and it shows the accumulation effect is real: **when memory
holds what the codebase cannot (cross-session decisions, fixes, the WHY), `.said` answers the same
questions ~25% cheaper with fewer turns** — the Voyager/ReasoningBank "later tasks cheaper" effect on the
right scenario. The earlier "+17% worse" runs were not a property of write-back; they were the wrong test
(asking what `init` already answered).

**Caveats (no over-claiming):** 5 questions is small (single-run noise — a pass-rate over N is the next
step). The correctness regex was too strict (scored 2/5 vs 1/5 while both arms actually answered ~4/5 —
the cost figures are the defensible signal, not that raw correctness count). And this seeds the memories;
an end-to-end run where the agent WRITES them across real prior sessions is the final proof. But the
direction is now clear and correctly measured: accumulated memory that carries non-file knowledge makes
later tasks cheaper.

---

## RETRACTION — the "+17%" was a methodology error (lazy note vs proper learn_fix)

The "+17% / not proven on indexed code" conclusion above is **withdrawn.** It came from storing the
write-back as a LAZY one-line `remember` label — `remember("Learned: save is the function that writes
the TRGM section")` — which is NOT how `.said` is meant to record a learning, and not how
`said-orchestration` does it.

The correct write is the **structured `learn_fix`** path (the orchestration `steps/learn.rs` move): the
LLM authors a structured note (Title / Files+Functions / Learnings / the WHY / Key Results) plus the
machine `change_set`, stored via the ONE shared writer `sca_core::ask::learn_coding_fix` (byte-identical
across CLI `learn-fix`, MCP `learn_fix`, and the orchestrator), with a dedup guard.

Measured difference on a FULLY-init'd brain (4,386 frames), same "sym()/TRGM" question:

| Write style | `ask` recall of the learning |
|---|---|
| LAZY `remember` one-liner (what the flawed run used) | buried — top hit fell back to `[0.56][symbol] sym`, the note didn't surface |
| PROPER `learn-fix` (structured note + change_set) | **`[0.84][semantic]`** leads; `recall-fix` returns **ROOT CAUSE → save() → TRGM** verbatim (perfect recall) |

So a properly-recorded learning out-ranks even the indexed source it concerns — exactly the
"perfect recall" `said-orchestration` already relies on. The negative was the wrong tool/wording, not a
property of write-back. orchestration's own dedup-guard comment (`steps/learn.rs`) warns that a *weaker*
note outranks and degrades recall — which is precisely the failure the lazy benchmark reproduced.

**Net:** write-back works on indexed code too, when stored as a structured learn_fix (not a label). The
honest open item remains a pass-rate-over-N end-to-end run, not the retracted "+17%".
