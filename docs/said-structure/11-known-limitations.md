# Known limitations + proposed enhancements

Every shortfall surfaced during the MVP build, grouped by subsystem. Each item has:

- **Limitation** — what's broken / missing / brittle today
- **Impact** — what it costs a user, and in what scenario
- **Enhancement** — what we would change to fix or upgrade it
- **Tracked in roadmap §** — pointer to the matching checklist entry in [12-roadmap.md](12-roadmap.md)

If you change a limitation here, update the matching roadmap entry in the same commit.

---

## 1. Retrieval

### 1.1 Temporal — PARTIALLY SHIPPED (write-time grounding); relative-word query resolution still open
- **Limitation** — no age-filtered retrieval: `created_at` is stamped once at ingest and is not a query
  filter, so the engine cannot compute "last quarter/year" relative to *today* at query time.
- **SHIPPED (commit `bd3503b`, FIXES-LOG #11)** — WRITE-TIME date grounding (the research-proven Mem0
  Layer-1 approach, verified in the local Mem0 source): when a personal memory is saved, relative phrases
  are resolved to absolute dates IN the stored text ("Last year I…" → "…(around 2025)") deterministically
  (no LLM), so plain semantic recall finds them. `time_compat::ground_relative_dates`; handles last
  year / this year / last quarter / last month with year-boundary wrapping. Result: "last year/month"
  recall moved to @1 (was @3), and absolute queries ("in 2025", "Q2 2026") hit the grounded token. The
  answering LLM does the remaining date-math over the top-K for free (Claude reads them).
- **Still open** — (a) query-side resolution of relative words NOT present in the stored text (a query
  "last year" against a memory that only says "in 2025" relies on the LLM, not the engine); (b) phrases
  beyond year/quarter/month ("N days ago", "last Tuesday"); (c) **FIXES-LOG #12** — MCP grounding is not
  persisted in a large single-session save-per-write batch (block-compaction save path; CLI is immune).
- **Impact** — the common "what did I do last year/quarter" case now works for interactive use; the
  LoCoMo relative-word category and the MCP-batch persistence bug remain.
