#!/usr/bin/env bash
# EDGE-TIER accuracy A/B on h1_lru (the task Claude may fail cold). MCP/CLI surface, memory-safe.
#   cold  : no memory, agent solves from scratch
#   memory: brain seeded with the verified golden LRU (+ the non-obvious invariant) via aligned learn-fix;
#           the `said setup` hook injects it the nudge way at prompt time
# Gate = node test (sole judge). Metric = gate-pass@k + attempts. INVALID (empty/timeout) excluded.
# Validate SMOKE=1 (1 each) before n>=5. NO orchestrator — this is the product hook surface.
set -u
SAID="/g/development/said-build/said-coding.exe"
NODE="/c/nvm4w/nodejs/node"
SEED="/g/development/said-build/hard-eval/seed"
MB="/g/cargo-tmp/edge_lru.said"           # pre-seeded memory brain (built by caller / below)
ROOT="/g/cargo-tmp/edge_ab"; rm -rf "$ROOT"; mkdir -p "$ROOT"
OUT="/tmp/edge_ab_out"; rm -rf "$OUT"; mkdir -p "$OUT"
N="${SAMPLES:-5}"; TIMEOUT_S="${TIMEOUT_S:-300}"
[ "${SMOKE:-0}" != "1" ] && [ "$N" -lt 5 ] && { echo "REFUSED: real run needs SAMPLES>=5 (SMOKE=1 to validate)"; exit 2; }
LIMIT=$N; [ "${SMOKE:-0}" = "1" ] && LIMIT=1

TASK="Implement the LRUCache class in h1_lru.js in the current directory: O(1) get/put, get counts as a use, put-update counts as a use, evict least-recently-used over capacity, get returns -1 if absent. Run node h1_lru.test.js until it passes. The interleaved-access stress test (test 5) is the tricky one."

# (re)build the memory brain from the verified golden LRU if missing (memory-safe: golden is gate-green)
if [ ! -s "$MB" ]; then
  "$SAID" create "$MB" >/dev/null 2>&1
  LEARN="ROOT APPROACH: O(1) LRU = HashMap(key->node) + doubly-linked list LRU(head)->MRU(tail). get/put-update move node to TAIL; evict head.next. NON-OBVIOUS INVARIANT (textbook trap, fails interleaved stress with 4!==-1): when a put TRIGGERS an eviction, insert the new key on the HEAD/LRU side, NOT tail (insertAtHead = size>1). Moving on access to tail but inserting post-evict at tail too SILENTLY breaks eviction order."
  "$SAID" --path "$MB" learn-fix --problem "Implement an LRU cache: O(1) get/put, get counts as a use, evict least-recently-used over capacity, return -1 if absent; pass interleaved stress" --edits '[{"file":"h1_lru.js","mode":"write-file","symbol":"LRUCache"}]' --learnings "$LEARN" --label "lru_golden" >/dev/null 2>&1
fi

run_one () { # arm sample -> echoes "pass valid"
  local arm="$1" s="$2"
  local d="$ROOT/${arm}_s${s}"; rm -rf "$d"; mkdir -p "$d"
  mkdir -p "$d/src"; cp "$SEED/src/h1_lru.js" "$d/src/h1_lru.js"; mkdir -p "$d/test"; cp "$SEED/test/h1_lru.test.js" "$d/test/h1_lru.test.js"
  if [ "$arm" = "memory" ]; then
    cp "$MB" "$d/code.said"
    ( cd "$d" && "$SAID" --path "$d/code.said" setup >/dev/null 2>&1 )
  fi
  local f="$OUT/${arm}_s${s}.json"
  # run claude IN the task dir via a subshell (absolute redirect path; no cwd leakage to the harness)
  ( cd "$d" && timeout "$TIMEOUT_S" claude --print --permission-mode bypassPermissions --output-format json "$TASK" >"$f" 2>/dev/null )
  # INVALID if no usable output (empty / no result) — exclude, don't score as RED
  if [ ! -s "$f" ] || ! grep -q '"result":"' "$f" 2>/dev/null; then echo "NA INVALID"; return; fi
  if "$NODE" "$d/test/h1_lru.test.js" >/dev/null 2>&1; then echo "1 VALID"; else echo "0 VALID"; fi
}

printf "arm|sample|pass|valid\n" > "$OUT/results.psv"
for arm in cold memory; do
  for s in $(seq 1 "$LIMIT"); do
    read -r p v <<< "$(run_one "$arm" "$s")"
    printf "%s|%s|%s|%s\n" "$arm" "$s" "$p" "$v" >> "$OUT/results.psv"
    echo "$arm/s$s pass=$p ($v)"
  done
done
echo "=== EDGE A/B ==="
awk -F'|' 'NR>1 && $4=="VALID"{n[$1]++; c[$1]+=$3} END{for(a in n) printf "  %-7s pass@1=%.2f  (%d/%d valid)\n",a,c[a]/n[a],c[a],n[a]}' "$OUT/results.psv"
echo "DONE -> $OUT/results.psv"
