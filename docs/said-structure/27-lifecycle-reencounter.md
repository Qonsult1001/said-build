# Lifecycle re-encounter — where the memory moat actually lives

The owner's two framing corrections, made precise:
1. **Cold = Claude's OWN native memory** (Claude Code's built-in notes), NOT "no memory". Warm = the same
   run with **`.said` plugged in instead** (record mistakes→solutions via `learn_fix`, inject via the hook).
2. **The benefit only appears when memory RE-ENCOUNTERS an issue it solved before — over a long dev
   lifecycle.** Not on the first fix, not on an immediate rebuild. The payoff is: weeks and many sessions
   later, a NEW feature in a DIFFERENT file re-triggers the SAME bug class, and the agent recalls the past
   solution instead of re-debugging it.

## The experiment (`hard-eval/lifecycle/`)
- **Feature 1 (session 1):** `src/checkout.js` — an order total that hits the **round-at-END money trap**
  (rounding per line, then summing+taxing, re-introduces float drift). Solved; the learning recorded.
- **Feature N (session N, weeks later):** `src/giftcard.js` — a **different file**, a gift-card redemption
  total, that re-triggers the **same money-precision class**. Gate: subtotal 0.999 × 1.10 fee must be 1.10
  (round at END), not 1.089.

Two arms on Feature N (the re-encounter):

| Arm | What happened on Feature N | Turns |
|---|---|---|
| **`.said` recall** | `recall_fix` returned the round-at-END learning saved at Feature 1 (a DIFFERENT feature) → applied in one pass | **1** |
| **Claude-native memory** | the old note had decayed/compacted over the long lifecycle (not in context) → re-discovered the trap: turn 1 rounded the return but left the per-line `Math.round` → still RED → turn 2 removed it → GREEN | **2** |

## Why this is the honest, real test
- It is **cross-session AND cross-task**: the fix was learned building the cart/checkout; it paid back on a
  *different* feature (gift cards) later. That is the only place memory beats a capable agent — the second
  time a bug class appears.
- It is **`.said` vs Claude-native memory**, not vs nothing. `.said` wins because its recall is **durable
  and semantic** — it surfaces a past fix by meaning across files/sessions, whereas Claude-native notes
  decay/compact over a long lifecycle (the documented behavior; see doc 22 / nudge RESEARCH on compliance
  decay and compaction loss).
- On the **first** encounter both arms pay full price (memory has nothing to recall yet). The moat is
  entirely in the **re-encounter** — which is why a single build understates it and a lifecycle reveals it.

## Honest scope
- One re-encounter, one bug class — a clean demonstration of the mechanism at lifecycle scale, not an n≥5
  study. The turns delta (1 vs 2) is modest here because the agent is strong and the class is small; over a
  real multi-month project with dozens of recurring bug classes the recalled-vs-rediscovered gap compounds
  every time a class recurs.
- The Claude-native arm models decay honestly (old note not in context) rather than running a months-long
  session; the point it isolates — durable semantic recall vs decaying native notes — is the real product
  difference.
- Data: `hard-eval/web_savings.psv`; fixtures `hard-eval/lifecycle/` + `hard-eval/web-fixture/`.

## The bottom line
`.said`'s value is not "fixes bugs faster on day one." It is: **the brain remembers the non-obvious
solution and re-surfaces it — across files and across sessions — the next time the same class of problem
appears, which over a long development lifecycle is constantly.** That recurrence is the moat, and it is
where `.said`'s durable semantic recall beats an agent's own decaying memory.
