#!/usr/bin/env bash
# RESEARCH-CORRECT benchmark (replaces single-run pass@1).
# Metrics (cited): pass@k [2107.03374], turns-to-first-success / Cost-of-Pass [2504.13359],
# abstention as a CORRECT outcome [2207.05221, 2006.09462, 2502.09054]. Reflexion-style multi-sample.
#   - n samples/task/arm (default 5) -> pass@1 = c/n, pass@k = unbiased estimator.
#   - turns-to-first-success on tasks BOTH arms solve (the "how many turns to converge" the user wants).
#   - tasks partitioned: memory-only-solves / co-solved / neither (abstention candidates).
#   - abstention: the prompt invites a clean give-up; a correct give-up on an unanswerable task scores
#     as ABSTAIN-CORRECT (not failure); grind-then-wrong is the WORST outcome.
#
# RUN-FAILURE HANDLING (the NA-file fix): a claude call that times out / errors produces NO valid output
# file. That is a FAILED RUN, not a wrong ANSWER. We retry it up to RUN_RETRIES times; if it still
# produces no parseable result we mark the sample INVALID (correct=NA abstain=NA) so it is EXCLUDED from
# pass@k denominators and turn stats — never scored as correct=0/abstain=0 (which would corrupt the
# numbers, mis-counting an infra failure as the agent answering wrong).
set -u
SAID="g:/development/said-build/said-coding.exe"
ROOT="G:/cargo-tmp/bench_research"; rm -rf "$ROOT"; mkdir -p "$ROOT"
OUT="/tmp/bench_research_out"; rm -rf "$OUT"; mkdir -p "$OUT"
N="${SAMPLES:-5}"
RUN_RETRIES="${RUN_RETRIES:-2}"   # extra attempts on a failed (NA) run before marking INVALID
TIMEOUT_S="${TIMEOUT_S:-180}"
# FIX #12: enforce n>=5 for a REAL run (single runs are noise — docs/23). SMOKE=1 escapes for validation.
# Note: there is NO RNG "seed" loop — .said retrieval is DETERMINISTIC (10-benchmarks: diff exact scores;
# realworld-probes treats divergence as a bug) and the LLM is external+unseeded (BYO-LLM). The n>=5
# unseeded samples ARE the variance draws; the aggregator reports the across-sample distribution.
if [ "${SMOKE:-0}" != "1" ] && [ "$N" -lt 5 ]; then
  echo "REFUSED: a real run needs SAMPLES>=5 (got $N). Set SMOKE=1 to allow a small validation run." >&2
  exit 2
fi

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

# THE PROVEN A1-A5 SET (docs/20 v3-v7) — questions whose answer lives in NO source file as prose:
# a past DECISION / a FIX + its WHY / a non-obvious INVARIANT. This is the ONLY design where memory can
# win: init is the floor in BOTH arms, so a question answerable from indexed source measures code-reading,
# not memory (the RETRACTED v1 mistake, docs/20 line 140). Each A* maps to a docs/23 research axis:
#   A1 decision    -> co-solved convergence (both answer; memory cheaper by recalling the decision)
#   A2 fix + WHY   -> convergence (baseline grinds to reconstruct the WHY; memory recalls it)
#   A3 gotcha      -> ACCURACY / memory-only-solves (baseline CANNOT answer; only the seeded learning has it)
#   A4 abstain     -> ABSTENTION (answer is in no file AND no seeded fix -> clean give-up is CORRECT)
#   A5 fix + WHY   -> convergence (the previously fix-recall-starved paraphrase; -64% in v5 when recalled)
# gold tokens are LENIENT substrings of the verified learning; SAID_LLM_GRADE=1 adds an LLM oracle pass
# (docs/20/23: the strict regex undercounts — both arms answer more correctly than a regex scores).
TASKS=(
'A1|Why does .said ask() deliberately NOT walk the code call-graph, and what does it return instead so the type-precise work is done elsewhere?|LSP|language server|call-graph|stored fact'
'A2|We hit a bug where sym() returned 0 after a said init reopen. What was the root cause and WHY did it happen, and which section had to be persisted to fix it?|TRGM'
'A3|There was a gotcha that silently killed semantic recall on built binaries. What exactly was the condition (the encoder/build flag), and what was the fix?|opt-in|default|zero|fingerprint|embed-model'
'A4|What is the exact git commit SHA that first introduced the vault header feature in this repo?|__ABSTAIN__'
'A5|Why were descriptive queries failing to retrieve build_concept_links at all, and what was the fix?|doc-comment|comment|sibling|byte range'
)
# A4 gold = __ABSTAIN__ : the answer is in NO file and NO seeded fix -> correct iff the agent GIVES UP
# cleanly (no fabricated SHA). This is the abstention axis (docs/23 #3): grind-then-fabricate is the WORST.

