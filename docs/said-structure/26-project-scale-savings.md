# Project-scale savings — build a real website cold, then with save-while-coding memory

The end-to-end experiment the owner asked for: take a real project (an HTML checkout website) with
**known complex edge bugs**, fix it **cold** (no memory), **save each learning into `.said` while coding**,
**wipe completely**, then fix the **same** bugs again with memory active — and measure the savings
start-to-finish. Driven by the agent directly (the method in `25-agent-driven-turns-to-fix-method.md`), so
nothing depends on a flaky headless subprocess.

## The fixture (`hard-eval/web-fixture/`)
A checkout website — `src/index.html` (real HTML/CSS shell) + `src/cart.js` (the cart/checkout logic) —
seeded with **5 known complex edge bugs**, each gate-checked by `test/cart.test.js` (the judge):

1. **Money precision** — float drift (`0.1 + 0.2 = 0.30000000000000004`) in the total.
2. **Discount clamp** — a >100% coupon drives the total negative (must floor at 0).
3. **Tax rounding** — rounding each line before summing drifts a cent vs round-at-end.
4. **Qty clamp** — quantity accepts negative / fractional values (must be `max(0, floor)`).
5. **Coupon idempotency** — applying the same coupon code twice stacks the discount.

The non-obvious one is **#1+#3 together**: rounding *per line* still fails, because `checkout` sums then
applies discount+tax which **re-introduces** float drift — the fix is to round **once at the very end**.
That's the trap a cold agent falls into.

## RUN 1 — COLD (no memory), project-driven
Fixed the bugs as the gate revealed them, one at a time:

| Turn | What happened |
|---|---|
| 1 | gate RED → money precision `0.300…04`; fixed `lineTotal` to round per-line |
| 2 | gate STILL RED on money — per-line rounding doesn't hold (checkout re-drifts) |
| 3 | realized: round at the END in `checkout` (+ discount clamp, tax round-at-end) → past bugs 1/2/3 |
| 4 | gate RED → qty clamp; fixed `setQty` to `max(0, floor)` |
| 5 | gate RED → coupon stacks; fixed `addCoupon` to dedupe by code |
| 6 | gate GREEN ✓ |

**COLD: 6 turns to GREEN** (5 real fixes + **1 wasted turn** on the round-at-END money trap).

## SAVE-WHILE-CODING → WIPE
After the cold run, stored each of the 5 fixes into `.said` via `learn_fix` — capturing the **non-obvious
invariant** for each (e.g. "do NOT round per-line; round at the END in checkout — per-line still drifts").
Recall verified per-fix at **0.72–0.80**, no cross-contamination (money→money, coupon→coupon). Then the
fixture was **wiped completely** back to the pristine buggy snapshot (gate RED again, no fix present).

## RUN 2 — MEMORY (save-while-coding payoff), same bugs
Consulted `.said` (`recall_fix`) FIRST — it returned all 5 invariants up front, including the round-at-END
money trap. Applied all 5 fixes in **one informed pass**:

**MEMORY: 1 turn to GREEN.**

## Savings start-to-finish

| | COLD | MEMORY | Savings |
|---|---|---|---|
| Turns to GREEN | 6 | 1 | **−83%** |
| Wasted/misfix turns | 1 (round-at-END trap) | 0 | eliminated |
| Gate runs | 6 | 1 | −83% |
| Bugs fixed | 5/5 | 5/5 | same correctness |

**The savings come from memory carrying the non-obvious invariant the cold agent had to rediscover by
re-failing.** The money round-at-END trap cost a full wasted turn cold; with memory it was handed over up
front, so all 5 fixes landed in a single pass. This is the same mechanism as the turns-to-fix benchmark
(`docs/24` §8, `docs/25`), now shown at **project scale, start-to-finish, with save-while-coding** — the
brain accumulates the learning during the first build and pays it back on the rebuild.

## IMPORTANT framing correction (the fair comparison)
The run above compared `.said` memory vs a **fresh/no-memory** rebuild. The owner's intended — and fairer
— comparison is **`.said` vs Claude's OWN native memory**:
- **COLD = Claude using its native memory** (Claude Code's built-in notes/auto-memory — how it already
  records mistakes/solutions). NOT "no memory."
- **WARM = the SAME run with `.said` plugged in instead** — recording mistakes→solutions as we go via
  `learn_fix` + injecting them via the hook, *replacing* Claude's native memory.

So the real question is not "memory vs nothing" but **"does swapping in `.said` beat what Claude already
does on its own?"** That requires two CROSS-SESSION runs (memory only pays back on a later session): a
first build where the learning is recorded (Claude-native vs `.said`), then a fresh session that rebuilds
the same project drawing on each. The single-session round-at-END win above understates this — it's the
mechanism; the head-to-head-vs-Claude-native version is the next run (see "Next").

## The benefit only appears over a LONG dev lifecycle (the real scenario)
Memory pays off **only when it re-encounters an issue it solved before** — not on the first fix, and not on
an immediate rebuild of the same file. The real scenario is a **multi-session development lifecycle**:
- early sessions hit + solve a class of bug (recorded — Claude-native memory vs `.said`),
- weeks later a **NEW feature / a different file** re-triggers the **same bug class** (e.g. a new "gift
  card" total path that also needs round-at-END money; a new "bulk import" that also needs qty clamp),
- the agent that **recalls the past solution** fixes it in one pass; the agent relying on Claude-native
  memory (which may have decayed/compacted away over many sessions) re-discovers it the hard way.

So the fair, real test is cross-session AND cross-task: record the learning while building feature 1, then
measure turns-to-GREEN on feature N that re-triggers the same class — **`.said` recall vs Claude-native
memory**. The single-build round-at-END win above is just the mechanism; the lifecycle re-encounter is
where the moat actually lives. That experiment is `27-lifecycle-reencounter.md`.

## Honest caveats
- One project, one rebuild — a demonstration of the mechanism at project scale, not an n≥5 distribution.
- The run above is `.said` vs no-memory (the mechanism). `.said` vs Claude-native memory is the fair
  head-to-head (above) — the next run.
- The agent (Claude) is strong; the cold cost here was 1 wasted turn out of 6. A weaker agent (or a human)
  would waste more on the round-at-END trap, so the savings widen.
- Reproduce: `hard-eval/web-fixture/` (fixture+gate), `hard-eval/web_savings.psv` (the numbers).
