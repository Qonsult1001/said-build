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