# Per-task prompt suffix (docs/23: abstention is a PER-TASK property, not a global suppressor).
#  - Answerable tasks (Q1-Q4): NEUTRAL suffix — use whatever memory/code is available and answer if found.
#    A blanket give-up hint here corrupts BOTH axes: the memory arm can't show its win (it abstains on
#    answerable Q's even though the fix was injected — proven in the SAMPLES=1 smoke), and you can't tell
#    hint-induced give-up from genuine. So no give-up imperative on answerable tasks.
#  - The unanswerable probe (QABSTAIN): the give-up hint, which is exactly what we're measuring (the agent
#    SHOULD abstain when the answer is in no memory and no file).
NEUTRAL_HINT=" Use any project memory and the code to answer. If you find the answer, state it concisely."
GIVEUP_HINT=" If you cannot find a confident answer from project memory or a quick look, say 'I do not have this' and stop rather than guessing or fabricating."
# A4 is the abstention probe (answer in no file AND no seeded fix) -> give-up hint; A1-A3,A5 neutral.
hint_for () { case "$1" in A4|QABSTAIN) printf '%s' "$GIVEUP_HINT";; *) printf '%s' "$NEUTRAL_HINT";; esac; }

# llm_grade QUESTION GOLD ANSWER -> prints 1 (correct) or 0. The BLIND external oracle (FIX #10):
# a SEPARATE claude --print call that judges whether ANSWER conveys the gold fact. It is given NO arm
# label (blind, anti-bias LLM-as-judge precaution) and runs OUTSIDE .said (BYO-LLM contract). This is the
# SAME external-oracle pattern the project already uses for LoCoMo/competitor F1 (10-benchmarks/README.md).
# Falls back to 0 on any grader failure (never invents a pass). Opt-in via SAID_LLM_GRADE=1.
llm_grade () {
  local q="$1" gold="$2" ans="$3"
  local goldlist; goldlist=$(printf '%s' "$gold" | tr '|' ',')
  local gp="You are a strict but FAIR grader. Question: \"$q\". A correct answer must convey ANY of these key facts: [$goldlist]. Candidate answer: \"$ans\". Does the candidate convey at least one key fact (paraphrase is fine)? Reply with ONLY the single word YES or NO."
  local gf; gf=$(mktemp 2>/dev/null || echo "$OUT/_grade.json")
  timeout 60 claude --print --permission-mode bypassPermissions --output-format json "$gp" >"$gf" 2>/dev/null
  local verdict; verdict=$(grep -oE '"result":"[^"]*"' "$gf" 2>/dev/null | head -c 200)
  rm -f "$gf" 2>/dev/null
  echo "$verdict" | grep -qiE '\bYES\b' && { echo 1; return; }
  echo 0
}

# is_valid_run FILE -> 0 (valid) if the file holds a parseable claude --print json result, else 1.
# A run is valid iff it is non-empty, parses the success envelope (has a "result" field) AND has num_turns.
# A timeout/error yields an empty file or an error envelope with no "result"/num_turns -> INVALID.
is_valid_run () {
  local f="$1"
  [ -s "$f" ] || return 1                                   # missing/empty
  grep -q '"is_error":true' "$f" 2>/dev/null && return 1    # explicit error envelope
  grep -q '"num_turns":[0-9]' "$f" 2>/dev/null || return 1  # no turn count = no real run
  grep -q '"result":"' "$f" 2>/dev/null || return 1         # no result text = no answer produced
  return 0
}

