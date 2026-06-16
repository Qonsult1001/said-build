#!/usr/bin/env bash
# Decompose the score: print action_score and target_score (Jaccard + one-sided)
# separately for the LRU target vs the best decoy, so we fix the RIGHT component.
# Adds a tiny debug env the scorer honors (SAID_FIX_SCORE_DEBUG) if present;
# otherwise just shows the final score from recall-fix at floor 0.
set -u
ROOT="/g/development/said-build/hard-eval"
SAID="/g/development/said-build/target/release/said.exe"
BRAIN="$ROOT/results/recall_decoys.said"
NODE="/c/nvm4w/nodejs/node"

# Rebuild the decoy brain fresh (measure script does this; re-call it quietly).
bash "$ROOT/recall-measure.sh" >/dev/null 2>&1

echo "=== action_residue of each problem (what the fingerprint sees) ==="
echo "LRU query : $("$SAID" --path "$BRAIN" recall-fix --problem "x" >/dev/null 2>&1; echo)"
for q in \
  "Implement an LRU cache with O(1) get and put and LRU eviction" \
  "Implement an LFU cache evict least frequently used" \
  "in-memory key-value store get put delete size no expiry" ; do
  # action residue = drop CamelCase/has-digit/snake/path tokens
  ar=$(printf '%s' "$q" | "$NODE" -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const out=[];for(const raw of s.trim().split(/\s+/)){const t=raw.replace(/^[^A-Za-z0-9_\/]+|[^A-Za-z0-9_\/]+$/g,"");if(!t)continue;const firstUpper=/^[A-Z]/.test(t);const isTarget=t.includes("/")||t.includes("_")||/[0-9]/.test(t)||(firstUpper&&/[A-Z]/.test(t.slice(1)))||((t.match(/[A-Z]/g)||[]).length>=2);if(!isTarget)out.push(t.toLowerCase());}console.log(out.join(" "));})')
  printf "  %-58.58s\n    action='%s'\n" "$q" "$ar"
done
echo ""
echo "READ: if 'lru' is MISSING from the LRU action residue (stripped as a target"
echo "token), the intent fingerprint cannot use the most discriminating word — that"
echo "is the root cause. Fix = keep a normalized 'lru' in the matched signal."
