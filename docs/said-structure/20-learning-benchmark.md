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

---

## v2 (proper structured learn_fix writes) — INCONCLUSIVE, reported as such

Re-ran the accumulation A/B with the memory arm seeded via PROPER structured `learn-fix` (problem + WHY +
change_set), not lazy `remember` labels. Raw result: baseline $0.773 / memory $1.053 (memory looked +36%
WORSE). But the run has too many confounds to conclude anything, so it is reported as inconclusive — not
as a disproof:

1. **Seed recall under-fired on the big brain.** The seeded `fix::` frames DID store (verified present),
   but on the 4,386-frame init'd brain a broad query (`ask "why does ask not walk the call-graph"`)
   returned the **indexed code** (`[0.68][symbol]`) over the seeded DECISION, and `recall-fix` came back
   "No known fix" for the broad phrasing. Yet a TARGETED query in the earlier isolated test recalled the
   same learn_fix at **[0.84]** with `recall-fix` returning the root cause verbatim. So learn_fix recall
   is **query-phrasing + threshold sensitive** on a large indexed brain — a real signal, but it means the
   benchmark measured phrasing luck, not accumulation value.
2. **A1 outlier dominated the cost.** memory/A1 = 12 turns / $0.438 vs baseline 5t / $0.105 — the agent,
   not handed a strong recall, read `ask.rs:366-372` source COMMENTS deeply (which the doc-comment fix
   now indexes) and spent 12 turns. One outlier = most of the +36%.
3. **Strict regex oracle** again under-counts correctness (3/5 both arms; the answers were largely right).

**Honest status of the whole write-back question:**
- BUILT + capability-proven: the agent can write (learn_fix/remember/journal), the SessionEnd backstop
  captures, and a TARGETED recall of a structured learn_fix returns the answer verbatim at 0.84.
- NOT cleanly proven by an A/B: every accumulation run so far has a confound (lazy notes v1; seed-recall
  phrasing + an outlier v2; over-strict oracle throughout). The one defensible positive remains the
  corrected docs/20 run on NON-file knowledge (~25% cheaper), and the one defensible mechanism fact is the
  0.84 targeted learn_fix recall.

**Decision: stop running confounded single-shot A/Bs.** A trustworthy proof needs (a) a fixed, lenient
LLM-graded oracle, (b) pass-rate over N to kill outliers like A1, (c) queries that match how the stored
learning is phrased (or an LLM rerank of the top-N, the documented headless path). Until then we claim
only what's mechanism-proven, not a cost win on indexed code.

---

## v3 — the FIRST run with working end-to-end recall (after the encoder fix)

