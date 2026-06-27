#!/usr/bin/env bash
# RESEARCH-CORRECT benchmark (replaces single-run pass@1).
# Metrics (cited): pass@k [2107.03374], turns-to-first-success / Cost-of-Pass [2504.13359],
# abstention as a CORRECT outcome [2207.05221, 2006.09462, 2502.09054]. Reflexion-style multi-sample.
#   - n samples/task/arm (default 5) -> pass@1 = c/n, pass@k = unbiased estimator.
#   - turns-to-first-success on tasks BOTH arms solve (the "how many turns to converge" the user wants).
#   - tasks partitioned: memory-only-solves / co-solved / neither (abstention candidates).
#   - abstention: the prompt invites a clean give-up; a correct give-up on an unanswerable task scores
#     as ABSTAIN-CORRECT (not failure); grind-then-wrong is the WORST outcome.
set -u
SAID="g:/development/said-build/said-coding.exe"
ROOT="G:/cargo-tmp/bench_research"; rm -rf "$ROOT"; mkdir -p "$ROOT"
OUT="/tmp/bench_research_out"; rm -rf "$OUT"; mkdir -p "$OUT"
N="${SAMPLES:-5}"

cp -r "g:/development/said-build/crates" "$ROOT/crates"
"$SAID" create "$ROOT/code.said" >/dev/null 2>&1
"$SAID" --path "$ROOT/code.said" init "$ROOT/crates" 2>&1 | grep -iE "memories added" | head -1
cp "$ROOT/code.said" "$ROOT/baseline.said"
cp "$ROOT/code.said" "$ROOT/memory.said"

# seed 5 verified fixes (clean make_fix_note; #8 fixed so all bodies persist)
declare -a LB=(decision-no-callgraph trgm-sym-reopen encoder-must-be-default steering-channel doc-comment-chunk)
declare -a FI=(ask.rs said_file.rs said_file.rs steering.rs code_search.rs)
declare -a PR=(
"Why does .said ask() deliberately not walk the code call-graph; what does it return and what does the type-precise work"
"sym() returns 0 after a said init reopen — which function/section is responsible and the root-cause why"
"The embed-model encoder gotcha that silently killed semantic recall — what was the condition"
"Why does the agent-steering hook use UserPromptSubmit instead of PreToolUse additionalContext"
"Why did descriptive queries fail to retrieve build_concept_links, and the fix")
declare -a LE=(
"DECISION: ask() does NOT walk the call-graph — shallow untyped traversal duplicating the LSP. ask() returns stored facts; the LLM hands symbols to the language server for type-precise references. Call-graph is code_calls/code_callers verbs only."
"ROOT CAUSE: SaidFile::save() skipped the TRGM section when trigram postings were empty, so trigram_doc_ids was empty on reopen and the symbol doc_index->doc_id translation failed. FIX: persist TRGM whenever a symbol_index exists."
"GOTCHA: the embed-model static encoder must be DEFAULT not opt-in. When opt-in, build_index produced ZERO fingerprints and semantic recall was silently dead."
"DECISION: steering injects via UserPromptSubmit with factual framing, NOT PreToolUse additionalContext. WHY: tool-result-adjacent context is the lowest-trust slot — the model flags it as prompt-injection."
"ROOT CAUSE: ast_chunk indexed only a definition's own byte range, but doc-comments are SIBLING nodes before it — so the richest description was unindexed. FIX: prepend leading doc-comments to the chunk.")
for i in 0 1 2 3 4; do
  edits='[{"file":"crates/sca-core/src/'"${FI[$i]}"'","mode":"replace-symbol","symbol":"x"}]'
  "$SAID" --path "$ROOT/memory.said" learn-fix --problem "${PR[$i]}" --edits "$edits" --learnings "${LE[$i]}" --label "${LB[$i]}" >/dev/null 2>&1
