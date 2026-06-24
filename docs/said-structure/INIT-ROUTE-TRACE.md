# `said init` — actual code route traced end-to-end (OOM investigation)

Traced against the documented contract in [07-cli-reference/init.md](07-cli-reference/init.md).
Purpose: understand the EXACT path init follows, where every data structure is built, and
why a full ~33K-frame ingest OOMs. Written before changing anything.

## The route (cmd_init, crates/said-cli/src/main.rs)

```
cmd_init(dir)
│
├─ canonicalize dir, resolve .said path                         [doc step 1-2]
├─ load_gitignore(dir)                                          [doc step 3]
├─ walk_dir_gitignore(dir, ...) → files: Vec<PathBuf>          [doc step 3]
│     skips: VCS dirs, hidden dirs, is_backup_dir, is_junk_dir, .gitignore matches
│     ⚠ is_junk_dir currently over-broad: excludes "out","bin","obj","packages",
│       "build","dist" — these can hold REAL user content (PROVEN: _deploy/out/ SQL
│       wrongly dropped → 277 memories became 2). Doc only sanctions
│       "node_modules, target, .venv, build artifacts".
│
├─ build indexed_hashes: HashSet<blake3:hex>  ONCE              [O(1) dedup, 847c77e]
│
├─ PHASE 1: for each file (SERIAL):                             [doc step 5]
│     read bytes → blake3 → dedup-skip (O(1) set) → decode_text
│     if code ext: ast_chunk(content, ext) → per chunk:
│         brain.remember_as(doc_id, chunk.content, title)   ← FRAME stored
│         brain.add_tag(... source:, ingest:code, blake3:, call:<callee> edges ...)
│         brain.record_symbol(...)                           ← SYMS index
│     else: brain.remember_as(rel_path, whole content)
│
├─ PHASE 1b: deletion sweep (tombstone frames whose file vanished)
│
├─ PHASE 2: brain.build_index_with_progress(progress)          [doc step 6]
│     │  (said_file.rs build_index_with_progress)
│     ├─ collect doc_ids + doc_texts (reads ALL frame texts into RAM)
│     ├─ INCREMENTAL vs FULL decision (prior==0 on fresh init → FULL):
│     │     engine.clear(); engine.index_batch_with_progress(doc_ids, doc_texts)
│     │       (engine.rs index_batch_with_progress, line 279)
│     │       ├─ A. build doc_freq + all_doc_words (tokenize every doc)
│     │       ├─ B. per-doc STREAM: chunk_text(512/256) → encode_batch → doc-mean
│     │       │      → write to DocMeanScratch (mmap temp, RAM-flat)  ✅ bounded
│     │       ├─ C/D. corpus mean + std
│     │       ├─ E. gammas; F. load IDF
│     │       ├─ G. read all doc-means from mmap → all_embs Vec (n_docs×64×4 ≈ 8MB) ✅
│     │       └─ H. core.add_docs_quantized(ids, all_embs, counts, gammas, ALL_DOC_WORDS)
│     │             (crystalline.rs ~2742)
│     │             └─ ⛔ WORD-LEVEL INDEXING LOOP (~2770): for every doc, for every word:
│     │                  doc_word_sets_fast / doc_word_tf_fast / doc_texts_fast (Vec per doc)
│     │                  word_inverted_fast (word→docset) / phonetic_index_fast / vocabulary_fast
│     │                  → ~2.3 GB at 33K code docs (measured 70KB/doc via SAID_MEM_REPORT)
│     │                  → THE OOM: single alloc ~3.3GB fails at ~62% (one map reallocating)
│     └─ also stream_index(... 100_000) builds the lean u32 ART inverted_index
│
├─ PHASE 3: brain.compact() ; brain.save()                     [doc step 7-8]
│     ⚠ the _fast word structures are NOT serialized — built here, written to disk
│       WITHOUT them, process exits → the 2.3GB word index is DISCARDED.
│       Rebuilt fresh at QUERY time via recall.rs:309 rebuild_entity_data →
│       rebuild_word_index_from_texts (same heavy structures, also ~2.3GB, also par_iter).
```