The v1/v2 runs were all invalid: v1 used lazy `remember` labels, v2 used a brain/MCP whose encoder was
dead (FIXES-LOG #5 — semantic scored 0.000, so recall could not fire at all). v3 is the first
accumulation A/B where recall actually works: correct coding-bundle CLI + the encoder baked into the MCP,
memory arm seeded via PROPER structured `learn-fix`, init as the floor in both arms, questions requiring
past-decision/fix/why knowledge that lives in no source file.

| Task | baseline | memory | note |
|---|---|---|---|
| A1 (why ask skips call-graph) | 4t / $0.064 ✓ | 10t / $0.303 ✓ | both right; memory re-read source anyway (behavioral miss) |
| A2 (sym/TRGM fix + why) | 18t / $0.284 ✓ | 9t / $0.202 ✓ | **memory −29%** (recalled the fix instead of grinding 18 turns) |
| A3 (encoder gotcha) | 9t / $0.156 **✗** | 9t / $0.169 **✓** | **memory CORRECT where baseline FAILED — the accumulation win** |
| A4 (hook channel why) | 1t / $0.057 ✗ | 1t / $0.054 ✗ | both gave up in 1 turn (broken/abstain task) |
| A5 (doc-comment fix) | 11t / $0.150 ✗ | 20t / $0.482 ✗ | outlier — memory ground 20 turns; both wrong |
| **Total** | **$0.711 / 43t / 2-of-5 correct** | **$1.210 / 49t / 3-of-5 correct** | |

### Honest reading

- **Correctness improved: memory 3/5 vs baseline 2/5** — driven by the **A3 flip** (memory answered the
  encoder-gotcha correctly; baseline could not, because that knowledge is in a test/decision the seeded
  learning carried, not in the implementation the baseline grepped). That is the accumulation effect
  working as designed: memory answers what the code alone cannot.
- **Cost is HIGHER, from two outliers, not a systematic loss:** A2 shows the intended win (memory −29%,
  recalled the fix vs baseline's 18-turn grind). But A1 (+$0.24) and A5 (+$0.33) are cases where the
  agent had the memory available yet investigated the source anyway — a behavioral miss (model didn't
  trust/lean on the injected recall), and A5 is a 20-turn outlier. On N=5 those two dominate the total.
- **Net:** the first valid run shows memory **more correct (3/5 vs 2/5)** with the **clearest single win
  being A2 (−29%)**, but **not cheaper overall** because two tasks where the agent over-investigated
  swamp the average. This is consistent with the standing caveat: N=5 is noise-dominated; the signal is
  the A3 correctness flip + the A2 cost win, not the aggregate.

### What's still needed for a clean claim (unchanged)

Pass-rate over N (to kill A1/A5-style outliers), a lenient LLM-graded oracle (the regex undercounts —
both arms answered A4/A5 more correctly than scored), and queries phrased to match the stored learning
(or the documented headless LLM rerank). The MECHANISM is now proven end-to-end (recall_fix returns the
stored confirmed fix at 0.82 CLI + MCP); what remains unproven is a clean aggregate COST win, and we do
not claim one.

---

## v4 — after the nudge injection fix: injection works, recall precision is now the bottleneck

Re-ran with the corrected PLAIN-FACTS injection (commit 5fabe2d, the nudge pattern — see doc 22).
Aggregate: baseline $0.874 / 29t / 2-of-5 correct; memory $1.205 / 51t / 3-of-5 correct. The aggregate
still shows memory pricier — but the cause is now ISOLATED and it is NOT the injection:

| Task | base | mem | recall fired? |
|---|---|---|---|
| A1 | 4t/$0.137 ✓ | 5t/$0.082 ✓ | yes — **memory −40%** |
| A2 | 9t/$0.167 ✓ | 19t/$0.352 ✓ | **NO ("No known fix")** → agent investigated |
| A3 | 5t/$0.073 ✗ | 9t/$0.149 **✓** | yes — **correctness flip (accumulation win)** |
| A4 | 1t/$0.059 ✗ | 3t/$0.114 ✗ | partial |
| A5 | 10t/$0.438 ✗ | 15t/$0.508 ✗ | **NO ("No known fix")** → agent investigated |

**Two cleanly-separated problems; the injection one is fixed:**
- **Injection framing — FIXED.** When recall fires, the plain-facts injection is USED: A1 memory −40%, and
  the isolated proof was 17→1 turns / $0.27→$0.045 on the sym/TRGM question. The earlier
  re-investigation (the "rejection") is gone (doc 22).
- **Recall precision — the remaining bottleneck.** A2 and A5 returned **"No known fix"** — the seeded
  verified fix EXISTS but the benchmark's paraphrased question scored below the 0.45 recall floor, so
  nothing was injected and the agent investigated from scratch (the cost). That is a recall-scoring
  problem (query-phrasing sensitivity of `best_coding_fixes` on a paraphrase), NOT an injection problem.

**Honest net:** the injection mechanism is proven (used, not re-investigated, ~6–40% cheaper when it
fires) and accumulation improves correctness (A3 flip). The aggregate is held flat by recall MISSING on
2/5 paraphrased questions. Next lever is recall precision for paraphrased fix queries (the documented
top-N + LLM-rerank path, or a lower/looser fix floor) — not the injection, which is done.

---

## v5 — both fixes in (nudge injection + fix-recall starvation): memory now wins on BOTH axes

After the injection fix (5fabe2d) AND the fix-recall-starvation fix (fa7a99d, restoring the 14.3
SCA-survival guarantee), the accumulation A/B flipped for the first time:

| | baseline | memory |
|---|---|---|
| Cost | $0.850 | **$0.806** (cheaper) |
| Correct | 3/5 | **4/5** |

Per-task:

| Task | baseline | memory | what it shows |
|---|---|---|---|
| A1 | 4t/$0.063 ✓ | 4t/$0.060 ✓ | tie (slightly cheaper) |
| A2 | 10t/$0.158 ✓ | 19t/$0.319 ✓ | baseline cheaper — memory over-investigated (the residual paraphrase-floor case: A2 fingerprints are strong but the weak spine keeps it ~0.34, borderline) |
| A3 | 5t/$0.078 ✗ | 9t/$0.151 **✓** | **correctness flip — accumulation answers what the code can't** |
| A4 | 1t/$0.051 ✗ | 2t/$0.094 ✗ | both wrong (abstain-style task) |
| A5 | 18t/$0.501 ✓ | 13t/$0.182 **✓** | **memory −64% — the fix-recall fix paying off** (A5 was the "No known fix" paraphrase before fa7a99d; now recalled + used, vs baseline grinding 18 turns) |

**Honest reading.** This is the significant shift: memory is now cheaper AND more correct. The wins are
directly attributable to the two fixes — A5 (−64%) is the previously-starved paraphrase now recalled
(fa7a99d), A3 is the accumulation correctness flip, and A1 shows the plain-facts injection used cheaply
(5fabe2d). The one loss, A2, is the residual paraphrase-floor case already tracked (strong fingerprints,
weak spine → borderline 0.34, so the memory wasn't confidently injected and the agent investigated).

**Caveat (unchanged, stated plainly).** N=5 with two large opposite swings (A5 −64%, A2 +102%) is
directionally convincing but not statistically airtight. The mechanism wins are real and explained; a
pass-rate over N + the A2 paraphrase-floor calibration would harden the aggregate. We claim: with the
injection + fix-recall fixes, memory recall is USED (not re-investigated) and the accumulation A/B now
nets cheaper + more correct — first time it has.

---

## v6 — CLEAN fixture (after the #8 corruption fix): memory wins all three axes (modestly)

The first accumulation A/B on a VERIFIED-clean fixture. Earlier runs (v3–v5) were corrupted by FIXES-LOG
#8 (a 2nd learn-fix blanked prior fix bodies, so 4 of 5 seeded fixes had empty bodies — the "correctness
regressions" were that, not the recall logic). With #8 fixed, the hardened harness's verify-bodies gate
confirmed all 5 fix bodies present (628–740 chars, 5 distinct ids) before running. 2 samples/task.

| | baseline | memory |
|---|---|---|
| Cost | $2.292 | **$2.225** (−3%) |
| Turns | 102 | **81** (−21%) |
| Correct | 6/10 | **7/10** |

Per-task (avg of 2):

| Task | baseline | memory | note |
|---|---|---|---|
| A1 (callgraph decision) | $0.507 c2/2 | **$0.078 c2/2 (−85%)** | the clean win — baseline grinds ~20 turns, memory recalls in ~5 |
| A2 (TRGM fix+why) | $0.195 c2/2 | $0.296 c2/2 (+52%) | memory over-investigated despite recall |
| A3 (encoder gotcha) | $0.192 c2/2 | $0.156 c2/2 (−19%) | solid win |
| A4 (hook-channel why) | $0.056 c0/2 | $0.061 c0/2 | both wrong (abstain-style) |
| A5 (doc-comment fix) | $0.196 c0/2 | $0.522 c1/2 (+167%) | outlier: memory spent the turns and actually SOLVED it (baseline 0/2, memory 1/2) |

**Honest verdict.** On a trustworthy fixture, memory beats baseline on cost (−3%), turns (−21%), AND
correctness (7/10 vs 6/10) — the first time all three align. The win is real but MODEST and driven by
A1 (−85%), partly offset by A5/A2 over-investigation. With N=2 over 5 tasks, two large opposite swings
(A1 −85%, A5 +167%) make it directionally convincing, not statistically tight. The machinery is now
sound end-to-end (the corruption that masked every prior run is fixed and guarded); the residual softness
is tuning (A2/A5 over-investigation: the agent sometimes verifies the recall instead of trusting it) and
sample size, not broken recall. A larger pass-rate-over-N would harden the aggregate.

---

## v7 — decision-point re-injection across all tasks: aggregate win stable, per-task is VARIANCE-dominated

Re-ran the full clean A/B after adding nudge's decision-point re-injection (fix-first recall on
UserPromptSubmit + PreToolUse + PostToolUse, commit 637b13f). Clean fixture (5/5 bodies). 2 samples/task.

| | baseline | memory |
|---|---|---|
| Cost | $1.628 | **$1.484 (−9%)** |
| Turns | 89 | **84 (−6%)** |
| Correct | 7/10 | 7/10 |

Per-task: A1 −33%, A4 −28% (clear wins); A3 ~tie; **A2 +5%, A5 +6% (slightly worse this run)**.

**Honest cross-run finding.** The aggregate "memory is modestly cheaper" is STABLE across v6 and v7
(−3% then −9%). But the PER-TASK winners/losers MOVE between runs: v6 had A1 winning big and A2/A5
losing; v7 has A1+A4 winning and A2/A5 lagging. A2 specifically went 16t (pre-fix) → 8t (isolated
re-test) → 10t (this full run) — it is NOT deterministically fixed; the agent sometimes trusts the
re-injected fix and stops, sometimes still verifies against source. So:

- The decision-point re-injection mechanism is sound and uniform (fires for every task, nothing
  regressed badly), and the aggregate is a consistent modest win.
- It is NOT "every task fixed exactly the same" — per-task results are variance-dominated at N=2. An
  earlier "A2 fixed (16→8)" claim was over-stated off a single sample; the full run corrects it.

**What a real per-task claim needs:** 5–10 samples/task to average out the agent's run-to-run variance
(the agent's verify-vs-trust decision is stochastic). Until then, claim only the stable aggregate
(memory cheaper, more or equal correct), not per-task determinism.

---

## LESSONS LEARNED from A1–A5 (the design rules every future memory benchmark must follow)

Seven versions (v1–v7) of this A/B taught a small number of hard rules. They are collected here so the
mistakes are never repeated, and they are the design contract behind the research-correct protocol in
[`23-benchmark-methodology.md`](23-benchmark-methodology.md).

### L1. The answer must live in NO source file (or the test measures code-reading, not memory)
The single biggest mistake (v1, and a regression I re-introduced once and had to be corrected on twice):
asking a question whose answer is in the indexed source. When both arms can `Read` the answer, the memory
arm just reads the code like the baseline — memory adds nothing, and can cost *more* (it sometimes verifies
the recall against the source anyway). Proven directly: on a fully-`init`'d brain, both arms answered
identical questions by reading the source; the injection was never cited. **Rule: a memory task's gold
answer must be a past DECISION, a FIX + its WHY, or a non-obvious INVARIANT that appears in no file as
prose.** `init` is the floor in BOTH arms; the only variable is the seeded non-file learning.

### L2. `init` is the floor in every arm; the seeded learning is the only difference
Keep base capability constant (Voyager/ReasoningBank/ExpeL): both arms get the same `init`'d codebase +
the same hook. The memory arm differs ONLY by carrying realistic prior-session learnings. This isolates
"does accumulated memory carry what the floor cannot" from "is the model good at reading code."

### L3. Store the RIGHT way — structured `learn_fix`, never a lazy `remember` one-liner
The retracted "+17% worse" result came from storing the learning as a one-line `remember` label, which
buried below the indexed source at recall time. The SAME learning stored as a structured `learn_fix`
(Title / Files+Functions / Learnings / WHY / change_set, via the one shared writer
`sca_core::ask::learn_coding_fix`) leads at `[0.84][semantic]` and `recall-fix` returns the root cause
verbatim. **A properly-recorded learning out-ranks even the indexed source it concerns.** A weak note
actively degrades recall (orchestration's own dedup-guard warns of this).

### L4. Strict regex oracles UNDERCOUNT — use a lenient/LLM-graded oracle
Every version flagged the same artifact: the regex scored 1–2/5 while both arms actually answered ~4/5.
Cost/turns were the trustworthy signal; raw "correct" counts were not. **Rule: gold tokens are lenient
substrings, and a real run adds an LLM-graded oracle pass (`SAID_LLM_GRADE`).**

### L5. Each A* maps to a DISTINCT docs/23 axis — partition, don't pool
- **A1** (why ask() skips the call-graph) → co-solved **convergence** (both answer; memory recalls the
  decision instead of grinding).
- **A2** (sym()/TRGM fix + WHY) → **convergence** (baseline greps ~18t to reconstruct the WHY; memory
  recalls it — the −29 to −47% win when recall fires).
- **A3** (the encoder/embed-model gotcha) → **ACCURACY / memory-only-solves** (baseline CANNOT answer;
  only the seeded learning carries it — the v3/v4 correctness *flip*).
- **A4** (a git SHA that exists in no file/fix) → **ABSTENTION** (clean give-up is CORRECT;
  grind-then-fabricate is the WORST outcome).
- **A5** (doc-comment recall bug + fix) → **convergence** (the fix-recall-starved paraphrase; −64% in v5
  once recall stopped starving).
Reporting these as one pooled number hides the mechanism; report the three buckets separately.

### L6. Abstention is a PER-TASK property, not a global prompt suffix
A blanket "say you don't have it and stop" hint on every task makes the agent abstain on ANSWERABLE
questions (measured: it gave up on A1/A2/A5 even though the fix was injected), collapsing both arms to
"both abstain" and destroying the comparison. **Rule: the give-up hint goes ONLY on the unanswerable
abstention probe (A4); answerable tasks get a neutral prompt.**

### L7. A failed RUN is not a wrong ANSWER — exclude it, never score it 0
A timed-out/errored `claude` call produces no valid output. Scoring that as `correct=0` mis-counts an
infra failure as the agent answering wrong, inflating the pass@k denominator with phantom failures.
**Rule: retry a failed run; if it still has no parseable result, mark it INVALID and EXCLUDE it from
pass@k denominators and turn stats** (`is_valid_run` gate + `valid` column; the aggregator skips INVALID).

### L8. Single runs (and N=2) are noise — the per-task verdict needs n≥5
The stable finding across v6/v7 is the *aggregate* (memory modestly cheaper, ≥ correct). The per-task
winners/losers MOVE between runs because the agent's verify-vs-trust decision is stochastic (A2 went
16t→8t→10t across runs). **Rule: claim only the stable aggregate until n≥5 (ideally ≥3 seeds); never
state per-task determinism off one sample.** This is the open item every version ended on, and the reason
docs/23 mandates n≥5.

### Net design contract (what a correct A1–A5 run looks like)
init-floor both arms · non-file gold (L1) · structured learn_fix seeds (L3) · A*→axis partition (L5) ·
per-task abstention only on A4 (L6) · NA-exclusion (L7) · lenient+LLM oracle (L4) · n≥5 + seeds (L8).
The harness `hard-eval/bench_research.sh` now encodes all eight; `hard-eval/bench_aggregate.sh` computes
the three axes with INVALID excluded.

### L9. HOW the memory is injected is the lever the benchmark measures — the nudge pattern

A1–A5 does not just measure "is the right learning recalled" — it measures whether the agent **USES** it
instead of re-deriving from source. That depends entirely on the injection mechanism, and the benchmark
history is the proof that the **nudge pattern** (documented in
[`22-memory-injection-nudge-pattern.md`](22-memory-injection-nudge-pattern.md)) is what makes it work.
The two docs are one story: doc 22 is the *mechanism*, this doc is its *measurement*. The exact changes
and their measured effect:

| Injection change (commit) | Doc 22 rule | Measured effect in this benchmark |
|---|---|---|
| Plain FACTS, not an authority/imperative claim (5fabe2d) | "inject as plain facts; never 'gate-verified, REUSE this'" | the isolated proof: **17 turns → 1 turn, $0.27 → $0.045** — the `<verified_memory>` "REUSE this" framing made the agent re-investigate to *verify the claim*; plain `<project_memory>` facts were used directly |
| Solve-first lead line (8795d2d) | "Found prior work… read this before repeating old debugging work" | A2/A5 stop grinding when recall fires (v5: A5 **−64%**) |
| Decision-point re-injection on PreToolUse, not only UserPromptSubmit (637b13f) | nudge's anti-decay: re-inject at the moment the agent reaches for a tool | A2 16t→8–9t when the agent reaching to `Read` source re-surfaces "you already concluded this" (variance-dominated at N=2 — see L8) |
| Top-K = 3, model picks (e8f9f95) | nudge `HOOK_SEARCH_LIMIT=3` | rescues the case where the right learning isn't rank-1 (the rank-3-brain A/B: top-1 RED 5 attempts → top-5 GREEN 1) |

**The causal chain the versions established:** v4 isolated that *injection framing was the problem, not
recall* ("when recall fires, the plain-facts injection is USED: A1 −40%"); v5 showed that **with the nudge
injection + the fix-recall-starvation fix, memory wins on both axes for the first time**. So a benchmark
result is only valid against the *current* injection mechanism — if doc 22's pattern changes, these
numbers must be re-measured. **Rule: never benchmark memory without first confirming the injection path
matches doc 22 (plain facts + solve-first lead + decision-point re-injection + top-K).** A regression in
the injection mechanism shows up here as "recall fired but the agent re-investigated anyway" (turns UP),
which is exactly the doc-22 failure signature.