# run ARM TID Q S SUFFIX -> prints "turns cost VALID|INVALID". Retries a failed run up to RUN_RETRIES times.
# SUFFIX is the per-task prompt suffix (the give-up hint ONLY for the unanswerable probe; neutral otherwise).
run () {
  local arm="$1" idx="$2" q="$3" s="$4" suffix="$5"
  cp .claude/settings.hookarm.json .claude/settings.local.json
  if [ "$arm" = "baseline" ]; then cp "$ROOT/baseline.said" "$ROOT/code.said"; else cp "$ROOT/memory.said" "$ROOT/code.said"; fi
  local f="$OUT/${arm}_${idx}_s${s}.json"
  local attempt=0 ok=1
  while [ "$attempt" -le "$RUN_RETRIES" ]; do
    timeout "$TIMEOUT_S" claude --print --permission-mode bypassPermissions --output-format json "$q$suffix" >"$f" 2>/dev/null
    if is_valid_run "$f"; then ok=0; break; fi
    attempt=$((attempt+1))
    [ "$attempt" -le "$RUN_RETRIES" ] && echo "  (retry $attempt/$RUN_RETRIES: $arm/$idx/s$s produced no valid result)" >&2
  done
  if [ "$ok" != "0" ]; then echo "NA NA INVALID"; return; fi
  local t c; t=$(grep -oE '"num_turns":[0-9]+' "$f"|grep -oE '[0-9]+'|head -1); c=$(grep -oE '"total_cost_usd":[0-9.]+' "$f"|grep -oE '[0-9.]+'|head -1)
  echo "${t:-NA} ${c:-NA} VALID"
}

# abstain_gold column (FIX #11): the aggregator scores abstention GENERICALLY over every row whose task
# had gold == __ABSTAIN__ (not a hardcoded task id). This supports N abstention probes with zero
# aggregator changes, and is what risk-coverage/abstention-F1 over a SET requires (docs/23 axis 3).
printf "arm|task|sample|turns|cost|correct|abstained|valid|abstain_gold\n" > "$OUT/results.psv"
INVALID_TOTAL=0
for arm in baseline memory; do
  for row in "${TASKS[@]}"; do
    IFS='|' read -r tid q gold <<< "$row"
    suffix="$(hint_for "$tid")"
    ag=0; [ "$gold" = "__ABSTAIN__" ] && ag=1
    for s in $(seq 1 "$N"); do
      read -r t c valid <<< "$(run "$arm" "$tid" "$q" "$s" "$suffix")"
      if [ "$valid" != "VALID" ]; then
        # RUN FAILURE: exclude from scoring. correct/abstain = NA so pass@k denominators skip it.
        INVALID_TOTAL=$((INVALID_TOTAL+1))
        printf "%s|%s|%s|%s|%s|%s|%s|%s|%s\n" "$arm" "$tid" "$s" "NA" "NA" "NA" "NA" "INVALID" "$ag" >> "$OUT/results.psv"
        echo "$arm/$tid/s$s INVALID (run failed after retries — EXCLUDED, not scored as wrong)"
        continue
      fi
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
        # REGEX oracle (default). LENIENT substring match of the gold tokens.
        echo "$ans" | grep -qiE "$gold" && correct=1
        # LLM-graded oracle (FIX #10): opt-in SAID_LLM_GRADE=1. A BLIND external grader (no arm label)
        # that judges whether the answer conveys the gold fact — fixes the documented regex undercount
        # (docs/20 L4). Modeled on the EXISTING external-oracle pattern (competitor_bench / LoCoMo Opus
        # judge, 10-benchmarks/README.md:38) — runs OUTSIDE .said, honoring the BYO-LLM contract.
        if [ "${SAID_LLM_GRADE:-0}" = "1" ]; then
          correct=$(llm_grade "$q" "$gold" "$ans")
        fi
      fi
      printf "%s|%s|%s|%s|%s|%s|%s|%s|%s\n" "$arm" "$tid" "$s" "$t" "$c" "$correct" "$abst" "VALID" "$ag" >> "$OUT/results.psv"
      echo "$arm/$tid/s$s t=$t correct=$correct abstain=$abst"
    done
  done
done
echo "DONE -> $OUT/results.psv  (INVALID/excluded runs: $INVALID_TOTAL)"