- **Roadmap §** — [Retrieval / Temporal scoring](12-roadmap.md#retrieval)

### 1.2 NarrativeQA at 0.721 — below the MTEB class average
- **Limitation** — graph fan-out finds the right paragraph in only ~72% of long-form narratives. Multi-hop bridge kicks in, but shallow.
- **Impact** — weakest MTEB number. Blocks any "book QA" or "long report" use case.
- **Enhancement** — deeper multi-hop walk (3-hop instead of 2) + pillar-aware rerank that boosts Episodic frames when the query language is narrative ("what happens after", "who does X meet").
- **Roadmap §** — [Retrieval / Multi-hop depth](12-roadmap.md#retrieval)

### 1.3 Relative cutoff (0.30 × top) is static
- **Limitation** — the `drop-results-below-top×0.30` rule is a constant. Some queries have one clearly-correct doc (cutoff should be 0.50); some have many equally-valid docs (cutoff should be 0.10).
- **Impact** — occasional relevant result pruning on loosely-specified queries.
- **Enhancement** — dynamic cutoff = f(top confidence, candidate distribution variance). If top is 1.0 and rank 2 is 0.9, narrow; if top is 0.5 and rank 2 is 0.49, widen.
- **Roadmap §** — [Retrieval / Dynamic cutoff](12-roadmap.md#retrieval)

### 1.4 BM25 fusion is route-gated (on for ask, off for search)
- **Limitation** — `search` skips BM25 deliberately to preserve the "pure semantic" contract. But some `search` callers would benefit from fusion.
- **Impact** — inconsistent behaviour between `ask` and `search`; requires users to know which one to use.
- **Enhancement** — optional `--fuse` flag on `search`, off by default. Keep the contract, let power users opt in.
- **Roadmap §** — [CLI / Search uniformity](12-roadmap.md#cli--mcp)

### 1.5 No tri-gram for Unicode beyond ASCII
- **Limitation** — trigram index tokenizes on 7-bit ASCII. CJK / Arabic / Hebrew corpora get no trigram hits; they fall back to SCA-only.
- **Impact** — slower + less precise retrieval for non-Latin-script brains.
- **Enhancement** — UTF-8-aware trigram with Unicode normalization (NFKC) before windowing.
- **Roadmap §** — [Retrieval / Unicode trigrams](12-roadmap.md#retrieval)

### 1.6 Large-repo INGEST: RESOLVED — 37 k Wonga peaks 486 MB (under the 580 MB target), ingests in ~20 s, recalls correctly

- **ROOT CAUSE FOUND + FIXED (2026-07-02): CSV data dumps. Not SQL, not the word index, not the allocator.**
  A **per-directory peak test** (ingest each Wonga subdir alone, measure peak) isolated it in one shot:
  `Wonga Compressed Project for Modernization` peaked **750 MB ALONE**, while every code/SQL-only dir —
  including the heavy-SQL `AB` (2,969 `.sql` files) — peaked **≤202 MB**. So SQL was never the problem
  (the owner said this from the start). That one dir carries **1,137 CSV transaction-dumps each UNDER the
  5 MB per-file cap** (85 MB total) that char-chunked into **~346 k passages** — the spike. A per-file size
  cap cannot catch a *swarm* of small-ish data dumps; the fix is excluding the **type**. `.csv` removed from
  `PLAIN_TEXT_EXTENSIONS` (opt back in with `SAID_INGEST_CSV=1`, still size-capped). Commit `eef73c0`.
- **MEASURED result:** culprit dir **750 → 223 MB**; **full 37 k Wonga 920 → 486 MB** (under the 580 MB
  constant-memory target), ingest **~3 min → ~20 s** (9 s read + 11 s compact), clean 44 MB brain. Recall
  unaffected — cold `ask "amortization schedule calculation"` still returns `AmortizationSchedule::Build`
  at score 1.00; SQL tables still recalled. This is the "runs within phone memory" contract met.
- **The measurement-method lesson (why this took so long).** Earlier runs reported a "~1.4 GB
  budget-invariant floor." That number was **inflated by a stray `said` process** (a leftover MCP/prior run
  holding ~528 MB) that the external `Get-Process said | Measure-Object -Sum` poller summed into every
  reading. An **in-process** probe (`K32GetProcessMemoryInfo`, reads *this* PID) gave the true single-process
  peak (~920 MB pre-fix) and the flat tail; the per-directory A/B then pinned the cause to one dir. Chasing
  the phantom sent several fixes down the wrong path (word-index SPIMI spill — only ~136 MB; rayon
  thread-count — 871 vs 920; passage-budget batching — *worse* at 954; mimalloc — worse). **Lesson: measure
  the exact PID from inside the process, and isolate by input (per-dir) before touching code.**
- One real transient fix did land on the way (kept): the encode window stored every passage embedding
  (`DocEnc.passages: Vec<Vec<f32>>`) → replaced with an incremental `passage_sum` fold (bit-identical),
  which cut the 13 k-SQL encode transient 786 → 233 MB (commit 812e178).

<details><summary>Earlier (superseded) investigation notes</summary>

- **Superseded 2026-07-01 note — the "~1.4 GB budget-invariant floor" was the stray-process artifact above.**
- **Owner decision (2026-07-01): SHIP the working fix.** The crash-fix + correct recall is the real value;
  driving the ~1.4 GB floor to ~500 MB requires restructuring `init` so frames-pending + mmap + index do
  not all coexist (compact per-batch during read, drop the pending buffer, derive WIDX from spilled
  segments — true single-pass SPIMI). That is real surgery with careful bit-identity verification, deferred.
  Until then: a 37 k-doc monorepo ingests crash-free + recalls at ~1.4 GB, needs a box with >2 GB free.

<details><summary>Earlier investigation notes (kept for history — the CSV/O(N²)/WIDX work still stands)</summary>

- **STATUS (measured, honest, 2026-07-01).** The REAL headline cause was **CSV DATA DUMPS**, not the word
  index. Wonga's actual code is only **82 MB** (.cs + .sql, 11,843 files); it also carried **3.8 GB of
  `african_bank_data/source/*.csv`** (an 831 MB transaction export + 63 more >5 MB) that were ingested
  because `.csv` was in `PLAIN_TEXT_EXTENSIONS` → char-chunked into a multi-GB passage explosion = the
  3.3 GB OOM. **Fix:** `should_enroll()` caps NON-code text/data files at `SAID_TEXT_MAX_BYTES` (default
  5 MB); code/SQL uncapped (both init filter sites). **Result:** Phase-1 read **200–370 s → 14.4 s**
  (~20×), Phase-2 encode **100.6 s, no crash**, files 15,249 → 15,186 (63 dumps excluded).
- **Measured memory breakdown @ 37,112 docs** (CSV excluded, skip-resident on), via `SAID_MEM_REPORT`:
  lexical word index = **459 MB total** (`vocabulary_fast=400 MB` dominant, `word_inverted_fast=0 MB`
  [skip-resident works], `doc_word_sets=20 MB`, `doc_word_tf=38 MB`) — **UNDER the 580 MB ceiling**; frame
  store `pending=285 MB`. So the word index AND the frame buffer are both fine. (An earlier note here
  guessed the per-doc structures were 800/900 MB — WRONG, they are 20/38 MB; corrected by the mem-report.)
- **PHASE-3 HANGS FIXED — full-defaults Wonga now ingests END-TO-END (milestone).** Two O(N²) passes hung
  Phase 3 on 37k frames; both fixed with record-linkage BLOCKING (Ravikumar VLDB'03 / Papadakis 2013),
  deterministic + bit-identical:
  - **OKF** (`build_concept_links`, said_file.rs) — the title-mention step was O(frames × titles) = 1.37 B
    pairs + a body decompress per frame. Now a token index + O(1) set-membership (a title is a body word-
    token). Same 6,274 edges. (36c7b84)
  - **Harvest** (`harvest_scan`, harvest.rs) — skeleton clustering was all-pairs Jaccard = ~105 M comps on
    ~14.5 k functions. Now blocked by shared call-token (Jaccard ≥ 0.70 ⇒ must share a call). Same
    clusters. (b509fd1)
  - **Proof:** full-defaults run (OKF on + harvest on + auto-spill) COMPLETED — `[okf] 6274 edges` +
    `[harvest] 771 blueprints` + `Compacted + saved`, exit 0, a **82.3 MB brain, 37,790 memories, 14,560
    symbols**. Recall verified (`amortization schedule` → the real `AmortizationSchedule.cs` constructor,
    score 1.00). First true full-config 37 k-frame Wonga brain.
- **Spill budget now PER-SYSTEM** (4e9b1f1): `clamp(available_RAM × 12%, 16 MB, 512 MB)` via `sysinfo`
  (Elasticsearch/Lucene/DuckDB precedent). 580 MB is the low-RAM-device guarantee (auto-spill there);
  capable machines stay at the ceiling and don't force-spill (spill costs ~14× read time — Spark/PostgreSQL
  "budget high, avoid spilling").
- **REMAINING for 580 MB on low-RAM devices:** peak on a high-RAM machine is still ~2 GB, from the
  **compact transient** (`frames.rs::compact_block_dict`): `raw_frames` holds all frames' decompressed
  bytes at once + a second full `flat` copy for zstd dictionary training + all compressed blocks collected
  before merge. NOT the word index (459 MB) or frames (285 MB). Fix: window the block compression + drop
  the redundant `flat` full-copy. (Recorded in memory `large-repo-ingest-too-slow`.) The sniper-shot A/B
  that isolated this: `SAID_OKF_LINKS=0` + `SAID_INIT_HARVEST=0` — if peak drops to ~500 MB the culprit is
  OKF/harvest (make it streaming); if still ~2 GB it is `compact_block_dict` (bound the repack).
- **What SHIPPED + WORKS (real value):** the disk-backed WIDX word-index section
  (`crates/sca-core/src/word_index.rs`) — serialize (varint-delta postings) + `WidxReader` in-place mmap
  decode + save/open persistence + WIDX-aware query sites (`docs_for_wid`) + skip building the resident
  inverted map during init. All **bit-identical recall** (tests `test_widx_from_core`,
  `skip_resident_wordidx_recall_matches_normal`; lib 98/0; binary regression 15/15). A re-opened brain
  reads postings from mmap and no longer rebuilds the 2.3 GB word index at query time. The crash that made
  Wonga totally un-ingestable is gone.
- **What's LEFT for 580 MB:** apply the SAME SPIMI/mmap pattern to the per-doc structures
  (`doc_word_sets_fast`, `doc_word_tf_fast` → spill to disk during the merge, derive WIDX from the spilled
  segments) and stream the Phase-1 frame buffer harder. Same technique, more surface. Until then, a 37k-doc
  monorepo ingests (crash-free) but at ~3 GB, not 580 MB.
- **Not duplicated by LAM (checked).** `SAID-ECHO/LAM/LAM`'s word index (`rust_candle/src/crystalline.rs`)
  is the SAME in-RAM `HashMap` inverted index with the SAME OOM and NO persistence; LAM's
  `MMAP_IMPLEMENTATION_*.md` are unbuilt PROPOSALS for dense embeddings, not the sparse word index. So the
  new WIDX section (`crates/sca-core/src/word_index.rs`) is the first real disk-backed inverted-index IO —
  legitimate, not a reinvention. **Read side DONE** (WIDX serialize + `WidxReader` in-place mmap decode +
  save/open persistence + a re-opened brain skips the rebuild). **Remaining:** the first-time `init` still
  builds the index in RAM inside `add_docs_quantized` (BEFORE save), so it needs a SEGMENTED build that
  spills postings to disk in bounded chunks — true single-pass SPIMI — to hold 580 MB on the FIRST build.
- **Memory progress so far (the transient half).** The encode + word-prep phases previously each did one
  `par_iter().collect()` over the WHOLE corpus. Two fixes landed:
  - **Build-artifact skip** — `is_junk_dir` now skips .NET/SQL build dirs (`bin`, `obj`, `Debug`,
    `Release`, `packages`, …). A .NET repo with no root `.gitignore` was pulling in `obj/Debug/*.generated.sql`
    (regenerated proc dumps) — a passage explosion. (Helps, but is not the main lever: a real bank keeps
    ~7.4k legitimate `.sql` files; the corpus is genuinely large.)
  - **Streaming `index_batch`** — the encode phase now processes docs in **bounded windows** sized from
    `SAID_INDEX_BUDGET` bytes (**default 580 MB** — the owner's constant-memory contract). Each window is
    encoded in parallel, folded serially in doc order (bit-identical corpus mean), written to the mmap
    scratch, then dropped. Peak heap for the phase = one window, not the corpus. Proven result-invariant:
    `test_index_budget_streaming::windowing_is_result_invariant` (1-byte budget == 4 GB budget recall).
- **Speed: STILL OPEN.** Phase-1 (read → tree-sitter chunk → encode) is still slow at scale (~250 s just to
  read+chunk 16k files; full build tens of minutes). This is the remaining blocker for onboarding a real
  enterprise codebase fast.
- **Impact** — RAM is now safe (a 13k-file repo stays within the 580 MB ceiling, no crash). Onboarding
  SPEED is the open product issue. RECALL is fast (~100 ms warm, doc 35).
- **Enhancement (speed — still REQUIRED)** —
  1. **Parallelize Phase-1 read+chunk** (the encode is already parallel + now windowed; the per-file
     read+tree-sitter loop is still serial — fan it out with a bounded channel into the streaming writer).
  2. **Incremental + resumable ingest** — checkpoint so a 13k-file init can resume; re-init only touches
     changed files (BLAKE3 dedup exists — make it the end-to-end fast path).
  3. **Serialize the word index** (open INIT-ROUTE-TRACE item) so it is never rebuilt.
  4. **Research the SOTA**: zoekt / Tantivy / ripgrep+tree-sitter / mem0/Zep bulk ingest — batching,
     mmap-direct chunking, SIMD batch encode, sharded indices. Measure files/sec + frames/sec + peak RAM as
     a first-class benchmark.
- **Target** — a 13k-file repo should init in **single-digit minutes within the 580 MB ceiling**, and
  re-init in seconds. Memory target met; speed target pending.
- **Roadmap §** — [Retrieval / fast large-repo ingest](12-roadmap.md#retrieval) — **HIGH PRIORITY.**

</details>

## 2. Pillars + memory model

### 2.1 `Factual → Semantic` collapse in legacy ingestion paths
- **Limitation** — frames written via `put_with` (the pre-pillar API, still used by `document_ingest`, `code_search`, `whisper_ingest`) collapse to Semantic even when they should be Procedural/Code/External. Shipped mitigation: `remember_with_pillar` explicitly calls `set_pillar` after `put_with`, but direct callers bypass it.
- **Impact** — per-pillar retrieval scope can miss frames that logically belong to Procedural/Code but are tagged Semantic.
- **Enhancement** — sweep the three remaining call sites to go through `put_with_pillar`. Low-risk, tracked.
- **Roadmap §** — [Pillars / legacy call-site sweep](12-roadmap.md#pillars)

### 2.2 No Episodic-timeline pillar distillation
- **Limitation** — Episodic frames pile up without any summary anchor. After 10k chat turns, the cheapest retrieval (for "what did we discuss yesterday") is still to scan all Episodic frames.
- **Impact** — LoCoMo temporal bottleneck, real-world latency ceiling.
- **Enhancement** — daily/weekly rollup — a Semantic frame that summarizes the last N Episodic frames (built offline, BYO-LLM). The brain stores the rollup as a content-addressable frame; the original Episodic frames stay as backup.
- **Roadmap §** — [Pillars / timeline distillation](12-roadmap.md#pillars)

### 2.3 Pillar is not enforced on ingest paths
- **Limitation** — `said ingest file.pdf` picks External pillar by default. `said add "note"` defaults to Semantic. Users can't override at the CLI.
- **Impact** — awkward for batch workflows.
- **Enhancement** — add `--pillar` flag to `add / ingest / init`. Trivial change.
- **Roadmap §** — [CLI / pillar flag](12-roadmap.md#cli--mcp)

## 3. Dream + brain state

### 3.1 Dream v1 content concatenation was pulled
- **Limitation** — v1 attempted to distill Episodic clusters by concatenating content. Hurt LoCoMo R@10 by 2pt. v2 only moves S_slow + recall_weight; leaves content alone.
- **Impact** — we don't yet get the "memory consolidation" benefit; only the fingerprint-drift benefit.
- **Enhancement** — v3 should distill content too, but via an **external** LLM call the caller makes, writing the distilled result as a new Semantic frame with `source_frames:` tags pointing to the original Episodics. The `.said` binary stays LLM-free.
- **Roadmap §** — [Dream / v3 distillation](12-roadmap.md#dream)

### 3.2 Dream threshold is only time-bucket aware
- **Limitation** — `dynamic_dream_threshold(N)` scales with corpus size linearly. Doesn't account for *topical* density (a burst of 50 related queries should trigger faster).
- **Impact** — slight lag before dream picks up a new topic.
- **Enhancement** — track rolling topic-variance; trigger when variance drops below a threshold regardless of count.
- **Roadmap §** — [Dream / adaptive threshold](12-roadmap.md#dream)

## 4. Surprise detector

### 4.1 Heuristic only — no ML model
- **Limitation** — surprise uses `(similarity, token_overlap)` thresholds. Works well on dominance-based similarity, but misses nuanced contradictions (paraphrase with flipped polarity).
- **Impact** — false-negatives on subtle reconsolidation events.
- **Enhancement** — optional ML scorer trained on SNLI-style entailment pairs, behind a feature flag. Keep the heuristic as fallback.
- **Roadmap §** — [Surprise / ML scorer](12-roadmap.md#surprise)

## 5. File format + storage

### 5.1 Save rewrites the entire file
- **Limitation** — `SaidFile::save()` writes the full brain every time, even for single-frame changes. OK at 10 MB; painful at 1 GB.
- **Impact** — perceptible latency on large brains; wears SSDs harder than needed.
- **Enhancement** — append-only incremental write path: keep existing BLKT/SYMS/TRGM sections, append a delta section, rewrite FTOC only. Compact periodically merges deltas.
- **Roadmap §** — [Storage / incremental save](12-roadmap.md#storage)

### 5.2 BRAN bloat fix is in place but legacy files may still be oversized
- **Limitation** — the BRAN deserialize bug (read past its section) was fixed (memory: `project_bran_bloat_fix.md`). But brains saved by pre-fix builds are still 2× bloated.
- **Impact** — disk space only; no behavioural bug.
- **Enhancement** — add `said admin compact --rebuild-bran` that re-serializes BRAN from the in-memory state.
- **Roadmap §** — [Storage / BRAN re-pack](12-roadmap.md#storage)

### 5.3 Windows file-lock fragility during rename-on-save
- **Limitation** — mmap handles on Windows sometimes survive the atomic rename, leaving `.said` files with stale blocks. Mitigated by careful mmap drop; still fragile.
- **Impact** — rare but when it happens, requires `said clean` or a restart.
- **Enhancement** — switch to `std::fs::rename` with retry loop + explicit `drop(mmap)` before rename. Already partly in; make it universal.
- **Roadmap §** — [Storage / Windows rename robustness](12-roadmap.md#storage)

### 5.4 No cross-platform path normalization on ingest
- **Limitation** — Windows paths stored as `C:\users\…`; Unix paths as `/home/…`. Cross-syncing a brain between OSes breaks `source:` tag resolution.
- **Impact** — Dropbox / team-share cross-OS workflows.
- **Enhancement** — normalize to forward-slash + `~/` or project-relative on ingest; resolve at query time.
- **Roadmap §** — [Storage / portable paths](12-roadmap.md#storage)

## 6. Ingestion plugins

### 6.1 Scanned PDF OCR requires `pdfium` at runtime
- **Limitation** — `ocr` feature needs `pdfium.dll` / `libpdfium.so` reachable. Without it, scanned PDFs skip silently.
- **Impact** — surprise failure for users who compiled with `ocr` but never installed pdfium.
- **Enhancement** — bundle a fallback rasterizer (slower, no external dep) that kicks in when pdfium isn't found.
- **Roadmap §** — [Ingestion / pdfium fallback](12-roadmap.md#ingestion)

### 6.2 Whisper on macOS / Linux is CPU-only
- **Limitation** — `directml` feature only works on Windows. On Mac/Linux, whisper runs on CPU.
- **Impact** — 10-50× slower transcription on non-Windows.
- **Enhancement** — wire Metal (macOS) and CUDA/ROCm (Linux) backends via sherpa-rs feature flags.
- **Roadmap §** — [Ingestion / whisper GPU cross-platform](12-roadmap.md#ingestion)

### 6.3 Code plugin: no Kotlin, Swift, Scala, Ruby, PHP
- **Limitation** — tree-sitter grammars cover 7 languages today.
- **Impact** — can't cleanly ingest JVM non-Java, mobile non-TS, Rails, WordPress.
- **Enhancement** — add grammars. Each is ~15 lines in Cargo.toml + a dispatch case in `code_chunk`.
- **Roadmap §** — [Ingestion / more languages](12-roadmap.md#ingestion)

### 6.4 Init is recursive but gitignore-aware; ingest (MCP) is shallow
- **Limitation** — CLI `init` walks with gitignore; MCP `ingest` on a folder walks only one level deep. Inconsistent.
- **Impact** — agents invoking MCP to ingest nested trees get partial coverage.
- **Enhancement** — unify behind a shared walker; keep gitignore behaviour by default.
- **Roadmap §** — [Ingestion / walker uniformity](12-roadmap.md#ingestion)

## 7. CLI / MCP

### 7.1 `said watch` deferred
- **Limitation** — Row 29. The "watch a folder, re-ingest on change" command is specced but not shipped.
- **Impact** — users manually run `said ingest path` after edits.
- **Enhancement** — `notify`-based watcher that debounces + calls the same ingest codepath.
- **Roadmap §** — [CLI / said watch](12-roadmap.md#cli--mcp)

### 7.2 No `said diff <ver1> <ver2>`
- **Limitation** — `history` lists versions but doesn't show content diffs.
- **Impact** — debugging "what changed between v3 and v4" is manual.
- **Enhancement** — `said diff doc_X v3 v4` → unified diff on content.
- **Roadmap §** — [CLI / diff](12-roadmap.md#cli--mcp)

### 7.3 MCP can't stream
- **Limitation** — MCP returns all `ask` results in one JSON-RPC response. Large result sets pause the client.
- **Impact** — UI feel on 100+ result queries.
- **Enhancement** — MCP streaming tool variant — emit notifications as results arrive. Requires protocol upgrade; deferred.
- **Roadmap §** — [MCP / streaming](12-roadmap.md#cli--mcp)

### 7.4 Admin UI not shipped (Row 43)
- **Limitation** — admin surface is CLI-only. No web UI for legal hold management / audit viewing.
- **Impact** — enterprise adopters need a UI for compliance auditors.
- **Enhancement** — small local web app (React + Rust axum backend) mapping 1:1 to `said admin`. Can start as an MCP tool called from Claude Code.
- **Roadmap §** — [Admin / UI](12-roadmap.md#admin--audit)

## 8. Module workflow (snapshot + sandbox)

### 8.1 Snapshot doesn't auto-refresh on parent changes
- **Limitation** — lens `frame_ids` are snapshot at creation time. New parent frames tagged `module:card` don't appear until re-snapshot.
- **Impact** — stale views. Users forget to re-run.
- **Enhancement** — `said snapshot card --live` mode: store a query in the lens instead of a frozen frame_id set; resolve at every query. Plus a `said snapshot --refresh` shortcut for the frozen case.
- **Roadmap §** — [Snapshot / live lens](12-roadmap.md#module-workflow)

### 8.2 Hub threshold hard-coded at ≥ 5 FKs
- **Limitation** — the "is this a hub table" rule is a magic number.
- **Impact** — some brains want 3, some want 10.
- **Enhancement** — `said snapshot card --hub-threshold 3`.
- **Roadmap §** — [Snapshot / config](12-roadmap.md#module-workflow)

### 8.3 Sandbox is SQL Server-only
- **Limitation** — the compose + schema-rendering templates are SQL Server 2022 specific.
- **Impact** — Postgres / MySQL / MongoDB monoliths can't use sandbox.
- **Enhancement** — template dispatch on brain metadata tag `sql_dialect:<postgres|mysql|mssql>`. One template file per dialect.
- **Roadmap §** — [Sandbox / dialects](12-roadmap.md#module-workflow)

### 8.4 Sandbox dependency sort is best-effort
- **Limitation** — function-ref grep is whole-word + comment/string stripping. Schema-qualified identifiers and dynamic SQL can evade it.
- **Impact** — rare mis-ordering; surfaces as schema-load errors the run-counter reports.
- **Enhancement** — proper SQL parser (sqlparser-rs) for the dep graph. Heavier dep, more accurate.
- **Roadmap §** — [Sandbox / accurate dep-sort](12-roadmap.md#module-workflow)

### 8.5 No cross-OS path portability in sandbox output
- **Limitation** — `docker-compose.yml` uses relative paths but `run.sh` uses `/bin/bash` — doesn't run on Windows without WSL / Git Bash.
- **Impact** — Windows-native users need WSL for bring-up.
- **Enhancement** — also emit `run.ps1` and `run.cmd`.
- **Roadmap §** — [Sandbox / multi-shell](12-roadmap.md#module-workflow)

## 9. Enterprise / compliance

### 9.1 Legal hold is single-tag
- **Limitation** — `legal_hold:CASE-A` is one tag; stacking multiple cases on one frame just adds more tags. Works but scales awkwardly at 100+ cases.
- **Impact** — enterprise use with many concurrent matters.
- **Enhancement** — dedicated `legal_holds` section (sorted list) alongside `tags`. Indexable separately.
- **Roadmap §** — [Audit / legal hold index](12-roadmap.md#admin--audit)

### 9.2 AppGrant registry is in-process only
- **Limitation** — AppGrant entries (which apps can write to which pillars) live in the brain's own AUDT section. Fine for single-brain use; not portable across a fleet.
- **Impact** — fleet deployments need to sync grants manually.
- **Enhancement** — export/import grant bundles; optional central registry file.
- **Roadmap §** — [Audit / grant fleet sync](12-roadmap.md#admin--audit)

### 9.3 Audit chain is per-brain; no cross-brain attestation
- **Limitation** — BLAKE3 chain protects against in-file tampering. Doesn't protect against wholesale file swap.
- **Impact** — strong adversary can replace the whole `.said`.
- **Enhancement** — sign the head hash with an external key (OS keychain, HSM, or remote signing service) on every save. Verify on open.
- **Roadmap §** — [Audit / external attestation](12-roadmap.md#admin--audit)

## 10. Migration + interop

### 10.1 Migration adapters cover mem0 + memvid only
- **Limitation** — Row 48 shipped adapters for mem0 (JSONL + category→pillar map) and memvid. Zep, LangMem, Letta are specced but not shipped.
- **Impact** — users on those platforms have to hand-export.
- **Enhancement** — implement the remaining adapters; the trait is stable.
- **Roadmap §** — [Migration / more adapters](12-roadmap.md#migration)

### 10.2 No round-trip export to mem0 / memvid
- **Limitation** — adapters import only. If a user wants to move off `.said`, they have to write their own exporter.
- **Impact** — lock-in perception, even though the format is open.
- **Enhancement** — `said export --format mem0 out.jsonl`.
- **Roadmap §** — [Migration / export](12-roadmap.md#migration)

## 11. Plugin ecosystem (Row 50)

### 11.1 `SaidPlugin` trait shipped; no discovery/marketplace
- **Limitation** — the trait exists. But there's no `~/.said/plugins/` discovery path, no signing, no manifest repo.
- **Impact** — plugin authors have to fork + rebuild.
- **Enhancement** — directory-based discovery with a signed manifest; small registry file at `plugins.said-lam.dev`.
- **Roadmap §** — [Plugins / discovery](12-roadmap.md#plugins)

### 11.2 Enterprise-mode refusal is coarse
- **Limitation** — `PluginRegistry` refuses any plugin in Enterprise mode unless explicitly whitelisted. All-or-nothing.
- **Impact** — enterprise users who trust a specific vendor can't opt in per-plugin without changing code.
- **Enhancement** — per-plugin allow-list with capability scopes (read-only / pillar-limited / full).
- **Roadmap §** — [Plugins / enterprise scopes](12-roadmap.md#plugins)

## 12. Documentation

### 12.1 Rows 1-28 are thematically folded, not individually paged
- **Limitation** — rows 30-50 each have a dedicated `05-features/row-XX-*.md`. Rows 1-28 are rolled into the themed folders ([10-benchmarks](10-benchmarks/), [06-ingestion-plugins](06-ingestion-plugins/), etc.).
- **Impact** — links from git commits referencing "row 17" don't have a single destination.
- **Enhancement** — add `05-features/themes/row-17.md` stubs that redirect to the themed folder. Low priority.
- **Roadmap §** — [Docs / row stubs](12-roadmap.md#docs)

### 12.2 No architecture diagram yet
- **Limitation** — the relationship between pillars / fingerprints / trigram / SCA / brain state is explained in prose only.
- **Impact** — harder for new contributors to orient.
- **Enhancement** — one Mermaid diagram in [01-overview](01-overview/) showing the ingest → index → query flow.
- **Roadmap §** — [Docs / architecture diagram](12-roadmap.md#docs)

---

## 13. said-forge (MVP 2026-04-23)

### 13.1 `forge_run` over MCP is deferred
- **Limitation** — the MCP `forge_run` tool does not actually run the batch. `self.brain: Arc<Mutex<SaidFile>>` uses `std::sync::Mutex`, whose guard is not `Send` across `.await`. `run_one()` awaits the LLM inside the generation loop.
- **Impact** — AI editors that want to trigger generation from MCP must call the CLI instead (`said forge run --all --yes`). The tool returns a CLI-command hint when invoked.
- **Enhancement** — switch `self.brain` to `tokio::sync::Mutex` or an actor, then port the runner loop natively. Architectural change beyond the forge MVP scope.
- **Roadmap §** — [Forge / MCP run](12-roadmap.md#forge)

### 13.2 Projection folder overwrites engineer edits
- **Limitation** — re-running `said forge run` for a story atomically rewrites `.forge/<slug>/story.md` / `plan.md` / `tasks.md` / `brain.md`. Hand-edits to those files are lost without warning.
- **Impact** — engineers who want to customize the projection must commit to external git before re-running, or use `said forge reset` + edit the brain's `forge:spec:*` frame directly (which the lineage preserves).
- **Enhancement** — three-way merge (previous-generation / current-edit / new-generation) with conflict markers. Milestone C (sandbox/runtime) explores a worktree-per-story model that avoids the overwrite problem entirely.
- **Roadmap §** — [Forge / edit preservation](12-roadmap.md#forge)

### 13.3 `confirm:true` gate is convention, not enforced
- **Limitation** — the MCP write tools (`forge_load`, `forge_reset`) refuse calls without `confirm:true`. The skill file instructs the AI to ask the user before setting it — but a hostile or non-compliant client can set `confirm:true` directly.
- **Impact** — not a security boundary. Destructive actions require cooperation from the AI, not hard authorization.
- **Enhancement** — a separate pre-flight confirm-token tool the AI must echo back. Adds latency; not in MVP.
- **Roadmap §** — [Forge / hard confirm gate](12-roadmap.md#forge)

### 13.4 `forge_get` 25k-token cap truncates bluntly
- **Limitation** — when the bundled markdown exceeds 25 000 tokens (≈100 000 chars), each section is proportionally truncated at a UTF-8 char boundary with a `[TRUNCATED — call get <frame-id>]` marker. The AI must drill down via individual `get <frame-id>` calls for full body.
- **Impact** — edge case. Only triggers for stories with unusually long grounding or spec bodies.
- **Enhancement** — structured response that includes the truncation metadata separately so clients can render a "fetch full" button. Not blocking MVP.
- **Roadmap §** — none (accept)

### 13.5 Single active directive per brain
- **Limitation** — loading a new directive via `said forge load` does not remove the previous directive's frames — they stay as history (`said history forge:directive:*`). The CLI's `latest_directive_hash` picks whichever was committed most recently. A single brain can't simultaneously hold multiple active directives.
- **Impact** — multi-project brains must use separate `.said` files, one per directive.
- **Enhancement** — explicit directive selection flag on `forge run` / `forge list`. Not in MVP.
- **Roadmap §** — [Forge / multi-directive](12-roadmap.md#forge)

### 13.6 Anthropic prompt-cache TTL is 5 minutes
- **Limitation** — the prompt-cache discount (90% on cache-read tokens) only applies within Anthropic's 5-minute TTL. Long batches where cold stories take >5 min to reach will re-pay the full prelude cost.
- **Impact** — cost-efficient only when stories are generated in ≥50-story contiguous groups.
- **Enhancement** — chunk the batch into groups of 50 and insert explicit cache-warmup at the start. Opportunistic; v2.
- **Roadmap §** — none (accept)

### 13.7 Server-side MCP tag filtering not implemented
- **Limitation** — all 6 forge tools carry `[forge]` in their description string, but there's no server-side `--mcp-tags +forge` / `-forge` flag to filter the tool listing.
- **Impact** — AI editors see all 31 tools regardless of need. Client-side filtering by description prefix works.
- **Enhancement** — wire a `--mcp-tags` flag through `said-mcp` that filters at `tools/list` time.
- **Roadmap §** — [Forge / mcp tag filter](12-roadmap.md#forge)

---

## 14. Product surface & discoverability

### 14.1 The agent is the UI — autonomous brain use depends on the host agent
- **Limitation** — there's no standalone GUI or slash-command surface for a normal user; the brain is reached **through an AI agent** (Claude/Cursor/Copilot via MCP). The brain-MCP constitution instructs the agent to use the brain autonomously — "**ALWAYS call `ask` first**" for user-context questions, and act as a note-taker (`remember` on decisions/facts) — but whether that actually happens depends on the **host agent obeying its instructions**. A less-compliant agent may answer from its own training instead of consulting the brain, or forget to save.
- **Impact** — the brain's value (recall + auto-memory) is only realized when the agent leans on it. Users can't discover `admin`, deep `ask`, or `list_tags` on their own; they rely on the agent to invoke them. This was learned the hard way in testing — the mechanism (the nudge) exists, but adoption is agent-dependent.
- **Why it's here, not fixed** — the mechanism is already built (constitution nudges + hook injection, doc [22-memory-injection-nudge-pattern.md](22-memory-injection-nudge-pattern.md)); the residual gap is a *product-surface* decision (a standalone UI / slash-command layer), which is a larger scope than a code fix. Recorded as an explicit decision to keep the "agent is the UI" model for now.
- **Mitigation today** — the constitution is as strong a nudge as prompt-level steering allows; users who want reliable autonomous use should prefer a capable agent and can always invoke tools explicitly ("use my brain — remember X", "check my brain for Y").
- **Roadmap §** — a standalone discovery surface (UI / slash commands) is not yet scheduled; see [12-roadmap.md](12-roadmap.md).

### 14.2 Status "dream cycles" — RESOLVED
- Fixed: `status`/`stats` now render a plain-English "Learning:" line ("learned from N searches, reorganized itself M times to surface answers faster") instead of the engineer-facing "Dream cycles: N". Raw counters remain under `stats --verbose`. (Kept here only as a pointer; the entry is resolved — remove on next cleanup.)

---

## Maintenance rule

When any item above is fixed:

1. Delete the entry from this file.
2. Tick the matching checkbox in [12-roadmap.md](12-roadmap.md).
3. If the fix changed public behaviour, add a note to the relevant page (e.g. fix for 1.1 updates [10-benchmarks/locomo.md](10-benchmarks/locomo.md)).

Every limitation is a promise to do something — or an explicit decision not to. No silent carry-overs.
