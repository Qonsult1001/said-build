# Fixes log — recall/ranking correctness

Chronological record of correctness bugs found and fixed while proving `.said` as a market-leading
coding-agent memory at scale (large-project MCP/CLI A/B testing). Each entry: the symptom, the root
cause traced to the documented design, the fix, and how it was verified. All fixes are in the SHARED
`sca_core` path, so CLI and MCP (and any Rust caller) benefit identically.

---

## 1. Symbols silently unreachable after `init` — `sym()` returned 0 despite a full index

**Commit:** `d89d31c`

**Symptom.** A real `said init` over a large codebase (322 files) reported thousands of memories added,
but `sym build_concept_links`, `sym ask`, `sym main` — every exact-symbol lookup — returned **0
results**. `ask` for an exact function name fell back to fuzzy semantic and returned the wrong frame at
~0.55 instead of the real function at 1.00. This made `.said` look weak at scale and was the main reason
the first large-project A/B showed `.said` no better than grep.

**Root cause (traced via `docs/3.6-trigram-symbol-index.md`).** The documented design: the `SYMS`
section stores positional `doc_index` values, and `sym()` translates them back to `doc_id` via the
`trigram_doc_ids` list — which is only populated from the **`TRGM`** section on open. `save()` wrote
`SYMS` but **skipped `TRGM`** whenever the trigram postings came out empty (frame text wasn't yet
readable at the moment `rebuild_trigram_index` ran). On reopen `trigram_doc_ids` was therefore empty,
so every symbol's `doc_index → doc_id` translation failed and `sym()` returned nothing — even though the
symbol index itself loaded fine (`symbol_count` was correct).

Confirmed at the byte level: the saved file had the `SYMS` marker but the header `trgm_offset` was `0`
and there was no `TRGM` section.

**Fix.** `save()` now writes the `TRGM` section (carrying the doc-id list `sym()` depends on) whenever a
symbol index is present (`symbol_index` with `num_names() > 0`), not only when trigram postings are
non-empty.

**Verified.** Re-indexing the full 322-file `.said` codebase: `sym(build_concept_links)=1`, `sym(ask)=8`,
`sym(decide)=4` (all were 0); `ask("build_concept_links")` now returns `[1.00][symbol]`. Binary
regression 15/15. A reproducing unit test was used to isolate it (`compact → save → reopen → sym` went
from 0 to 3).

---

## 2. Coincidental common-word symbols outranked the real answer (section-(a) ranking)

**Commit:** `5e27424`

**Symptom.** A descriptive query — "which function builds the wikilink concept graph from frames" —
returned the trivial symbol `frames` at confidence **1.27**, burying the real `build_concept_links`.
Same failure for "the abstention gate in ask" → symbol `threshold`; "the steering hook" → symbol
`steering`. Any descriptive question containing a word that happens to name a symbol was hijacked by
that coincidental match.

**Root cause (traced via `docs/3.5-retrieval-pipeline.md`).** Engine A gave **every** symbol-name match
a flat confidence `1.00` and the rerank pinned all symbol hits above the semantic rerank (the doc states
"Sym hits stay pinned above the semantic rerank"). That assumes a symbol hit is always intentional. It
also let an additive grep bonus push the fused score to 1.27 — above the symbol ceiling. Engine B
already documents the correct principle for its own scoring (do NOT treat rare common-English words as
discriminators — they coincidentally land in the wrong doc and out-rank the correct semantic match);
Engine A simply lacked the analogous guard.

**Fix (structural + corpus-derived — NO hard-coded stopword list).**
- `symbol_distinctiveness(name, query_len)`: a **compound identifier** (snake_case / camelCase hump /
  digit / ≥12 chars) stays at `1.00`; a **short single lowercase word** that is one token inside a
  multi-word descriptive query is **discounted into the grep band** (~0.55–0.92). A one-word query that
  literally is the identifier keeps full confidence.
- The symbol+grep fused confidence is **capped at 1.0** (was reaching 1.27).
- The rerank pins **only distinctive** symbol hits (confidence ≥ 0.95); a discounted symbol competes on
  cosine like any candidate, so a stronger semantic answer can lead.

**Verified.** Exact-symbol queries unchanged — `build_concept_links` / `frames_linking_concept` /
`rebuild_trigram_index` all still `[1.00][symbol]`. "The function that returns frames linking to a
concept" now returns `[1.00] frames_linking_concept` (was a coincidental symbol). The 1.27 inflation is
gone. Binary regression 15/15; `test_recall_at_volume` still passes.