done
# VERIFY bodies (the #8 guard)
for i in 0 1 2 3 4; do
  id=$("$SAID" --path "$ROOT/memory.said" recall-fix --problem "${PR[$i]}" --min-similarity 0.0 2>&1 | grep -oE "fix::[a-z0-9]+" | head -1)
  len=$("$SAID" --path "$ROOT/memory.said" get "$id" 2>/dev/null | wc -c)
  [ "$len" -lt 40 ] && { echo "SEED CORRUPT ${LB[$i]} ($len) — abort"; exit 1; }
done
echo "=== fixture clean (5/5 bodies) ==="

cd "$ROOT"
"$SAID" --path "$ROOT/code.said" setup >/dev/null 2>&1
cp .claude/settings.local.json .claude/settings.hookarm.json 2>/dev/null

# Questions span difficulty; A_HARD is an ABSTENTION probe (the answer is in NO fix and NO file as prose).
TASKS=(
'Q1|Why does .said ask() deliberately NOT walk the code call-graph? What does it return instead?|LSP|language server|call-graph|stored fact'
'Q2|What was the root cause of sym() returning 0 after a said init reopen, and which section had to be persisted?|TRGM'
'Q3|What condition silently killed semantic recall (the embed-model encoder gotcha)?|opt-in|default|zero|fingerprint'
'Q4|Why were descriptive queries failing to retrieve build_concept_links, and what was the fix?|doc-comment|comment|sibling|byte range'
'QABSTAIN|What is the exact git commit SHA that introduced the vault header feature?|__ABSTAIN__'
)
# QABSTAIN gold = __ABSTAIN__ : correct iff the agent GIVES UP cleanly (no fabricated SHA).

# Prompt suffix: invite clean abstention (the turn-budget proxy in a single --print run).
ABSTAIN_HINT=" If you cannot find a confident answer from project memory or a quick look, say 'I do not have this' and stop rather than guessing or exhaustively searching."

run () { # arm idx q s
  local arm="$1" idx="$2" q="$3" s="$4"
  cp .claude/settings.hookarm.json .claude/settings.local.json
  if [ "$arm" = "baseline" ]; then cp "$ROOT/baseline.said" "$ROOT/code.said"; else cp "$ROOT/memory.said" "$ROOT/code.said"; fi
  local f="$OUT/${arm}_${idx}_s${s}.json"
  timeout 180 claude --print --permission-mode bypassPermissions --output-format json "$q$ABSTAIN_HINT" >"$f" 2>/dev/null
  local t c; t=$(grep -oE '"num_turns":[0-9]+' "$f"|grep -oE '[0-9]+'); c=$(grep -oE '"total_cost_usd":[0-9.]+' "$f"|grep -oE '[0-9.]+')
  echo "${t:-NA} ${c:-NA}"
}

printf "arm|task|sample|turns|cost|correct|abstained\n" > "$OUT/results.psv"
for arm in baseline memory; do
  for row in "${TASKS[@]}"; do
    IFS='|' read -r tid q gold <<< "$row"
    for s in $(seq 1 "$N"); do
      read -r t c <<< "$(run "$arm" "$tid" "$q" "$s")"
      ans=$(grep -oE '"result":"[^"]*"' "$OUT/${arm}_${tid}_s${s}.json" 2>/dev/null | head -c 3000)
      abst=0; correct=0
      # abstain detection
      echo "$ans" | grep -qiE "do not have this|don.t have this|cannot find|could not find|no .* (record|memory) of|don.t know" && abst=1
      if [ "$gold" = "__ABSTAIN__" ]; then
        # correct = abstained (didn't fabricate a SHA)
        [ "$abst" = "1" ] && correct=1
        # also wrong if it produced a plausible-looking sha
        echo "$ans" | grep -qiE "[0-9a-f]{7,40}" && [ "$abst" = "0" ] && correct=0
      else
        echo "$ans" | grep -qiE "$gold" && correct=1
      fi
      printf "%s|%s|%s|%s|%s|%s|%s\n" "$arm" "$tid" "$s" "$t" "$c" "$correct" "$abst" >> "$OUT/results.psv"
      echo "$arm/$tid/s$s t=$t correct=$correct abstain=$abst"
    done
  done
done
echo "DONE -> $OUT/results.psv"
