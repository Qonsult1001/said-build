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

---

## 7. (RESOLVED) A2 paraphrase miss — the gate was the bug, not the matcher (per-query distributional gate)

**Commit:** `6cbdd9f`

**Symptom.** A paraphrased fix query ("which save function writes the section so sym survives reopen")
recalled the CORRECT fix at rank-1 but at a moderate score (0.34), below the absolute recall floor, so
`recall_fix` returned "No known fix" and the agent investigated (the A2 cost in the v5 A/B).

**Root cause (docs + arXiv).** The bug is the GATE, not the matcher — the right fix is already rank-1.
A fixed absolute cosine/score floor is the wrong gate because embedding spaces are anisotropic (vectors
in a narrow cone, raw cosines concentrate and aren't comparable across queries — SimCSE arXiv:2104.08821;
cross-query non-comparability / QB-Norm arXiv:2408.04887). 0.34 is not "low" in any absolute sense. This
is the exact enhancement already noted in docs/11-known-limitations §dynamic cutoff: "if top is 0.5 and
rank 2 is 0.49, widen."

**Fix.** `recall_coding_fixes` gates on a PER-QUERY DISTRIBUTIONAL test: keep a candidate if it clears
the absolute floor (unchanged) OR it is the top candidate that STANDS OUT from the rest of the fix pool
(gap-to-rank-2 ≥ 0.6 of the pool spread). Relative to each query's own spread — no hard-coded magic
number (consistent with the project's no-magic-threshold rule).

**Decoy-safety by construction.** When two near-identical fixes are present (the LRU/LFU twin case), the
top does NOT stand out → the absolute floor remains the gate → twin discrimination preserved. Verified:
A2 recalls (lone standout 0.34); A5 still 0.76; LRU target leads LFU/TTL (recall-measure.sh); off-topic
kubernetes query still abstains; regression 15/15; recall canary green.

**Research backing (full set):** the gate-not-matcher framing + ranked alternatives (HyDE 2212.10496,
Query2doc 2303.07678, doc2query 1904.08375, ANCE-PRF 2108.13454, RankGPT 2304.09542, anisotropy
2104.08821, QB-Norm 2408.04887). The distributional gate is the cheapest, fully-offline, no-LLM,
no-magic-number option; doc2query (learn-time question expansion) is the tracked follow-up to RAISE
scores, and top-N + LLM rerank is the optional-LLM ceiling.

---

## 8. (RESOLVED) Repeated `learn-fix` on a large brain corrupted PRIOR fix bodies

**Status:** root-caused + minimal repro; fix not yet applied (core storage path — needs care).

**Symptom (minimal repro).** On a large init'd brain (~4,387 frames):
```
learn-fix #1 → fix::6eb6… , get → 266 chars         (body present)
learn-fix #2 → fix::…     , get #2 → 257 chars       (body present)
get #1 again → 0 chars                                (FIRST body now EMPTY)
```
A second `learn-fix` blanks the first fix's body. With 5 seeds, only the LAST retains a body — so
multi-fix brains effectively hold one recallable fix. Does NOT reproduce on a small/empty brain (2
sequential learn-fix both keep bodies) — it's scale/incremental-path specific.

**Impact (this is the bug under the benchmarks).** Every accumulation A/B silently tested a brain where
4 of 5 seeded fixes had empty bodies → "A2/A3 recall failures" and the "correctness regression" were
mostly this, not the recall/gate logic. It also means real users who `learn-fix` repeatedly on a real
(large) codebase brain lose all but the most-recent fix's body. High priority.

**Root cause (hypothesis, strong).** `learn_coding_fix` does `remember_with_pillar` → `build_index()` →
`save()`. `build_index`'s incremental path reads frame bodies via
`frames.read_frame_text(doc_id, self.data.as_slice())` — i.e. from the mmap of the LAST-SAVED file. A
newly-added frame's body lives in the pending/spill buffer, not yet in `self.data`; after the next
`save()` + the following `learn-fix`'s incremental `build_index`, a prior fix frame's body is read from a
stale/!current `self.data` and re-indexed (and re-saved) as empty. The frame's doc_id + tags survive
(they're in metadata), but the body is lost — exactly the observed signature (doc_id recallable, `get`
empty).

**Next step.** Reproduce in a Rust unit test (two `learn_coding_fix` on a >50-frame brain, assert
`get` of the first is non-empty after the second), then fix the body source in the incremental
build/save path (ensure pending/spill bodies are flushed into `self.data` — or re-read from the spill —
before the next incremental index). Until fixed, batch fix-seeding must `compact()`/full-rebuild between
adds, or store all fixes then `build_index` once.

**CONFIRMED ROOT CAUSE (2026-06-27).** Reproduced reliably at scale + isolated to the BLOCK-COMPACTED
save path (a `said init` brain is block-dict compacted, so `SaidFile::save()` takes the
`self.frames.has_blocks()` branch, NOT the uncompacted one). That branch calls
`self.frames.flush_block_pending(buf.len())` (said_file.rs ~2354 and ~2567), which delegates to
`flush_block_pending_with_source(base, &[])` — **with an EMPTY source slice.** In
`flush_block_pending_with_source` (frames.rs ~880), an EXISTING (already-persisted) block can only be
copied forward by reading its bytes from `source_data`; with an empty source it hits the
"can't recover this block" branch and the block (carrying the prior fix's body) is **silently dropped**.
So every re-save of a block-compacted brain loses pre-existing block bodies that weren't re-pending.

**ATTEMPTED FIX + WHY IT'S NOT YET DONE.** Passing `self.data.as_slice()` (cloned to avoid the
frames/data borrow conflict) as the source REGRESSED 1/5→0/5 and surfaced a SECOND, independent bug: a
`task_identity` doc_id COLLISION (5 distinct problems → 2 ids; whitespace-free labels should be distinct
but the stored ids collide). The two bugs are tangled in the hottest serialization path, so the change
was REVERTED rather than ship a half-fix. The correct fix must: (1) pass a valid source to
flush_block_pending so existing blocks copy forward, AND (2) fix the doc_id collision so distinct fixes
get distinct frames — with the #[ignore]'d guard test (test_learnfix_body_corruption_8) going green.

**SAFE WORKAROUND until fixed.** Seed multiple fixes by storing ALL of them, then `compact()` +
`build_index()` + `save()` ONCE at the end (a full rebuild re-encodes every body), instead of
save-per-fix. Or store fixes on a SMALL brain (no block compaction) and merge. The verify-bodies gate
(get each fix, assert non-empty) MUST run before trusting any multi-fix brain.

**RESOLVED (commit pending).** The FINAL root cause was narrower than the first hypothesis: it was NOT
only that `flush_block_pending` passed an empty source (that path drops pre-existing BLOCKS, fixed by
passing `self.data`). The real miss was COMMITTED NON-BLOCK frames: a fix added by `learn_coding_fix`
AFTER the brain was block-compacted is stored as a Plain (inline) frame; once saved it becomes a
COMMITTED frame that is NEITHER in a block NOR in `pending`. The block-save path
(`flush_block_pending_with_source`) only re-emitted blocks + pending, so every committed inline frame's
body was dropped on the next save — that is what blanked the prior fix. Fix: (a) `save()` passes
`self.data` as the block source, and (b) `flush_block_pending_with_source` now also copies forward every
committed non-block frame from `source_data` at its offset (updating the offset), before moving pending →
committed. The earlier "task_identity doc_id collision" was a MISDIAGNOSIS — a display artifact of the
corruption; `task_identity` gives distinct ids for distinct labels (verified). Result: CLI 5-fix store →
5/5 bodies present + 5 distinct ids (was 1/5). Guard test now GREEN and RUNS IN CI (un-ignored). Binary
regression 15/15, recall canary green, save/admin-restore/persistence integrity tests green.

**Guard test:** `crates/sca-core/tests/test_learnfix_body_corruption_8.rs` (#[ignore], run with
`-- --ignored`) — currently RED, self-builds a block-compacted brain, asserts fix#1 body survives fix#2.

**Benchmark note.** All accumulation correctness numbers (docs/20 v3–v5) are SUSPECT because of this —
the cost numbers (memory cheaper) are less affected (they measure agent behavior given whatever was
injected), but a clean re-run requires this fix or a single-batch-index seeding path.

---

## 9. (RESOLVED) Non-deterministic `ask` ranking — same query returned a different memory each run

**Commit:** `c0ba138`

**Symptom.** On a brain of short one-line personal memories, the SAME query returned a DIFFERENT top
memory on repeated runs, and the correct answer was often dropped before the rerank could fix it
(measured recall@1 ~20–33% on 100 short memories, unstable between processes).

**Root cause.** The candidate set is assembled in a `HashMap`; the final ranking sort was by
`confidence` with NO tie-break. Ties are extremely common for short memories (many score identically),
so ties resolved by the HashMap's per-process-random iteration order — a coin-flip each run.

**Fix.** A deterministic `doc_id` tie-break on EVERY ranking sort in `ask.rs` (merge, seed, blueprint,
fix, by_spine). Proven recall-SAFE: at 400 memories it only reorders exact ties (r@5/r@10 unchanged vs
the parent commit) — the correct deterministic trade. **Guard test:**
`crates/sca-core/tests/test_recall_determinism.rs` (same query → same top hit ×15).

---

## 10. (RESOLVED) OKF single-word entities regressed recall — common sentence-openers became concept hubs

**Commit:** `c0ba138` (superseded the first attempt in the same commit's earlier form)

**Symptom.** After adding single-word proper-noun extraction so personal memories link on names
("Rotterdam", "Carol"), recall REGRESSED at volume: multi-hop r@5 1.00→0.50, preference r@5 0.83→0.67,
paraphrase r@10 0.85→0.80 (3 of 10 categories in the 400-memory recall-quality test failed their gates).

**Root cause (measured before/after against the parent commit).** The first single-word extractor took
ANY capitalised word (≥4 chars, not in a stop list). Common sentence-OPENERS ("Region…", "Meeting…",
"Migrated…") are capitalised only because they start a sentence — they became spurious concept hubs that
over-bridged unrelated memories, so Engine-D pulled wrong siblings into the top-K and displaced correct
answers.

**Fix.** `extract_entities_memory` now captures a single-word proper noun ONLY when it appears
MID-SENTENCE (the previous token did not end a sentence). A name a user links on sits inside a sentence
("in Rotterdam", "hired Carol"); a common word capitalised only as a sentence-opener never does. Purely
positional, no dictionary, deterministic. Restored all 10 categories to parent-level; live MCP:
"rotterdam" links 2 memories, concept list clean. Routed to personal-memory frames only (deny-list
`!ingest: && pillar!=External`); code/ingested frames keep the multi-word extractor byte-identical.
**Gate:** `test_recall_quality_volume` (10/10 categories at 400).

---

## 11. (RESOLVED) Temporal caveat — relative-time queries ("last quarter/year") didn't recall at top-1

**Commit:** `bd3503b` (feature) + `<pending>` (centralised the write chokepoint)

**Symptom.** "What did I do last year / last quarter" recalled the right memory only in the top-3, not
top-1 — the engine has no clock, so it never resolved "last year" to an absolute date; recall matched on
generic "I did" semantics.

**Root cause + research.** Verified the leaders' approach in the LOCAL Mem0 source
(`G:\development\SAID-ECHO\research\mem0`, not just docs). The proven win (Mem0 `configs/prompts.py`:
"Always ground relative references to specific dates"; LoCoMo 86→90, LongMemEval 90→95) is WRITE-TIME
grounding — resolve relative phrases to ABSOLUTE dates IN the stored text so plain semantic recall finds
them — NOT a query-side resolver. Query-time date-math the answering LLM does for free (Claude reads the
top-K). Zep/Graphiti's bi-temporal graph is the enterprise version, overkill for the free brain.

**Fix.** `time_compat::ground_relative_dates(text, y, m, d)` — deterministic, no-LLM, `today` passed in
(pure/reproducible; `today_ymd()` reads the clock only at the caller boundary via a self-contained
`civil_from_days`, no date crate). Resolves last year / this year / last quarter (wraps Q1→Q4 prior
year) / last month (wraps Jan→Dec prior year), appends `(around <date>)`, additive + idempotent, unknown
phrasing untouched. Applied at the single memory-write chokepoint `remember_with_salience` (so no caller
path can bypass it) for MCP, and at the CLI `add` layer for the CLI (which uses `remember_as`). **Proven:**
"Last quarter I shipped the release." → "(around Q2 2026)"; live MCP "last year/month" → @1 (was @2);
"certified in 2025" / "Q2 2026" → @1. 9/9 grounding unit tests
(`test_temporal_grounding.rs`); 400-memory recall (10/10) + determinism unchanged.

---

## 12. (OPEN — root-caused, localized) MCP write-time grounding not persisted in a large single-session batch

**Status:** root-caused + localized to the block-compaction save path; fix deferred (hot serialization
path — same care as #8; a naive fix there regressed once, so this needs a guard test first).

**Symptom.** In the 1000-record CLI-vs-MCP benchmark, temporal grounding (#11) lands correctly on the
CLI (verified on disk) but on the MCP surface the grounded body is NOT on disk after the full 1000-write
session — so MCP temporal recall was measured on UNGROUNDED text (invalid; the CLI temporal numbers are
the valid ones). The grounding FEATURE is correct: it lands in every isolated test (single write, 31st
write, 200 filler, full gold set) and in live single MCP `remember`s — it only fails to persist in the
large single-session MCP batch.

**Root cause (localized via `[DEBUG-tg8]` after-save probe).** The MCP `handle_remember` calls
`save()` after EVERY write (1000 saves in one session). Above a spill/block-compaction threshold
(reproduces past ~200–1000 frames, not below) the block copy-forward path
(`save()` → `flush_block_pending_with_source`, said_file.rs ~2455) drops the freshly-grounded body — the
SAME family as #8 (committed/blocked bodies lost on re-save from a stale block source). Smoking gun: the
Heisenberg probe itself FIXED it — adding a `brain.get("tr_0")` (a `read()` → mmap access) between saves
made grounding persist on disk, because the intervening read refreshes state the next block-save reads
from. CLI is immune: it's a fresh process per `add`, so it always re-mmaps.

**Next step.** Reproduce in a Rust unit test (MCP-style save-per-write past the spill threshold, assert a
grounded frame's body survives), then fix the block-source bookkeeping in the save path (ensure the
just-saved frame bodies are the source for the next block copy-forward without needing an external read),
un-ignore the guard. Until fixed, MCP temporal-heavy corpora should batch writes then index/save once
(as #8's workaround), or the temporal grounding for MCP can be trusted for interactive single writes
(the real usage) — the failure is specific to a 1000-write scripted single session.

---

## 13. (RESOLVED) Rare exact-term match dropped from top-10 when common words diluted the query

**Commit:** `<pending>`

**Symptom (found on the wiki-link MCP benchmark, 95% not 100%).** "cardiologist" alone → the one note
containing it at rank 1 (score 1.00). But "who is our **lead** cardiologist" → that same note DROPPED OUT
of the top-10 entirely, and a filler note tied at ~0.65 while an unrelated memory scored ~0.81. Adding
common words to a query erased the rare-token exact hit.

**Root cause (traced with a fast Rust repro, `test_rare_term_dropout`).** The lexical `text` score for a
doc matching all query terms is `shared = 0.40 + 0.15·(terms_present−1)`. A full **2-term** match scores
EXACTLY **0.55**. The semantic-rerank gate is `let lexically_discriminated = kept.iter().any(|c| c.kind
== "text" && c.confidence > 0.55)` (ask.rs ~713) — strictly **greater than** 0.55. So a maximally-rare
full match (rarity=1.00 "cardiologist") landed at exactly 0.55, was NOT counted as lexically
discriminated, the set was treated as semantic-led, and the full-float rerank overwrote the strong
lexical hit with a ~0.0 whitened cosine — pushing the correct gold below filler/off-topic and out of
top-10. Deep mode skips the rerank/abstention, which is why the gold was rank 1 in `deep` but gone in
normal mode (the diagnostic that localized it). The `plant`-once unit corpus didn't trigger it; the
per-write incremental-index path (CLI/MCP) did — so the repro indexes per write.

**Fix.** A COMPLETE-MATCH rarity bump (ask.rs): when a doc matches EVERY query term (`terms_present ==
keywords.len()`, ≥2 terms) AND one term is rare (`rarity ≥ 0.85`), lift `shared` by `0.10·rarity`
(capped 0.80). This clears the `> 0.55` rerank gate so a genuine complete lexical match keeps its lead,
while staying well BELOW the 0.95 identifier band — it can only lift a COMPLETE match, so it can't
mis-fire on a paraphrase where the rare word lands in a doc missing the other terms (the failure mode the
identifier-gating comment warns about). **Verified:** `test_rare_term_dropout` GREEN (was RED, gold
rank -1 → in top-10); 400-memory recall-quality (10/10 categories), determinism, and 9/9 temporal
grounding tests all still green (zero regression). Ground truth on the saved benchmark brain: a CLI read
returns the gold at rank 1.

**Guard test:** `crates/sca-core/tests/test_rare_term_dropout.rs`.

**Related (separate, still open).** The wiki-link MCP benchmark still shows the miss when it queries
IN-SESSION immediately after the writes (before the brain settles to disk) — the saved brain (what a
real user reopens; verified via CLI + reopened MCP session) has the gold at rank 1. That in-session vs
saved-state ranking inconsistency is a distinct low-impact concern (real usage reopens), not this fix.

---

## 14. (RESOLVED) Abstention gap-filter hid a real answer from the LLM-picks contract

**Commit:** `<pending>`

**Symptom.** At 4000 memories, 2 of 10 preference queries returned **only 1 result** and the correct
memory was ABSENT: "what car do I drive" → 1 filler note (gold "My car is a blue Toyota Corolla" gone),
"what did I study" → 2-3 filler (gold "I studied marine biology" gone). Widening `--top` to 20/100/500
did nothing — still 1-2 results.

**Root cause (traced with the built-in env A/B knobs — no code change to diagnose).** The gold was NOT
lost: `--deep` (and `SAID_ASK_ABSTAIN=0`) returned it at **rank 10**. Isolating the three abstention
sub-filters (`SAID_ASK_FLOOR` / `SAID_ASK_GAP` / `SAID_ASK_ZMIN`) pinned it to the **GAP filter**
(ask.rs ~865): it drops semantic hits trailing the leader by more than `gap` (0.20). The weak static
encoder (doc 3.2) scored **9 filler notes ABOVE the gold** ("what car do I drive" is closer in the
mean-pooled 128-dim space to "internal survey record N" than to "My car is a Toyota" — the gold sat at
1.35 while filler led at 2.51), so the gold trailed the leader by far more than the gap and was cut,
collapsing the returned list to 1. That HID a real answer that was sitting in the top-10 from the LLM —
directly violating the documented **TOP-K + LLM-picks** contract (14.15-canon-memory.md: "rank@1 was the
wrong metric… top-K-then-model-picks is"; 3.5 + FIXES-LOG #7: ".said surfaces the top-N and the LLM
reranks by reading back").

**Fix (design-aligned, NOT encoder/lexical tuning).** The gap filter now PRESERVES the top-K window: it
may only trim the tail BEYOND the requested `top`, never cut a candidate WITHIN the first `top`. So a
real answer the encoder ranked low still reaches the LLM's context, which reads all K and picks it. The
gap keeps its legitimate job (trimming the "returns the whole brain" tail past top-K). Crucially the
**negative/existence abstention is unaffected** — that comes from the z-score + threshold-free shape
gate, NOT the gap (verified: 400-gate negative/existence stays 1.00 with the gap fully relaxed).

**Verified.** pf_6/pf_8 now return in the top-10 (rank 10) at 4000. **Preference r@10 80% → 100%**, so
**all 6 categories PASS at 4000 on both surfaces** (CLI overall 77/97/**100**; MCP 75/97/97, ask 30ms).
400-memory recall-quality 10/10 (negative/existence 1.00), determinism, rare-term, temporal all still
green — zero regression. This is the correct fix for the preference gap: return the top-K, let the LLM
pick — NOT lexical/stem/bump tuning (which traded single-hop twins; reverted earlier this session).

---

## 15. (RESOLVED) Production surface gaps — CLI config no-op, MCP param inconsistency, MCP no help / description leaks

Found by a full command/tool sweep of the shipped brain binaries against a real 100-memory brain
(exercise EVERY CLI command + EVERY MCP tool, not just the happy path). Four surface defects — none a
recall bug, all "the product surface isn't production-clean":

**15a — CLI `config` was a no-op stub.** `said config <k> <v>` printed "Set k=v" but stored nothing;
`said config <k>` always printed "(not set)". The function had a "for now… stored in a simple file"
comment and never wrote/read anything. **Fix:** `resolve.rs` gained `set_config`/`get_config`/
`list_config` (persist a key→value map to `config.json` in the config dir, same dir as the `use`
default), and `cmd_config` now calls them. Verified: set in one process, get in a SEPARATE process
returns the value; no-arg lists all keys.

**15b — MCP param inconsistency (`name` vs `doc_id`).** `get`/`delete` took `doc_id`, but
`history`/`checkout` took `name` for the same thing (a memory's id). A caller switching tools hit
"missing field `name`" / "missing field `doc_id`" errors. **Fix:** `history`/`checkout` now take
`doc_id` (consistent with the rest), with `#[serde(alias = "name")]` so legacy `name` calls still work.
Handler updated `t.name` → `t.doc_id`. Verified: both `{doc_id:…}` and `{name:…}` work; schema
advertises `doc_id`.

**15c — `said-mcp --help` produced NOTHING.** The MCP binary is a stdio server, so running it by hand
just blocked on stdin — a human had no way to see what it is or what tools it exposes (the CLI has a
21-command `--help` menu; MCP had nothing). **Fix:** `main.rs` handles `--help`/`-h` (prints a
brain-tier menu of the 11 tools + connect example) and `--version`/`-V` BEFORE starting the server.

**15d — MCP tool descriptions leaked the coding tier into the free brain.** `ask`/`get`/`history`/
`checkout`/`status` descriptions said "code", "symbol", "project's", "call-graph", "LSP", "SQL", "grep"
— copied from the coding build and never reworded for the memory-only brain. The agent reads these
(they ARE the tool contract), so it both confused the agent and leaked paid-tier framing into the free
product (the same leak the MCP *instructions* were already guarded against — but the *tool descriptions*
weren't). **Fix:** reworded to tier-neutral, memory-accurate language ("Find memories by meaning", "Read
the exact text of a memory by its id", "version history of a memory") that's true for every build.
Verified: `tools/list` has ZERO leaks; all 11 tools still functional.

**Lesson → the global standard (see [production-surface-parity](#production-surface-parity-the-standard)
below).** Recall correctness is necessary but not sufficient for a production deliverable: the CLI and
MCP are TWO surfaces of one product and must reach parity — human-facing `--help` on both, tier-accurate
descriptions, consistent parameter names, and NO stub commands. Sweep every command/tool against a real
brain before shipping.

## 16. (RESOLVED) Status mislabel, code-tier noise on brain, `admin audit` schema gap, tags write-only

Four defects found by driving the real **brain** build (MCP + CLI), all shipped in the same session:

1. **`status` said "Search index: absent" on a healthy brain.** The field was keyed off
   `trigram_present` — the trigram/grep index, a **code-tier** feature always empty on a text brain —
   so a 28-memory, perfectly-recalling brain reported "absent". **Fix:** report the SEMANTIC index
   (`index_docs`, what `ask` uses): "present (28 of 28 memories indexed…)". Also dropped the "Symbols:
   0" line and the code-tier "next steps" (`overview`/`search`/`sym`/`snapshot`) from brain builds;
   they now suggest `ask`/`remember`/`get`. Same fix in CLI `stats --verbose`.
2. **Tags were WRITE-ONLY.** Every frame stored `tags`, and they drove `delete`/scope, but nothing
   surfaced them — `list_concepts` only walks the `[[wikilink]]` graph. Agents kept inventing synonyms
   (`priority:launch` vs `priority:launch-blocker`, `link:wikilink` vs `link:wikilinks`). **Fix:** added
   `list_tags` (MCP) / `list-tags` (CLI) — aggregate the tag vocabulary with per-tag counts, sorted by
   frequency, optional prefix — matching the global standard (Obsidian's core "Tags view"). Plus a
   `remember` "ALWAYS TAG + call list_tags first to reuse" directive so agents converge instead of
   fragmenting. See [40-build-tier-capability-matrix.md](40-build-tier-capability-matrix.md).
3. **`admin` compliance actions were in the FREE brain tier + `audit` was undocumented.** Two problems,
   one fix. (a) `audit` was implemented but missing from the `admin` schema, so an agent never learned it
   existed (repeat of the #15 discoverability rule). (b) More importantly, the whole **enterprise
   compliance surface** — `legal-hold-add/release`, `retention-sweep`, `audit` (SOX/GDPR/HIPAA machinery)
   — shipped in the FREE personal brain, which is a monetization leak: those belong to the paid tier.
   **Fix (product decision):** added an `enterprise` cargo feature; the four compliance actions are now
   compile-gated to the **`full`/Enterprise bundle only** (CLI: `#[cfg(feature="enterprise")]` on the
   `AdminAction` variants + match arms; MCP: on the handler arms). Basic recovery (`list-tombstones`,
   `restore`, `who-deleted`) stays free in every bundle — a user must always be able to undo a delete.
   On a non-Enterprise build a compliance action returns an honest "needs the Enterprise build" message,
   not a silent unknown. Verified: brain CLI `admin --help` hides them + `admin audit` is unrecognized;
   brain MCP returns the upsell message; basic actions still work; `full` still has all seven.
4. **Tier surface not documented anywhere.** No single doc said what each shipped bundle
   (brain/coding/coding-plus/full) includes vs excludes, so docs kept referencing code-tier commands in
   brain contexts. **Fix:** new [40-build-tier-capability-matrix.md](40-build-tier-capability-matrix.md)
   — the authoritative per-bundle tool/command surface, verified against the `tool_box!` macros and
   `#[cfg(feature)]` gates.
5. **Multi-brain not tier-gated + file-delete safety unstated.** (a) `create` had a one-brain-per-PC
   guard but any user could bypass it with `--force` — multi-brain is meant to be an Enterprise
   capability. **Fix:** gated `--force` (CLI) and the MCP `create` tool behind `enterprise`; the free
   build refuses a second brain with an Enterprise upsell (both surfaces read the same
   `%APPDATA%\said\default` / `~/.config/said/default` anchor). Full build unchanged (multi-brain works).
   (b) Confirmed + documented the file-safety invariant: **no tool ever `fs::remove_file`s a populated
   `.said`** — `delete` only tombstones memories inside the file (recoverable via `admin restore`); the
   sole fs-delete is an empty self-made placeholder guarded by `is_pristine_brain` (zero active frames).
   Removing a real brain is human-only. Reinforced in the `delete` tool description +
   [40-build-tier-capability-matrix.md](40-build-tier-capability-matrix.md) "File-lifecycle safety".
6. **Vague-query tie bleed — added a tag filter to `ask` (additive, no regression).** At scale, many
   memories sharing a `[[wikilink]]` all get the concept-link engine's flat 0.90 reachability score, so
   a *vague* query (no lexical signal) returns results tied at 0.90 with the target mid-pack — reproduced
   live (q2-said-watch at #9; 15 memories carry `link:integrations`). Documented encoder+reachability
   ceiling, NOT a scoring bug (do not lexical-tune — it trades single-hop twins). **Fix (precision lever,
   not encoder tuning):** `ask` now takes an optional tag filter — `tags:[…]` (MCP) / `--tag` (CLI) — that
   narrows recall to memories carrying ALL those tags BEFORE scoring, via a `SaidFile::tag_scope` helper
   feeding the `scope_doc_ids` parameter `ask()` already had. `ask "…watches files…" --tag quarter:Q2` →
   the Q2 facet only, cross-quarter bleed gone. **Regression-proofed:** the param defaults to empty → the
   identical original path, so an unscoped `ask` is byte-for-byte unchanged (verified live: targeted +
   vague queries identical to pre-change; recall-at-volume canary still green ≥0.95; scope/dedup tests
   pass). Also cleaned 66 `fill:live-brain` test fillers from the primary brain (tombstoned, recoverable)
   — proving they were NOT the cause: the vague probe was unchanged after cleanup; the flat-0.90
   concept-link tie was.

**Known state (not a defect — tracked):** these fixes are in source + `target/release`, but the shipped
**v0.11.2 release zips still carry the 2026-07-07 binaries** (no `list_tags`, old `status`). Shipping the
fixes requires a **v0.11.3 CI rebuild** (`build-binaries.yml`) so every bundle + native installer is
rebuilt consistently. Until then, `install.ps1`/`install.sh` download the pre-fix binaries. Do NOT
hand-patch individual release assets across a version — bump the version and rebuild the whole matrix.

## Production surface parity — the standard

To stop the class of defect in #15 from recurring, every shipped `.said` binary must satisfy, per build
variant:

1. **Human-facing `--help` and `--version` on BOTH surfaces.** The CLI's command menu and the MCP
   server's `--help` (tool menu) must both render for a person — an MCP stdio server that only speaks
   JSON-RPC to an agent still needs `--help` so a human can see it.
2. **Tier-accurate descriptions.** A build's command/tool descriptions must describe ONLY what that
   build exposes. The free brain must not mention code/SQL/symbols/modules (paid tiers) in ANY
   surface — not the MCP instructions AND not the individual tool descriptions.
3. **Consistent parameter names across tools.** The same concept uses the same param everywhere
   (`doc_id` for a memory's id in `get`/`delete`/`history`/`checkout`). Rename with a `serde(alias)`
   for back-compat; never ship two names for one thing.
4. **No stub commands.** A command that prints success but does nothing (the old `config`) is a
   correctness bug. Every advertised command must actually do what it says, verified against a real brain.
5. **Sweep-before-ship.** Exercise EVERY CLI command and EVERY MCP tool against a populated brain
   (see the #15 sweep) — not just recall. A command that errors on a valid call, or whose help is
   blank, is a release blocker.
