# Phase 3b — stress-tests + the write-at-scale finding

## FINDING #11 (real, architectural): WRITE ops don't scale
On the 46,656-frame mega-brain, a SINGLE `learn-fix` (or `remember`) takes >2 MINUTES (timed out at
120s, still running). Root cause: every write triggers a FULL index rebuild (rebuild_trigram_index +
build_index over the whole corpus) after the append -- O(corpus), not O(new). Fine at 5k frames
(~150-190ms, doc #19 "flat, not O(N^2)"), CRIPPLING at 46.6k. The doc-19 "160-190ms/store, flat"
claim was measured at 42->202 frames; it does NOT hold at 46.6k.
IMPACT: recall (reads) is fine at scale (sym 0-1ms, ask ~450ms warm); WRITES are the scale wall.
This is WHY the stress-tests (which do many interleaved writes) couldn't complete via MCP at 46.6k.
FIX (park + grill): incremental/deferred index update on write (append to a pending buffer, rebuild
lazily or in a background compact), NOT a full rebuild per write. This is the single most important
scale finding -- it's the difference between "a live agent memory you write to constantly" and "a
build-once read-many index."

## FINDING #10 (perf): recall_fix ~7.5s at 46.6k
The float-rerank fix-recall (best_coding_fixes -> ask(deep, fetch=200) + fix-store union + re-encode)
scales with brain size: 0.6-2s at <10k frames, 7.5s at 46.6k. Reads are otherwise fast (sym sub-ms,
ask ~450ms warm). Tuning target: cap the deep-ask fetch harder + a cheap lexical prefilter before
re-encode.

## Stress-tests (could not complete at 46.6k due to #11 write cost)
- cross-project federation + LRU/LFU twin separation: PROVEN at SMALL/MEDIUM scale earlier (the
  edge-recall + coverage runs showed 100% recall + twin_precision test green). At 46.6k the WRITE cost
  (#11) blocked the stress harness before recall could be measured -- so twin/federation at 46.6k is
  UNMEASURED this run (honest), pending the write-scale fix.
- long-session (50 ops): blocked by #11 (each write >2min).

## Honest headline for Phase 3b
The stress-tests surfaced the REAL scale ceiling: it's not READS (fast at 46.6k) and not INGEST
(bounded, we proved 12 projects) -- it's WRITES (learn-fix/remember) doing a full index rebuild each
time. That's finding #11, the most actionable architectural item from the whole mega-run.
