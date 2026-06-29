# blueprint-bench — the blueprint (canon) end-to-end benchmark

Measures what `.said`'s blueprint memory actually buys a coding agent, **with vs without `.said`**, driven
through the **live `said-mcp` server** (the real product surface — encoder loaded once, like real use).

## What it measures (the four things)

1. **The 80/20 split** — on real C# source, the share that is reusable framework skeleton (`[80%]`-marked)
   vs entity-specific slots (`[20%]`-marked).
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

```
Harvested 3 blueprints from the C# examples (one-time onboarding).
Server+encoder warm-up: ~555ms ONCE (not per recall).

shape           | 80% | without(ch) | with(ch) | saved% | warm recall(ms)
01-rest-crud    | 93% |        5468 |      244 |    96% |          ~25
02-http-client  | 97% |        4895 |      388 |    92% |          ~12
03-cli-command  | 97% |        3430 |      252 |    93% |          ~11
TOTAL           |     |       13793 |      884 |    94% |

Learnings: save fix / recall-by-paraphrase / update-when-better (verified) / recall-improved — 4/4 PASS
```

**~94% fewer chars (tokens), ~80% reusable skeleton, warm recall ~10-25ms.**

## Honest caveats (read these)

- **Tokens are the defensible metric.** Build *wall-time* depends on the BYO-LLM, not `.said` — so we report
  `.said`'s own warm recall latency (ms), not a "time to build vs normal" claim that would conflate the LLM.
- **The "without" output cost** assumes the agent re-emits the full structure each time; an agent that
  copy-pastes from an open file pays less — but still pays the **read** cost and gets no cross-language /
  cross-project reuse, no auto-update, no portability.
- **Recall RANKING among harvested blueprints is weak** — and it is the ENCODER, not the scorer.
  `best_blueprints` is byte-identical to the proven `best_coding_fixes` (per 14.15: recall reuses the
  existing engine, no new mechanism). The weakness is the static Model2Vec encoder's isotropy on short
  code skeletons — diagnosed + solved in `SAID-ECHO/research/MinishLab` (`code_encoder_compare.md`:
  potion-code-16M ranks the right item #1 where the text encoder ranks >10; `02_semhash.md`: a 5-signal
  rerank). Confirmed at scale (42 blueprints), so it is not merely a tiny-corpus artifact. Fix path = the
  code encoder + semble rerank in the SHARED engine. What works today: harvest, exact-shape recall,
  cross-language render, the 80/20 saving, learnings. Each arm queries with the shape it builds; the
  learnings arm recalls by exact shape. See 14.15 "Known limit".
- Char counts are deterministic; warm recall ms varies a few ms per run (machine load).
