#!/usr/bin/env bash
# REAL end-to-end TURNS-TO-FIX benchmark, headless, docs/23-aligned.
# 2 arms x 4 tasks x n>=5 samples. Each sample = a fresh claude --print agentic loop (write->test->fix)
# against the node gate (the sole judge). The ONLY difference between arms is memory:
#   cold   : empty .said, no hook  -> agent solves from scratch
#   memory : .said pre-loaded with the 4 VERIFIED goldens + `said setup` hook (nudge injection) -> the
#            agent recalls the fix/invariant instead of rediscovering it
# Metrics: pass@k (gate GREEN) + turns-to-first-success on co-solved + INVALID-exclusion (empty/timeout).
# The win = memory converges in FEWER turns (carries the non-obvious invariant the cold agent must find).
set -u
SAID="/g/development/said-build/said-coding.exe"
NODE="/c/nvm4w/nodejs/node"
SEED="/g/development/said-build/hard-eval/seed"
RES="/g/development/said-build/hard-eval/results"
ROOT="/g/cargo-tmp/turns_matrix"; rm -rf "$ROOT"; mkdir -p "$ROOT"
OUT="/tmp/turns_matrix_out"; rm -rf "$OUT"; mkdir -p "$OUT"
N="${SAMPLES:-5}"; TIMEOUT_S="${TIMEOUT_S:-420}"; RETRIES="${RETRIES:-1}"
[ "${SMOKE:-0}" != "1" ] && [ "$N" -lt 5 ] && { echo "REFUSED: real run needs SAMPLES>=5 (SMOKE=1 to validate)"; exit 2; }
LIMIT=$N; [ "${SMOKE:-0}" = "1" ] && LIMIT=1

declare -a TID=(h1_lru h2_intervals h3_store h4_ratelimiter)
declare -a EXTRA=("" "" "h3_index.js" "")   # extra seed src files a task needs (multi-file)
declare -a TASK=(
"Implement the LRUCache class in src/h1_lru.js: O(1) get/put, get counts as a use, put-update counts as a use, evict least-recently-used over capacity, get returns -1 if absent. Use the Bash tool to run 'node test/h1_lru.test.js' and fix until it passes. The interleaved-access stress test is the tricky one."
"Fix the bugs in mergeIntervals in src/h2_intervals.js (sort by start, merge touching intervals with <=, do not mutate inputs). Use the Bash tool to run 'node test/h2_intervals.test.js' and fix until it passes."
"Add TTL expiry to the Store class in src/h3_store.js: set(k,v,ttlMs) expires the key ttlMs after set using this.now(); expired keys absent for get/has/size, cleaned lazily; non-TTL keys never expire; re-set refreshes ttl; don't break existing behaviour. Use the Bash tool to run 'node test/h3_store.test.js' and fix until it passes."
"Implement the token-bucket RateLimiter in src/h4_ratelimiter.js: starts full, continuous fractional refill at refillPerSec capped at capacity, tryRemove(n,now) deducts only if enough tokens else false, available(now) returns current tokens. Use the Bash tool to run 'node test/h4_ratelimiter.test.js' and fix until it passes.")

# memory brain: load all 4 verified goldens (the "load all the data into memory" step)
MB="$ROOT/memory.said"; "$SAID" create "$MB" >/dev/null 2>&1
declare -a INV=(
"O(1) LRU = HashMap + doubly-linked list LRU(head)->MRU(tail); get/put-update move node to TAIL, evict head.next. NON-OBVIOUS INVARIANT (fails interleaved stress 4!==-1): on a put that TRIGGERS an eviction, insert the new key on the HEAD/LRU side NOT tail (insertAtHead=size>1)."
"mergeIntervals: SORT by start first; merge when iv[0] <= last[1] (touching counts, use <= not <); COPY intervals, never mutate the input arrays."
"Store TTL: store {v, exp}; exp = now()+ttlMs (null = never); on every get/has/size check now()>=exp and lazily delete; use this.now() not Date.now(); re-set refreshes exp; don't break non-TTL keys."
"Token bucket: tokens start = capacity; on each op refill tokens += (now-last)/1000*rate capped at capacity, advance last; tryRemove deducts only if tokens>=n else false (no deduction); available() refills then returns tokens.")
for i in 0 1 2 3; do
  "$SAID" --path "$MB" learn-fix --problem "${TASK[$i]%% Use the Bash*}" \
    --edits '[{"file":"src/'"${TID[$i]}"'.js","mode":"write-file"}]' --learnings "${INV[$i]}" --label "${TID[$i]}" >/dev/null 2>&1