**Known remaining (honest).** Two purely-descriptive phrasings ("builds the wikilink concept graph from
frames", "the threshold-free abstention gate") still surface the coincidental word because the float
rerank only fires when a `semantic`-kind candidate is present; when the candidate set is all
symbol/text, the discounted symbol can still lead. Widening the rerank gate to cover this is tracked as
a follow-up — it is a ranking-quality gap, not a correctness bug, and does not affect exact-symbol or
the multi-keyword descriptive cases that now resolve correctly.

---

## CLI vs MCP — one shared path

Both the CLI (`said ask`) and the MCP `ask` tool call the same `sca_core::ask::ask` (said-mcp
`handler.rs`). There is **no separate MCP ranking path** — every fix above applies to both. An earlier
appearance of an "MCP-only" problem was a test harness passing the wrong argument name (`question`
instead of the tool's `query` field), which returned an error, not a different ranking.

---

## 4. Doc-comments excluded from indexed code chunks

**Commit:** `85ffff4`

**Symptom.** A descriptive query — "the function that builds the wikilink concept graph" — could not
retrieve `build_concept_links` AT ALL (absent from the top-20), even though that function's doc-comment
literally describes building a wiki/concept GRAPH. Grep for the doc-comment's words ("navigable graph",
"cross-document map") returned nothing.

**Root cause.** `ast_chunk` indexed only a definition's own tree-sitter byte range. But `///` / `//!` /
`/** */` doc-comments are SIBLING comment nodes BEFORE the definition, not part of it — so the single
richest natural-language description of what each function DOES was never indexed.

**Fix.** Walk backwards over the contiguous preceding comment siblings (no blank-line gap) and prepend
them to the chunk content; index the chunk from the first doc-comment line.

**Verified.** `build_concept_links` went from absent-in-top-20 to retrievable + grep-able by its
doc-comment vocabulary (now in the top-N the LLM picks from). Unit test + regression 15/15.

**Design note (the rerank correction).** Getting the answer to RANK #1 inside the engine was the wrong
goal. Per docs/3.5 + docs/13: `.said` surfaces the top-N and the LLM reranks by reading back (the
interactive loop's implicit rerank). The bar is "in the top-N", which the doc-comment fix met.

---

## Known gap — opt-in LLM rerank is documented but unimplemented

`docs/13-integrations.md` (line 112) specifies that headless single-shot consumers get a
LongMemEval-grade benefit from an opt-in LLM rerank: **CLI `said ask --rerank`** and **MCP `ask` with
`rerank: true`** — the LLM picks/reorders the top-N when there's no agent loop to do the implicit
read-back rerank. **Neither is implemented.** Deferred (the top-N contract is sufficient for agent-loop
consumers, which do the rerank by reading back). Tracked here so the doc and code are honestly
reconciled.

---

## 5. (RESOLVED) learn_fix recall dead end-to-end via MCP — the MCP server shipped without the encoder

**Symptom.** On a brain built by the real CLI `said init` (any size — reproduced at 37, 175, and 4,388
frames), `rank_by_fingerprint(q)` returns **0 hits** and `ask` never emits a `[semantic]` result (only
`[symbol]`/`[text]`). Downstream this kills coding-fix recall: `best_coding_fixes` scores
`rel_conf * (0.3 + 0.4*semantic + 0.3*intent)`, so with `semantic=0` and `intent=0` the max score is
**0.30 — permanently below the 0.45 recall floor**. Every `recall_fix` / MCP `recall_fix` / orchestrator
memory-recall therefore returns "No known fix" on a real codebase. THIS is why learn_fix doesn't work
end-to-end.

**What it is NOT (8 controlled tests, all GREEN — the library is sound):**
- NOT incremental indexing: a fix added incrementally to a 500-frame code brain fingerprints at 0.87.
- NOT reopen: SCRM survives save→reopen (0.616→0.616; 0.87→0.87; recall_coding_fixes 0.893 after reopen).
- NOT scale: `rank_by_fingerprint` returns 0.56–0.65 at N=5/50/500/2000 in-process.
- NOT the fix vs code-mean hypothesis: full rebuild gave the same scores as incremental.
- `build_index_with_progress` (the chunked path init uses) + save + reopen works in-process.

**What it IS.** Something the CLI `said init` does differently from an in-process
`remember_as` + `build_index_with_progress` + `save`. The verbose init prints "[2/3] Encoded (SCA):
100% (0.0s)" — the 0.0s for real encoding is suspect. Leading hypotheses to check next: (a) the encoder
is not actually loaded at the moment `cmd_init` calls `build_index` (try_load_encoder ordering vs the
brain instance that gets indexed), or (b) the chunked-init branch encodes but does not persist the SCRM
section the way the in-process path does. NOT yet fixed — diagnosed and narrowed.

**Impact.** High: it silently disables the entire SCA semantic engine for CLI-built brains, so .said
falls back to symbol+grep only. Matches the historical "embed-model opt-in → 0 fingerprints" failure
class. The fix belongs in `said-cli::cmd_init` / `try_load_encoder` (or the chunked branch of
`build_index_with_progress`), proven by: after `said init`, `rank_by_fingerprint` must be > 0 and `ask`
must emit `[semantic]` hits.

**RESOLUTION (commit 895706c).** Two findings: (1) the earlier "dead fingerprints" brains were built by
a stale/mis-featured binary — a current `coding`-bundle CLI build produces live semantic ([0.83]) and the
full confirmed-fix loop works (recall-fix → 0.82). (2) The actual end-to-end MCP failure: `said-mcp`'s
default features OMIT `embed-model`, so a plain `cargo build -p said-mcp` ships a server with no baked-in
encoder → semantic dead → recall_fix always "No known fix" (while the CLI recalled the same fix at 0.82).
Fixed: `attach_encoder` now warns loudly once when no encoder loads, naming the fix (rebuild with
`--features coding`/`full`). Verified end-to-end through the MCP server: recall_fix returns the stored fix
(trgm-fix → ROOT CAUSE → TRGM). Build requirement: ship said-mcp with a bundle that includes embed-model.

---

## 6. (RESOLVED) Fix-recall starved by a coincidental symbol hit — violated the 14.3 SCA-survival guarantee

**Commit:** `fa7a99d`

**Symptom.** A verified coding fix that recalled fine on its exact problem text returned "No known fix"
on a PARAPHRASE — even though `brain.query` (SCA semantic) surfaced the fix at 0.71. On the live A/B this
made `recall_fix` miss on paraphrased questions (A2/A5), so the agent investigated from scratch (the cost
that held the accumulation benchmark flat).

**Root cause — a documented contract was violated.** `docs/14-novel-mechanisms/14.3-relative-cutoff.md`
guarantees: *"the top-3 semantic hits always pass the cutoff even when a symbol or trigram hit
dominates"* (ASK_SCA_GUARANTEED=3). Two things broke that for fix recall:
1. `best_coding_fixes` called `ask(deep=false)`. The non-deep abstention block (ask.rs ~794) drops the
   semantic tail when ANY "confident" hit exists — and a COINCIDENTAL symbol match (query word "save" →
   a `save` symbol) counts as confident. So the semantically-recalled fix frame was discarded AFTER the
   guaranteed-survival cutoff — i.e. the abstention block (added later) overrode the 14.3 guarantee.
2. `rel_conf = conf/top_conf` was a raw multiplier; in deep mode the float rerank can zero a fix frame's
   ask confidence (its stored text differs from the query), so `rel_conf=0` nullified strong
   semantic+intent fingerprints → final score 0.

**Fix (restores the documented contract).** `best_coding_fixes` asks with `deep=true` (the fix-recall
path needs the full semantic-led pool to filter by `FIX_KIND_TAG`, then re-scores by fingerprints — it
must not be subject to end-user abstention trimming, which is what the 14.3 guarantee protects). And
`rel_conf` is floored at 0.5 (membership in the fix-filtered set IS the spine signal; the fingerprint
discriminators decide; a strong spine still ranks higher).

**Verified.** Paraphrased fixes that returned "No known fix" now recall (A5-style 0.76; A2-style found
at 0.34, semantic 0.49 + intent 0.61, up from absent). Adversarial decoy separation preserved (LRU
target leads LFU/TTL, `hard-eval/recall-measure.sh`). Regression 15/15; recall canary green.

**Note.** A heavy paraphrase whose SPINE (ask) signal is weak can still land just under the default 0.45
`SAID_RECALL_MIN` even with strong fingerprints (A2 = 0.34). That is the top-N-vs-hard-floor tension: the
orchestrator injects top-k=5 and lets the LLM pick, so a borderline-but-correct fix still reaches the
model. A calibrated/lower fix floor for paraphrase is the remaining tuning lever, tracked, not a
regression.
