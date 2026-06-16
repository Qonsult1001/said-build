#!/usr/bin/env bash
# MARGIN: for each LRU query, list the TOP-3 recall-fix candidates with scores,
# so we see how far the TARGET sits above the best DECOY. A safe confidence floor
# needs a real gap. Uses recall-fix at floor 0 + a tiny patch: we can't get top-3
# from the CLI (it returns only #1), so we measure separation differently —
# run each query, record target score; then run a DECOY-shaped query, record what
# the LRU target scores on it (cross-talk). Big target-on-LRU vs target-on-decoy
# gap = separable.
set -u
ROOT="/g/development/said-build/hard-eval"
SAID="/g/development/said-build/target/release/said.exe"
BRAIN="$ROOT/results/recall_decoys.said"
NODE="/c/nvm4w/nodejs/node"

score_of() { # $1=query  -> "provenance @score"
  "$SAID" --path "$BRAIN" recall-fix --problem "$1" --min-similarity 0.0 --json 2>/dev/null \
    | "$NODE" -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{const j=JSON.parse(s);if(!j.fix)console.log("NULL @0");else console.log((j.fix.provenance||"?")+" @"+j.fix.score.toFixed(3));}catch(e){console.log("err @0");}})'
}

echo "=== LRU queries (should hit TARGET:lru, high) ==="
for q in \
  "Implement an LRU cache with O(1) get and put and LRU eviction" \
  "LRUCache get put evict O(1)" ; do
  printf "  %-58.58s -> %s\n" "$q" "$(score_of "$q")"
done

echo ""
echo "=== DECOY queries (should hit their OWN decoy, NOT lru) ==="
for q in \
  "Add a TTL expiry cache where keys expire after a timeout" \
  "Implement a token-bucket rate limiter with fractional refill" \
  "Implement an LFU cache evict least frequently used" \
  "in-memory key-value store get put delete size no expiry" \
  "fixed-size ring buffer queue overwrite oldest when full" ; do
  printf "  %-58.58s -> %s\n" "$q" "$(score_of "$q")"
done

echo ""
echo "READ: if decoy queries return their OWN provenance (not TARGET:lru), ranking"
echo "is correct. The CONCERN is the SCORE band (0.5-0.65) being too compressed to"
echo "set one safe floor. That's the fix target: score separation, not ranking."
