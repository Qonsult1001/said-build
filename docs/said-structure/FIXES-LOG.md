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