## Why it OOMs (root cause, measured)

The **BM25 lexical word-index** built in `add_docs_quantized` (and again at query time in
`rebuild_word_index_from_texts`). Each normalized word is stored ~5-9× as a separate
`String` across `doc_word_sets_fast`, `doc_word_tf_fast`, `word_inverted_fast`,
`vocabulary_fast`, `phonetic_index_fast`. At 33K code docs (huge identifier vocabulary) this
is ~2.3GB; the failing allocation is a single ~3.3GB map/Vec growth.

NOT the cause (ruled out this session): node_modules (excluded), O(N²) dedup (fixed), the
per-doc SCA encode (streamed to mmap, bounded), passage count (capping didn't help),
parallel parse. The SCA embedding path is already memory-bounded; the word index is not.

## Did "it used to work"?

The encode/word-index path is UNCHANGED from the original (b95a038 ran full Wonga → 53.6MB
brain, 33,447 memories — it completed, peaking high via pagefile on a clean machine). So the
~2.3GB word index has ALWAYS been there; full Wonga "worked" only by squeaking under the
limit via pagefile. On a pressured machine it fails. Subtrees (≤30K symbols) always fit.

## Regression I introduced (must fix, separate from OOM)

`is_junk_dir` over-broad: `_deploy/out/` (real deployment SQL) now excluded → that folder
dropped from 277 memories to 2. Must narrow the junk list to the doc-sanctioned set
(node_modules, target, .venv, .git-style + true build-artifact dirs) and NOT generic names
like `out`/`packages` that frequently hold real content.

## Fixes applied (TDD, one structure per commit, recall gate green after each)

1. **`is_junk_dir` narrowed** (89c7333) — removed generic names (`out`/`bin`/`obj`/`build`/
   `dist`/`packages`/`coverage`) that hold real content; kept only always-dependency/cache
   dirs (`node_modules`/`vendor`/`target`/tool-caches). Fixed the `_deploy/out/` regression
   (277 memories restored). `.gitignore` still honored for everything else.

2. **Word interning** (fe5e08b, 6555e83, 8448c5b, ee07095) — DONE. A single shared vocab
   (`word_vocab: Vec<String>` + `word_to_id: HashMap<String,u32>`, `intern_word()`) now backs
   all four word-keyed lexical structures, each migrated to `u32` keys/values one commit at a
   time with the recall gate (`test_lexical_interning` + `scripts/regression-check.sh`) green
   each step:
   - `phonetic_index_fast` (soundex→word-ids), `word_inverted_fast` (word-id→docset),
     `doc_word_sets_fast` (per-doc word-ids), `doc_word_tf_fast` (word-id→tf).
   - `vocabulary_fast` removed entirely (its words live in `word_to_id`).
   - Each unique word stored ONCE instead of ~9× as a String. Readers resolve query words
     via `word_to_id` / new `doc_has_word()` + `doc_words_joined()` helpers.
   - **Subtle bug fixed:** the clear sites (`clear`/`clear_word_index`/`rebuild_*`) must ALSO
     clear `word_vocab`+`word_to_id`, else recompute-on-growth re-interns words into an
     ever-growing vocab.
   - Verified: `lexical_word_index_bytes` (word-keyed only) 1,865 B/doc on a realistic
     vocab-reuse corpus; recall + lexical + sym + wikilinks + multi-hop all preserved.

Note: `doc_texts_fast` (raw per-doc normalized text) is NOT word-keyed and is unchanged — it
scales with content (big SQL chunks), not vocabulary. The passage-count explosion in the
SCA encode (char-chunked 512/256 → ~700 passages on a 400KB SQL chunk) is a SEPARATE concern
from the lexical index, tracked independently.
