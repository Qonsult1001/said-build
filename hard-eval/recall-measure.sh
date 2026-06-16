#!/usr/bin/env bash
# MEASURE recall (b): with the real LRU learning + N plausible decoys in the brain,
# does recall surface the RIGHT learning for LRU-shaped queries? Reports the
# matched doc_id + score per query for BOTH scorers:
#   - CLI recall-fix  (best_fix_for: 0.9*action_fp + 0.1*target_overlap)
#   - ask fusion      (best_iteration's signal: text/BM25 confidence)
# No code changes — pure measurement to locate the break before fixing.
set -u
ROOT="/g/development/said-build/hard-eval"
SAID="/g/development/said-build/target/release/said.exe"
BRAIN="$ROOT/results/recall_decoys.said"

rm -f "$BRAIN"
"$SAID" create "$BRAIN" >/dev/null 2>&1

# Minimal valid edits payload for decoys (content irrelevant to recall scoring).
ed() { printf '[{"file":"src/%s.js","mode":"write-file","content":"// %s\\n"}]' "$1" "$1"; }

# --- THE REAL TARGET ---
"$SAID" --path "$BRAIN" learn-fix \
  --problem "Implement an LRU cache: O(1) get/put, get and put-update count as a use, evict least-recently-used over capacity, get returns -1 if absent" \
  --edits "$(ed lru)" \
  --learnings "Doubly-linked list + Map; non-obvious: a new key that triggered an eviction inserts at the LRU side, not MRU." \
  --label "TARGET:lru" >/dev/null 2>&1

# --- DECOYS: plausible, vocabulary-overlapping coding fixes ---
"$SAID" --path "$BRAIN" learn-fix --problem "Add a TTL expiry cache: set(k,v,ttlMs) expires after ttlMs, expired keys absent for get/has, non-TTL keys never expire" --edits "$(ed ttl)" --learnings "Store expiry timestamp per key; lazy cleanup on access." --label "DECOY:ttl-cache" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Implement a token-bucket rate limiter: continuous fractional refill capped at capacity, tryRemove deducts if enough tokens else false" --edits "$(ed bucket)" --learnings "Track tokens + last refill time; refill on demand." --label "DECOY:ratelimiter" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Add a write-through cache in front of the database: get checks cache then db, put writes both, invalidate on delete" --edits "$(ed wt)" --learnings "Cache aside vs write-through; keep db source of truth." --label "DECOY:writethrough-cache" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Implement an in-memory key-value store with get put delete size, no expiry, O(1) operations" --edits "$(ed kv)" --learnings "Plain Map wrapper; size via map.size." --label "DECOY:kv-store" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Fix merge intervals: sort by start, merge touching intervals, do not mutate input" --edits "$(ed iv)" --learnings "Sort first; merge when start<=prevEnd." --label "DECOY:intervals" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Add an LFU cache: evict least-frequently-used, tie-break by least-recently-used, O(1) get and put" --edits "$(ed lfu)" --learnings "Freq buckets + DLL per freq; min-freq pointer." --label "DECOY:lfu-cache" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Implement a fixed-size ring buffer queue: enqueue dequeue, overwrite oldest when full, O(1)" --edits "$(ed ring)" --learnings "head/tail mod capacity; full vs empty flag." --label "DECOY:ringbuffer" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Add memoization cache to a recursive function: cache results by args, return cached on repeat call" --edits "$(ed memo)" --learnings "Map keyed by serialized args." --label "DECOY:memoize" >/dev/null 2>&1
"$SAID" --path "$BRAIN" learn-fix --problem "Implement a priority queue (binary heap): push pop peek, O(log n) push/pop, min-heap" --edits "$(ed heap)" --learnings "Array heap; sift up/down." --label "DECOY:heap" >/dev/null 2>&1

echo "=== brain built: $("$SAID" --path "$BRAIN" stats 2>/dev/null | grep -i 'doc\|frame' | head -1) ==="
echo ""

# Queries that SHOULD all map to the LRU target.
QUERIES=(
  "Implement an LRU cache with O(1) get and put and LRU eviction"
  "implement LRU cache eviction least recently used"
  "least recently used cache evict when over capacity"
  "build a cache that evicts the least recently used entry"
  "LRUCache get put evict O(1)"
)

printf "%-62s | %-26s | %-8s\n" "QUERY" "recall-fix match" "fusion#1"
printf "%s\n" "--------------------------------------------------------------+----------------------------+---------"
for q in "${QUERIES[@]}"; do
  # CLI recall-fix at a LOW floor so we see the score even on weak matches.
  rf=$("$SAID" --path "$BRAIN" recall-fix --problem "$q" --min-similarity 0.0 --json 2>/dev/null)
  rf_label=$(printf '%s' "$rf" | "/c/nvm4w/nodejs/node" -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{const j=JSON.parse(s);if(!j.fix){console.log("NULL");}else{console.log((j.fix.provenance||j.fix.doc_id||"?")+" @"+(j.fix.score!==undefined?j.fix.score.toFixed(2):"?"));}}catch(e){console.log("parse-err");}})')
  # Fusion top-1 (orchestrator signal): first ask result line.
  f1=$("$SAID" --path "$BRAIN" ask "$q" 2>/dev/null | grep -oE "\[0\.[0-9]+\]\[[a-z]+\] (fix|fixaction)::[0-9a-f]+" | head -1)
  printf "%-62.62s | %-26.26s | %-8s\n" "$q" "$rf_label" "$f1"
done

echo ""
echo "TARGET doc_id (the right answer):"
"$SAID" --path "$BRAIN" grep "TARGET:lru" 2>/dev/null | grep -oE "fix::[0-9a-f]+" | head -1 || echo "(use recall-fix labels above)"
