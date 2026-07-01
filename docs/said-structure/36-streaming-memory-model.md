# 36 — Streaming ingest: the memory model (SPIMI, one budget)

How `.said` ingests a corpus of ANY size — a handful of files or a 40,000-file monorepo — while keeping
**peak RAM bounded to a single per-device budget**. This is the canonical **SPIMI** (Single-Pass
In-Memory Indexing) + external-merge pattern that Lucene (`IndexWriter`), Tantivy, and RocksDB
(memtable → SST) all use: a bounded in-RAM buffer, flushed to disk when full, so peak memory is a
function of the *budget*, not the corpus size.

## One coordinated budget (not three)

Ingest has three memory-hungry phases. Historically each had its OWN budget knob, which meant they
could **stack** to ~3× the intended peak. They are now derived from ONE `IngestBudget`
([said-cli `IngestBudget::resolve`](../../crates/said-cli/src/main.rs)):

```
total = clamp(available_RAM × 12%, 16 MB, 512 MB)
        ├─ frames      40%   — the raw-frame pending buffer (spills to <path>.spill when full)
        ├─ encode      40%   — the passage-embedding window (index_batch processes docs in windows)
        └─ word-index  20%   — the WIDX inverted-index build (derived + windowed under this slice)
```

- **Fraction, not fixed** — `12%` of *available* RAM (Elasticsearch/Lucene use ~10% of heap; DuckDB 80%
  of RAM). A fixed 512 MB would waste RAM on a 4 GB phone and under-budget a 256 GB server.
- **16 MB floor** — Lucene's `IndexWriter` default; keeps a low-RAM device usable.
- **512 MB ceiling** — spilling costs throughput (measured ~14× slower read when the frame buffer
  spills; Spark/PostgreSQL guidance: *budget high, avoid spilling unless forced*). So a capable machine
  hits the ceiling and does NOT force-spill on normal repos; only a genuinely low-RAM device drops below
  it and spills earlier. **The ceiling is the low-RAM-device guarantee, not a tax on workstations.**

Per device: mobile (2–4 GB) → ~16–480 MB (spills, bounded); laptop/desktop/server → 512 MB (ceiling,
fast). Override the total with `SAID_INGEST_BUDGET` (bytes); explicit `SAID_SPILL_BUDGET` /
`SAID_INDEX_BUDGET` per-phase overrides are still respected.

## The invariant — peak RAM ≈ budget, independent of corpus size

This is the whole point, and it's measured. On the African-bank subset, with `SAID_INGEST_BUDGET=125 MB`:

| Corpus | Frames | Peak private heap |
|---|---|---|
| AfricanBank/src | 4,554 | **161 MB** |
| Full AfricanBank | 8,781 (~2×) | **220 MB** |

Doubling the corpus barely moved the peak (161 → 220 MB, sub-linear) — it did **not** double. Peak
tracks the *budget* (125 MB + encoder + fixed overhead), not the frame count. That is the SPIMI
guarantee: a 10-file repo pays no segment/merge overhead (one buffer, no flush), and a 40k-file repo
stays bounded (buffer flushes to spill, RAM released).

## The three phases (how each stays bounded)

1. **Frame buffering** (`SAID_SPILL_BUDGET` ← frames slice). Raw frames accumulate in a pending buffer;
   when `frames.pending_bytes()` exceeds the slice, they spill to a `<path>.spill` scratch file and the
   committed frames page from the OS cache, not process RAM ([INIT-ROUTE-TRACE](INIT-ROUTE-TRACE.md) #4).
2. **Passage encoding** (`SAID_INDEX_BUDGET` ← encode slice). `index_batch` processes docs in windows
   sized from the slice: par-encode a window → fold serially in doc order → drop it. Peak = one window.
   Bit-identical to the un-windowed build ([`test_index_budget_streaming`](../../crates/sca-core/tests/test_index_budget_streaming.rs)).
3. **Word index** (word-index slice). The BM25 inverted index is **derived** (transpose of the per-doc
   sets) rather than accumulated, and serialized to the on-disk **WIDX** section
   ([3.6](03-core-subsystems/3.6-trigram-symbol-index.md) sibling; [`word_index.rs`](../../crates/sca-core/src/word_index.rs)).
   A re-opened brain reads postings in place from the mmap'd WIDX + loads `word_idf` verbatim (v2), so a
   cold query never rebuilds the 2.3 GB word index ([34](34-ultimate-stretch.md) / doc 35 speed notes).

## Stability properties (what a correct streaming design guarantees)

- **Peak RAM independent of corpus size** — measured above.
- **Deterministic output** — postings sorted before serialization, frame ids deterministic, absolute
  offsets, so the `.said` file is identical regardless of *when* a flush happened.
- **Crash-safe** — spill scratch is temporary; the `.said` is written via temp-then-atomic-rename with a
  CRC32; a failed index propagates a nonzero exit (no silent empty-brain save).
- **Small-input efficient** — a small repo fits one buffer: no spill, no windowing overhead (measured:
  110-file init in ~1.4 s, cold `said ask` ~140 ms).

## See also

- [INIT-ROUTE-TRACE](INIT-ROUTE-TRACE.md) — the frame-store spill (#4) mechanics.
- [11 known-limitations §1.6](11-known-limitations.md) — the large-repo ingest history + open items.
- [35 production build](35-production-build.md) — the resident MCP / `said serve` warm path.
- Sources: Manning IR (SPIMI/BSBI, ch.4); Aggarwal-Vitter 1988 (external-memory I/O); Apache Lucene
  `IndexWriter` (`ramBufferSizeMB` + TieredMergePolicy); Tantivy `IndexWriter`; RocksDB LSM (memtable →
  SST → compaction).
