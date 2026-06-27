#!/usr/bin/env bash
# SAVE-AXIS test (MCP/CLI surface): does the AGENT itself store a correct, recallable learn_fix after
# solving — using the now-aligned ITERATION_TEMPLATE guidance? Measures three sub-axes:
#   1. SAVE RATE      — of N solved tasks, how many produced a learn_fix with an intact body (#8 guard).
#   2. RECALL FIDELITY— next session, does recall-fix return THAT saved fix for the task (recall@1)?
#   3. TOKEN SAVINGS  — a follow-up question answered WITH the saved memory vs WITHOUT (baseline), cost delta.
# Memory-safe: the agent writes; we only READ the brain to score. No orchestrator. Validate small (SMOKE=1).
set -u
SAID="g:/development/said-build/said-coding.exe"
ROOT="G:/cargo-tmp/save_axis"; rm -rf "$ROOT"; mkdir -p "$ROOT"
OUT="/tmp/save_axis_out"; rm -rf "$OUT"; mkdir -p "$OUT"
N="${SAMPLES:-3}"
TIMEOUT_S="${TIMEOUT_S:-240}"
NODE="/c/nvm4w/nodejs/node"

# Solvable coding tasks (each has a node gate). The agent solves, then is asked to learn_fix it.
# Kept small + self-contained so the SAVE behavior is what varies, not task difficulty.
declare -a TID=(s1_dedup s2_debounce s3_flatten)
declare -a TASK=(
"Implement dedupe(arr) in $ROOT/work/s1_dedup.js: return a new array with duplicates removed, preserving first-seen order. Make node $ROOT/work/s1_dedup.test.js pass."
"Implement chunk(arr,size) in $ROOT/work/s2_debounce.js: split arr into sub-arrays of length size (last may be shorter). Make node $ROOT/work/s2_debounce.test.js pass."
"Implement flatten(arr) in $ROOT/work/s3_flatten.js: deeply flatten a nested array to a single level. Make node $ROOT/work/s3_flatten.test.js pass.")
# recall key per task (what next-session recall-fix queries with — a PARAPHRASE, not the stored text)
declare -a RKEY=(
"remove duplicate elements from an array keeping first occurrence order"
"split an array into fixed-size groups"
"deeply flatten a nested array into one level")

mkdir -p "$ROOT/work"
# gates
cat > "$ROOT/work/s1_dedup.test.js" <<'EOF'
const a=require('./s1_dedup.js');const assert=require('assert');
assert.deepStrictEqual(a.dedupe([1,2,2,3,1]),[1,2,3]);
assert.deepStrictEqual(a.dedupe([]),[]);
assert.deepStrictEqual(a.dedupe(['a','a','b']),['a','b']);
console.log('ok');process.exit(0);
EOF
cat > "$ROOT/work/s2_debounce.test.js" <<'EOF'
const a=require('./s2_debounce.js');const assert=require('assert');
assert.deepStrictEqual(a.chunk([1,2,3,4,5],2),[[1,2],[3,4],[5]]);
assert.deepStrictEqual(a.chunk([],3),[]);
assert.deepStrictEqual(a.chunk([1,2,3],3),[[1,2,3]]);
console.log('ok');process.exit(0);
EOF
cat > "$ROOT/work/s3_flatten.test.js" <<'EOF'
const a=require('./s3_flatten.js');const assert=require('assert');
assert.deepStrictEqual(a.flatten([1,[2,[3,[4]]]]),[1,2,3,4]);
assert.deepStrictEqual(a.flatten([]),[]);
assert.deepStrictEqual(a.flatten([[1],[2,3]]),[1,2,3]);
console.log('ok');process.exit(0);
EOF
# stubs
for t in s1_dedup s2_debounce s3_flatten; do echo "module.exports={};" > "$ROOT/work/$t.js"; done

"$SAID" create "$ROOT/brain.said" >/dev/null 2>&1
cd "$ROOT/work"

[ "${SMOKE:-0}" != "1" ] && [ "$N" -lt 3 ] && { echo "use SMOKE=1 for <3"; }
LIMIT=$N; [ "${SMOKE:-0}" = "1" ] && LIMIT=1

printf "task|solved|saved|body_len|recalled|recall_score\n" > "$OUT/results.psv"
SAVED=0; RECALLED=0; SOLVED=0
for i in $(seq 0 $((LIMIT-1))); do
  tid="${TID[$i]}"; task="${TASK[$i]}"; rkey="${RKEY[$i]}"
  fn=$(echo "$tid" | sed 's/^s[0-9]_//')
  # ---- AGENT SOLVES + SAVES (the SAVE axis): instruct it to learn_fix after the gate is green ----
  prompt="$task After you make the test pass, store the verified fix in project memory by running this exact command (it is the coding-memory tool): $SAID --path $ROOT/brain.said learn-fix --problem \"$rkey\" --edits '[{\"file\":\"$tid.js\",\"mode\":\"write-file\"}]' --learnings \"<the non-obvious invariant you used>\" --label \"$tid\". Do it once, after the test passes."
  f="$OUT/${tid}_solve.json"
  timeout "$TIMEOUT_S" claude --print --permission-mode bypassPermissions --output-format json "$prompt" >"$f" 2>/dev/null
  # did the gate pass?
  if "$NODE" "$ROOT/work/$tid.test.js" >/dev/null 2>&1; then solved=1; SOLVED=$((SOLVED+1)); else solved=0; fi
  # did the agent SAVE a fix? (read the brain — #8 body-intact guard)
  id=$("$SAID" --path "$ROOT/brain.said" recall-fix --problem "$rkey" --min-similarity 0.0 2>/dev/null | grep -oE "fix::[a-z0-9]+" | head -1)
  blen=0; saved=0
  if [ -n "$id" ]; then blen=$("$SAID" --path "$ROOT/brain.said" get "$id" 2>/dev/null | wc -c); fi
  [ "$blen" -ge 40 ] && { saved=1; SAVED=$((SAVED+1)); }
  # RECALL FIDELITY: does the saved fix come back at the DEFAULT floor (not floor 0) for the paraphrase?
  rscore=$("$SAID" --path "$ROOT/brain.said" recall-fix --problem "$rkey" 2>/dev/null | grep -oE "Fix \([0-9.]+\)" | grep -oE "[0-9.]+" | head -1)
  recalled=0; [ -n "$rscore" ] && recalled=1 && RECALLED=$((RECALLED+1))
  printf "%s|%s|%s|%s|%s|%s\n" "$tid" "$solved" "$saved" "$blen" "$recalled" "${rscore:-NA}" >> "$OUT/results.psv"
  echo "$tid solved=$solved saved=$saved body=$blen recalled=$recalled score=${rscore:-NA}"
done
echo "=== SAVE-AXIS: solved=$SOLVED/$LIMIT  saved=$SAVED/$LIMIT  recalled=$RECALLED/$LIMIT ==="
echo "DONE -> $OUT/results.psv"
