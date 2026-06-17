# 15 — Coding orchestration (the model-agnostic Claude-Code loop)

The orchestrator makes **any** external LLM behave like Claude Code: it drives the full plan → design → code → test → repair → learn lifecycle, gates every change with the project's real build/test commands, and transfers verified *learning* from the `.said` brain so a weak/cheap model succeeds where it fails alone.

Lives in [`crates/said-orchestration`](../../crates/said-orchestration/). It is a **separate process** from the core `.said` binary — per the BYO-LLM rule ([13 — integrations](13-integrations.md)), the `.said` library never calls an LLM; the orchestrator does, via [`said-llm`](../../crates/said-llm/).

## The three pieces it composes

| Piece | Crate | Role |
|---|---|---|
| The playbook (prompts) | [`said-prompts`](../../crates/said-prompts/src/agents/coding.rs) | The canonical phase prompts, in Claude Code's voice |
| The story (memory) | [`sca-core`](../../crates/sca-core/) | Recall verified iterations; store new ones |
| The model | [`said-llm`](../../crates/said-llm/) | The BYO-LLM provider (Groq / OpenAI-compatible / Anthropic / Claude-CLI) |

The orchestrator itself ([`steps/mod.rs`](../../crates/said-orchestration/src/steps/mod.rs)) is just the loop that wires them together. Each lifecycle step is one file under [`steps/`](../../crates/said-orchestration/src/steps/).

## One shared learning store (CLI · MCP · orchestrator)

Coding-fix memory is written and read through **one** pair of functions in
[`sca_core::ask`](../../crates/sca-core/src/ask.rs):

- `learn_coding_fix(brain, problem, note, edits_json, label)` — store a verified iteration
- `recall_coding_fix(brain, problem, min_score)` — recall the best match (semantic scorer)

Every surface calls these, so they can never drift:

| Surface | Write | Read |
|---|---|---|
| CLI | `said learn-fix` | `said recall-fix` |
| MCP (for agents) | `learn_fix` tool | `recall_fix` tool |
| Orchestrator | `steps/learn.rs` (step 6) | `steps` step 0 (`recall::best_iteration`) |

A frame written by any one is byte-identical and recallable by all the others:

