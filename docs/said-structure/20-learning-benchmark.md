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
