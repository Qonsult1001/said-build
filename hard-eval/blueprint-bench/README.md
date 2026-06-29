# blueprint-bench — the blueprint (canon) end-to-end benchmark

Measures what `.said`'s blueprint memory actually buys a coding agent, **with vs without `.said`**, driven
through the **live `said-mcp` server** (the real product surface — encoder loaded once, like real use).

## What it measures (the four things)

1. **The 80/20 split** — on real C# source marked with the canon `[Sn] GENERATED/YOURS` convention
   (same as `hard-eval/canon-proto`): GENERATED sections = the reusable 80% skeleton (.said rewrites them
   from the blueprint), YOURS sections = the entity-specific 20% (you keep/edit). Map: `said.index.md`.
2. **Tokens saved** — chars (the token proxy `token_value.psv` uses), counted identically in both arms.
3. **Time taken** — the **warm** `recall_blueprint` tool-call latency on the running server (NOT a cold
   process spawn). Server+encoder warm-up is reported **separately, once**.
4. **Learnings** — a bug fix saved via `learn_fix`, recalled by paraphrase, and the blueprint
   **auto-updated when a better way is found** (verified build → supersede).

## The two arms (honest method)

- **WITHOUT `.said`** (the normal agent): to render a shape in a new language it must **read the example
  files** to learn the structure, then **emit the full structure**. cost = chars read + full chars emitted.
- **WITH `.said`**: harvest the C# once (one-time onboarding), then **`recall_blueprint`** (small payload)
  and write **only the 20%**. cost = recall payload + the 20%.

## The examples (`examples/`)

Three small but real apps, each with the SAME shape appearing 2× in C# so harvest learns ONE blueprint;
the Rust render proves cross-language reuse:

- `01-rest-crud` — Create endpoint (Invoice + Order) → `create` blueprint
- `02-http-client` — call an external API (Weather + Geo) → `lookup` blueprint
- `03-cli-command` — a command handler (AddUser + RemoveUser) → `run` blueprint

## Run

```
node hard-eval/blueprint-bench/run-bench.js     # needs the encoder build: said.exe + said-mcp.exe built --features coding
```

## Result (representative)

```text
Harvested 3 blueprints from the C# examples (one-time onboarding).
Server+encoder warm-up: ~550ms ONCE (not per recall).

shape           | GEN% | without(ch) | with(ch) | saved% | warm recall(ms)
01-rest-crud    | 49%  |        6196 |      907 |    85% |          ~25
02-http-client  | 54%  |        5635 |      936 |    83% |          ~11
03-cli-command  | 36%  |        4188 |      833 |    80% |          ~11
TOTAL           |      |       16019 |     2676 |    83% |

Learnings: save fix / recall-by-paraphrase / update-when-better (verified) / recall-improved — 4/4 PASS
```

**HONEST numbers (measured against the real `[Sn] GENERATED/YOURS` markers, from `results/savings.psv`):**
- **~83% fewer chars (tokens)** — the agent recalls the GENERATED skeleton + writes only the YOURS slots,
  and avoids reading all the example files to learn the pattern.
- **GENERATED share is 36-54% (avg ~46%), NOT 80%** in these teaching examples. The "80/20" is the
  *concept*; the actual reusable share depends on the code. Real framework code (e.g. the harvested
  OrchestrationFactory) skews more GENERATED; these compact examples are more balanced. We do NOT claim
  80% here — we report what's measured.
- warm recall ~10-25ms (server warm-up ~550ms ONCE, separate).

CORRECTION HISTORY (removed false claims): an earlier version reported "93-97% reusable / 94% saved" —
that was inflated (it counted only a thin recall payload as the "with" cost, and labeled the split 80%
without measuring it). The numbers above are measured from the canon markers; the token saving (~83%) is
real, the GENERATED share is honestly lower than 80% for these examples.

## What arXiv says about these results (specific, not generic)

- **The 80/20 token saving is the documented target.** *"Tokens carrying zero value (structural
  boilerplate, ceremonial syntax) are pure waste... none [of existing RAG] proactively neutralize
  tokenization overhead, and none enforce structural discipline before retrieval"* (**Beyond
  Human-Readable**, arXiv:2604.07502). Blueprints strip exactly that boilerplate from the token cost — and
  the paper confirms existing retrieval does NOT. (Also: context degrades past ~40% utilization, so the
  saving is quality, not just cost.)
- **The cross-language render is the right design.** *"Natural language consistently emerges as the most
  effective intermediate representation across all target languages; no universally effective formal
  intermediate language exists"* (arXiv:2407.05411; **NL in the Middle**, arXiv:2507.08627). The canon is
  stored as structured-NL and rendered per language — on the right side of the literature.
- **The recall-ranking limit was hypothesized as "anisotropy" (Soft-ZCA, arXiv:2411.17538) — but we TESTED
  it and it's a NEGATIVE result.** Offline full Soft-ZCA (real corpus covariance, eigendecomp, α sweep)
  stayed 2/3; the cli query still mis-ranks. Corrected diagnosis: a **vocabulary gap** (implementation-token
  skeletons vs intent queries), not embedding geometry — whitening can't bridge it. Next lever = a
  token-based rerank (semble), not whitening. See 14.15 "Experiment 1 RESULT". Measure with
  `recall-ranking-probe.js`.

## Honest caveats (read these)

- **Tokens are the defensible metric.** Build *wall-time* depends on the BYO-LLM, not `.said` — so we report
  `.said`'s own warm recall latency (ms), not a "time to build vs normal" claim that would conflate the LLM.
- **The "without" output cost** assumes the agent re-emits the full structure each time; an agent that
  copy-pastes from an open file pays less — but still pays the **read** cost and gets no cross-language /
  cross-project reuse, no auto-update, no portability.
- **Recall RANKING among harvested blueprints is weak — an OPEN limit, not a known fix.** It is the
  encoder, not the scorer (`best_blueprints` is byte-identical to `best_coding_fixes`, per 14.15). MEASURED
  CORRECTION: the embedded encoder is ALREADY `said-lam-static-4M`, **128-dim** (verified live:
  `encode_query -> 128 dims`), not the 64-dim model the older MinishLab research tested. The weakness was
  measured ON the 128-dim encoder at scale (42 blueprints) — so doubling general-text dims (64→128) did NOT
  fix short-code separation. The untested lever is a code-SPECIALIZED encoder or the semble rerank
  (`MinishLab/02_semhash.md`), NOT "more dimensions." Honest OPEN question until measured on a real brain.
  What works today: harvest, exact-shape recall, cross-language render, the 80/20 saving, learnings. Each
  arm queries with the shape it builds; the learnings arm recalls by exact shape. See 14.15 "Known limit".
- Char counts are deterministic; warm recall ms varies a few ms per run (machine load).
