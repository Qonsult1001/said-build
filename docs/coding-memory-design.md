# Coding Memory — design (v2, native pillars)

**Supersedes** the standalone-`fixcase` approach in `fix-replay-design.md`. This is
`.said`'s memory model applied to coding: a portable, single-file, self-improving
record of "problems seen + the fix that actually built+passed", consulted
**memory-first** before any code fix. The whole intent of `.said` (memory-first
recall via `ask`) — but for code.

## Principles (from the user)

1. **The user just wants a fix.** No ticket numbers in the human path. You describe
   a problem; the memory returns a verified fix if it has seen the shape before.
2. **Least possible calls.** Same surface as `said ask` / `said add`. `.said` does
   the work internally (splitting, scoping, ranking).
3. **Reference first, automatically.** Any code-fix flow checks the coding memory
   BEFORE reasoning — like `said ask` is the reflex for docs.
4. **Stay pure `.said`.** 1-bit fingerprints, single portable file. No Milvus, no
   full-float stores. The moat is portability + self-tuning from real outcomes.

## Build on native pillars — do NOT reinvent

`.said` already has exactly the right abstractions (see
`docs/said-structure/04-four-pillars/`):

- **Procedural pillar** = `TRIGGER → STEPS → OUTCOME`, auto-tag
  `procedural:outcome=success`, planned ranking `task_match(query,trigger) ×
  success_rate`. **A verified fix IS a procedural memory.**
  Writer: `remember_as_procedural(id, trigger, &steps, outcome, tags)`.
- **Code pillar** = AST-chunked source, lang/symbol/source tags.
  Writer: `remember_as_code(id, lang, body, symbol, path, tags)`.

So coding-memory = Procedural (the fix recipe) + Code (the touched symbols),
retrieved via the existing 3-engine `ask`.

## Learn (after green build+test only)

The gate stays the sole ground truth. On green:

```
remember_as_procedural(
  id   = auto (content hash),
  trigger = "<problem description>",        // the ticket/problem text, plain
  steps   = [ <change-set edits, one per step> ],
  outcome = "success — built+passed",       // ONLY recorded on green
  tags    = [ "lang:<l>", optional "pr:<n>" provenance, ... ],
)
// + the touched symbols' post-fix source → remember_as_code(...)
```

`pr:<n>` is OPTIONAL provenance metadata (used by the auto-pipeline for
traceability), never required from a human, never the lookup key.

## Recall (memory-first, automatic)

Before any fix attempt, the flow calls:

```
ask "<problem>"  scoped to  pillar:procedural AND procedural:outcome=success
  → highest task_match wins; if confidence ≥ threshold → return its STEPS (no LLM)
  → else → fall through to the LLM (today's behaviour)
```

**Intent separation (the breakthrough, see
`memory/fix-replay-intent-breakthrough.md`):** the procedural `trigger` match must
fingerprint the ACTION/intent separately from the TARGET nouns, or "document the
endpoint" matches "add the endpoint". Measured: whole-text 1-bit fingerprints
FAIL to separate intent; action-isolated 1-bit fingerprints SEPARATE cleanly
(+0.19 to +0.375 margins). `.said` derives the action field internally by
stripping identifier/code-shaped tokens (the target) and fingerprinting the
residue (verb-dominated). User passes one plain string; `.said` splits it.

## CLI / MCP surface (minimal)

Keep it `ask`-shaped. Options, simplest first:
- Reuse `ask` with a pillar filter the consumer sets, OR
- thin verbs that wrap the pillar writers/readers (names TBD; must be
  memory-flavored, not ticket-flavored).

Either way: **input is a plain problem description; output is a verified fix or
nothing.** `record-fix`/`suggest-fix` (0.9.0) become thin wrappers over
`remember_as_procedural` / scoped `ask`, with `--label` demoted to optional
provenance.

## Import (LOCAL FIRST this iteration; global later)

- **This iteration:** `said init <repo>` already AST-chunks a codebase into Code
  frames. Prove: init a repo → ask-for-fix recalls memory-first on that codebase.
- **Later:** merge external `.said` files (another project, a downloaded one) into
  the coding memory — "learn from working code globally". Portable because it's one
  file. Spec only for now.

## Non-negotiable division (all of it)

LLM = patch generator. Build+test gate = ground-truth oracle. Coding memory =
propose/rank only (returns a known-good recipe to TRY; the gate still verifies).
Memory can never emit unverified code as "done". This is what stops the loop
amplifying its own mistakes.

## What shipped (0.10.0)

The model is dead simple, per the user: **"it's a memory; the client does the
work; answers that work get stored; it's just another pillar, unique because
portable."** It is NOT a reasoning engine and NOT a precision classifier —
recall returns a verified fix when a workable one exists, else nothing (exactly
like Cursor/Claude memory). The differentiator is the GROWING verified corpus,
not ranking cleverness.

Shipped:
1. **`said learn-fix --problem <p> --edits[-file] <e> [--label <prov>]`** — stores
   a verified fix as a Procedural-pillar memory (TRIGGER→STEPS→OUTCOME=success)
   plus a companion action-residue frame for intent-isolated recall. `--label` is
   optional provenance, never the key. Stores ONLY verified (caller calls it after
   a green gate).
2. **`said recall-fix --problem <p> [--min-similarity 0.5]`** — returns the
   verified fix for a problem of this shape, or nothing. Scores by the PROVEN
   action-residue 1-bit fingerprint (intent) + target-token overlap. Reuses the
   native `ask` pipeline (Sym/AST + grep + SCA) to surface candidates.
3. **Intent separation** — action/target split (`sca_core::ask::action_residue`),
   PROVEN in `tests/test_intent_separation.rs` (+0.19/+0.375 margins). Pure 1-bit,
   no full floats, no Milvus.
4. **Incremental indexing works** — fixes learned across separate processes into
   an existing brain recall correctly (verified). This clears the Advisory
   `BLOCKED` note about incrementally-added frames not being retrievable.

Verified: 84 sca-core lib tests green; intent-separation test green; all 4 bundles
build zero-warning for said-cli + said-mcp; 2/2 real paraphrases recall the
correct fix end-to-end.

Known limitation (accepted — gate is the safety net): a "document the X" query
can score near an "add the X" fix when they share the exact target nouns. Harmless
in practice — the build/test gate + client verify any recalled fix; a wrong recall
costs at most one wasted build, never a bad merge. Quicker/perfect ranking is
explicitly NOT the goal; the growing corpus is.

## Next (not in 0.10.0)

- `said init <repo>` / ingest external code + other `.said` files as coding memory
  (the SCALE moat — a massive verified corpus any LLM can consult before
  generating). Retrieval already supports this (Code pillar + `recall_by_pillar`);
  it's a wiring/import job, not new machinery.