done
echo "=== memory loaded: $(for i in 0 1 2 3; do "$SAID" --path "$MB" recall-fix --problem "${TASK[$i]%% Use the Bash*}" 2>/dev/null | grep -oE 'Fix \([0-9.]+\)' | head -1; done | tr '\n' ' ')"

setup_dir () { # idx dir
  local idx="$1" d="$2"
  local tid="${TID[$idx]}"
  rm -rf "$d"; mkdir -p "$d/src" "$d/test"
  cp "$SEED/src/$tid.js" "$d/src/$tid.js"; cp "$SEED/test/$tid.test.js" "$d/test/$tid.test.js"
  [ -n "${EXTRA[$idx]}" ] && cp "$SEED/src/${EXTRA[$idx]}" "$d/src/${EXTRA[$idx]}"
}

run_one () { # arm idx sample -> "turns pass valid"
  local arm="$1" idx="$2" s="$3"
  local tid="${TID[$idx]}"
  local d="$ROOT/${arm}_${tid}_s${s}"; setup_dir "$idx" "$d"
  if [ "$arm" = "memory" ]; then cp "$MB" "$d/code.said"; ( cd "$d" && "$SAID" --path "$d/code.said" setup >/dev/null 2>&1 ); fi
  local f="$OUT/${arm}_${tid}_s${s}.json" attempt=0
  while [ "$attempt" -le "$RETRIES" ]; do
    ( cd "$d" && timeout "$TIMEOUT_S" claude --print --permission-mode bypassPermissions --output-format json "${TASK[$idx]}" </dev/null >"$f" 2>/dev/null )
    [ -s "$f" ] && grep -q '"result":"' "$f" 2>/dev/null && break
    attempt=$((attempt+1))
  done
  if [ ! -s "$f" ] || ! grep -q '"result":"' "$f" 2>/dev/null; then echo "NA NA INVALID"; return; fi
  local turns; turns=$(grep -oE '"num_turns":[0-9]+' "$f" | grep -oE '[0-9]+' | head -1)
  local pass=0; ( cd "$d" && "$NODE" "test/$tid.test.js" >/dev/null 2>&1 ) && pass=1
  echo "${turns:-NA} $pass VALID"
}

printf "arm|task|sample|turns|pass|valid\n" > "$OUT/results.psv"
for arm in cold memory; do
  for i in 0 1 2 3; do
    for s in $(seq 1 "$LIMIT"); do
      read -r turns pass valid <<< "$(run_one "$arm" "$i" "$s")"
      printf "%s|%s|%s|%s|%s|%s\n" "$arm" "${TID[$i]}" "$s" "$turns" "$pass" "$valid" >> "$OUT/results.psv"
      echo "$arm/${TID[$i]}/s$s turns=$turns pass=$pass ($valid)"
    done
  done
done
echo "=== AGGREGATE (pass@1, avg turns-to-GREEN, INVALID excluded) ==="
awk -F'|' 'NR>1 && $6=="VALID"{ n[$1"|"$2]++; if($5=="1"){c[$1"|"$2]++; t[$1"|"$2]+=$4; tc[$1"|"$2]++} }
END{ for(k in n){ split(k,p,"|"); printf "  %-7s %-14s pass@1=%.2f (%d/%d)  avgTurns(green)=%.1f\n",p[1],p[2],c[k]/n[k],c[k]+0,n[k],(tc[k]>0?t[k]/tc[k]:0) } }' "$OUT/results.psv" | sort
echo "DONE -> $OUT/results.psv"