- **blake3 doc_id** (`fix::<16hex>`) — the canonical `.said` content hash, same as ingest/dedup/frame-checksums. (Orchestration's old FNV-1a hash was removed — divergent hashing is a latent footgun.)
- **Native Procedural pillar** — a coding-fix is an action-with-outcome, so it is stored via `remember_with_pillar(Pillar::Procedural, …)` (not merely tagged), so `recall_by_pillar(Procedural)` finds it. Tags carried alongside: `pillar:procedural`, `procedural:outcome=success`, `coding-fix`, optional `pr:<label>`.
- **Body layout:** `TASK: …` + the human note (FILES/STEPS/ERRORS/LEARNINGS or a full iteration note) + `FIX_EDITS_SEP` + the verified change-set JSON + `FIX_ACTION_SEP` + the intent residue. A companion `fixaction::<16hex>` frame holds only the action residue (intent fingerprint).

**Rule 2 holds:** the CLI and MCP are LLM-free. `recall_fix`/`learn_fix` are pure memory; an MCP agent (e.g. Claude) recalls a learning, drives **its own** LLM, and on a green gate stores the result with `learn_fix` — contributing to the same store the standalone orchestrator learns from. The LLM-driving loop itself is the separate `said-orchestrate` process (BYO-LLM key).

## The lifecycle (fixed order)

From [`steps/mod.rs:run`](../../crates/said-orchestration/src/steps/mod.rs) — one file per step, run in this order every time:

```
0. memory   — recall the most relevant VERIFIED iteration for this task (if any)
1. plan     — read-only exploration + approach           (Claude: plan mode)
2. design   — structure consistent with conventions      (Claude: design)
3. code     — produce + apply a change-set               (Claude: code)
4. test     — run the gate                               (Claude: verify)
5. repair   — on red, fix + re-gate (bounded loop)       (Claude: repair)
6. learn    — on green, author + compress + store it     (Claude: session memory)
```

The build/test gate is the **sole judge** of correctness. No step ever declares the work done — green only when the gate passes.

### Step 0 — memory (the moat)

`crate::recall::best_iteration` finds the best verified iteration for the task. When there is a genuine match, its **learning** (the approach, the gotchas, a reference implementation) is injected into the code/repair context as authoritative guidance the model **adapts** — never a pasted diff.

This is the strategic claim, and it is the thing that makes a weak model punch above its weight: **a fix is never byte-for-byte exact across codebases, but the learning is.** We transfer understanding, the model adapts it to *this* file, and the gate verifies. (See [the moat, proven](#the-moat-proven).)

### Steps 1–2 — plan / design (read-only)

Prose phases. `PLAN` and `DESIGN` in [`said-prompts`](../../crates/said-prompts/src/agents/coding.rs) are written in Claude Code's idioms: "do not propose changes to code you haven't read", "don't add features/refactors beyond what was asked", "the right amount of complexity is what the task actually requires."

### Step 3 — code (Claude-faithful apply)

The model emits a **change-set** (`{"edits":[...]}`), not free text. [`apply.rs`](../../crates/said-orchestration/src/apply.rs) applies it with the **exact contract of Claude Code's Edit tool**:

- **Exact + unique or fail.** An anchor must be an exact, unique substring of the current file, or the edit is *rejected* — no fuzzy/whitespace-collapsing matching that silently corrupts. A bad anchor errors cleanly and feeds the repair loop (mirrors Claude's "the edit will FAIL if old_string is not unique").
- **Line-number-prefix rule in the prompt, verbatim** from Claude's Edit tool ("Never include any part of the line number prefix in the anchor or content"). The numbered source is surfaced via [`source.rs`](../../crates/said-orchestration/src/source.rs) — Claude's "Read before Edit".
- **Edit modes:** `write-file` (whole function/class/small file — most reliable), `insert-after-text` / `insert-before-text`, `replace-text`. An anti-gutting guard rejects a `write-file` that would shrink a non-stub file by >50%.
- **No verbatim diff replay.** It was prototyped and removed: it only "passed" on an identical stub (a lookup table, not learning) and violates the never-paste thesis.

An apply failure is *repairable*, not fatal — it is fed to the repair loop like a gate failure.

### Steps 4–5 — test / repair

`test` runs the gate. On red, `repair` gets the gate error + the (re-read) current source + recalled errors-to-avoid, and emits a corrective change-set. Bounded by `--max-attempts`; the loop never merges red.

### Step 6 — learn

On green, the LLM **authors** a structured iteration note (Claude's session-memory move), `.said` **compresses** it, and stores it as a Procedural-pillar coding-fix frame. The next task recalls this whole story — the compounding answer base. The highest-leverage quality lever is here: the note must capture the **non-obvious invariant**, not a generic summary (see below).

## Recall: which learning, among many

`best_iteration` ([`recall.rs`](../../crates/said-orchestration/src/recall.rs)) delegates to **one shared scorer**, `sca_core::ask::best_coding_fix` ([`ask.rs`](../../crates/sca-core/src/ask.rs)) — the same scorer the CLI's `recall-fix` uses, so the two can never diverge.

It rides the documented [`ask` chain](03-core-subsystems/3.5-retrieval-pipeline.md) for the candidate neighborhood, then discriminates **within** it using `.said`'s own 1-bit hierarchical signals ([3.1 SCA engine](03-core-subsystems/3.1-sca-engine.md)):

- the **semantic fingerprint of the problem** (`rank_by_fingerprint`, pure-semantic route) — separates near-twins by *meaning*, not shared words;
- the **action/intent fingerprint** — isolated intent ("add X" vs "document X").

```
score = rel_ask_conf × (0.3 + 0.4·semantic + 0.3·intent)
```

This separates an LRU fix from an LFU fix whose text literally contains "tie-break by least-recently-used" — a case no lexical token-overlap method can split (they tie). The semantic fingerprint resolves it.

> **Requires the static encoder.** Semantic recall is dead without it: `build_index` indexes 0 fingerprints and `rank_by_fingerprint` returns nothing. The encoder is embedded by default (`embed-model`) in both `said-cli` and `said-orchestration` — the CLI prints `[SCA] Loaded embedded model`. Always confirm `said stats` shows `SCA docs indexed > 0` before trusting any recall number.

## Run it

```bash
said-orchestrate \
  --brain project.said \
  --repo /path/to/project \
  --task "Implement the LRUCache class per its spec; make node test/lru.test.js pass" \
  --files "src/lru.js" \
  --build "node test/lru.test.js" \
  [--test "..."] [--max-attempts 3] [--llm-config llm.toml]
```

Model is BYO via env (model-agnostic):

- Groq: `GROQ_API_KEY` (+ optional `GROQ_MODEL`)
- Any OpenAI-compatible: `OPENAI_API_KEY` + `SAID_LLM_BASE_URL` + `SAID_LLM_MODEL`
- Anthropic: `ANTHROPIC_API_KEY` (+ `ANTHROPIC_MODEL`)

Tuning env: `SAID_LLM_REASONING_EFFORT` (off/low/medium/high), `SAID_LLM_TEMPERATURE`, `SAID_LLM_PROVIDER_ORDER`, `SAID_RECALL_MIN` (confidence floor, default 0.45), `SAID_RECALL_DEBUG`, `SAID_APPLY_DEBUG`, `SAID_FIX_SCORE_DEBUG`.

Source files for the code/repair context are surfaced from `--files` (comma-separated, repo-relative). The orchestrator exits non-zero if the gate never goes green.

## The moat, proven

From [`hard-eval/RESULTS.md`](../../hard-eval/RESULTS.md) — measured, gate-judged, on the current binaries:

- **gpt-oss-120b cannot solve LRU cold** (RED, 5 attempts). It solves the other three hard tasks (intervals bugfix, TTL store feature, token-bucket from scratch) cold in 1 attempt — 3/4.
- After a verified LRU solution + its **learning** is in the brain, **gpt-oss-120b warm → GREEN, 1 attempt, ~13s — on a *perturbed* repo where a verbatim paste is impossible.** The model adapts the learning to the changed file; the gate confirms it.

**Same weak model, same hard task. Memory flips fail → pass — by transferring learning, not pasting a diff.** This is "portable memory ≈ RL-in-weights, but as a file, over any model."

### Learning QUALITY is the lever

The single variable that flipped the warm LRU run fail → pass was the **quality of the stored learning**, not the apply or the model:

- A *textbook* learning summary ("move node to MRU; evict from head") made the model write a textbook LRU that **fails the interleaved-stress test** (`4 !== -1`).
- A learning that captured the **non-obvious invariant** — a newly-`put` key that *triggered an eviction* inserts at the LRU side, not MRU — plus the textbook trap flagged as an error-to-avoid, made the model adapt it correctly and pass.

So the `learn` step's job is to extract the gotcha/invariant, not a generic restatement.

## How to test

```bash
# 1. Build with the encoder (default) and run the unit tests
cargo test --release -p said-orchestration            # apply/loop: 18 tests
cargo test --release -p said-prompts                   # prompts: 12 tests
cargo test --release -p sca-core --lib --features static-embed,code   # incl. the scorer's deps

# 2. Recall precision (no LLM): 1 real LRU fix + 9 vocabulary-overlapping decoys
bash hard-eval/recall-measure.sh        # expect 7/7 LRU queries + 5/5 decoys -> correct learning

# 3. End-to-end moat (needs an LLM key): cold RED -> warm GREEN on a perturbed repo
bash hard-eval/run-said.sh              # cold sweep, gpt-oss-120b: 3/4 (h1_lru red cold)
# then warm h1_lru with a learned brain -> GREEN, 1 attempt
```

## Known limitations

- **Scale of the recall proof.** 7/7 + 5/5 precision and the warm-LRU moat are measured on **small brains (≤10 fixes)**. The mechanism is correct; precision at thousands of records is the next validation.
- **Adversarial twins.** The semantic fingerprint separates the LRU/LFU case, but two genuinely-near-identical learnings can still be close; the confidence floor (`SAID_RECALL_MIN`) is the safety valve — a MISS falls through to the LLM with no injection rather than injecting the wrong learning.
- **Learning quality is manual-ish.** The `learn` prompt asks for the invariant, but whether the model reliably *extracts* the non-obvious invariant from any green run (vs. a human noticing it) is not yet measured at scale — the top product lever.

## See also

- [13 — Integrations](13-integrations.md) — the BYO-LLM rule (why this is a separate process)
- [3.5 — Retrieval pipeline](03-core-subsystems/3.5-retrieval-pipeline.md) — the `ask` chain the recall scorer rides
- [3.1 — SCA engine](03-core-subsystems/3.1-sca-engine.md) — the 1-bit semantic fingerprint the scorer discriminates with
- [04 — Four pillars](04-four-pillars/procedural.md) — where coding-fixes are stored (Procedural)
- [`hard-eval/RESULTS.md`](../../hard-eval/RESULTS.md) — the measured results this page summarizes
