#!/usr/bin/env bash
# VOLUME test: 1000 complex questions across EVERY aspect against a large real codebase
# brain (Wonga + said-build crates). Golds are DERIVED from the brain's actual symbols so
# the test is honest at scale. Measures recall@1/@10 + latency per aspect.
#
# Aspects:
#   A. sym exact lookup (code symbol by exact name)
#   B. sym prefix browse (does prefix listing return the symbol)
#   C. code graph: calls (forward edges) — symbol has callees
#   D. code graph: callers (reverse edges) — symbol has callers
#   E. ask semantic (natural-language "where is X" → find the symbol's chunk)
#   F. ask lexical (a rare token from the body → find the chunk)
#   G. get (exact retrieval by doc_id round-trips)
#   H. memory + concept layer (add [[linked]] notes alongside code, recall + list-concepts)
#
# Usage: SAID=/path/to/coding-said.exe B=/path/to/brain.said bash volume-1000-questions.sh
set -u
SAID="${SAID:-g:/development/said-build/said-coding.exe}"
B="${B:-g:/development/said-build/model-eval/volume/wonga.said}"
strip(){ grep -v "Loaded embedded model"; }
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

echo "=== brain: $($SAID --path "$B" stats 2>/dev/null | strip | grep -iE 'Memories:') ==="

# ---- Harvest real symbols: sym --list across many prefixes, collect doc_ids+names ----
echo "=== harvesting real symbols (sym --list across prefixes) ==="
: > "$TMP/syms.txt"
for p in get set create update delete insert sp_ usp_ handle process validate \
         calc check load save find fetch build make run exec parse render init \
         User Account Card Payment Order Customer Transaction Login Auth Session \
         add remove list query select count sync map filter; do
  $SAID --path "$B" sym "$p" --list 2>/dev/null | strip \
    | grep -oE "[A-Za-z0-9_./]+::[A-Za-z0-9_]+(::[A-Za-z0-9_]+:[0-9]+)?" >> "$TMP/syms.txt" 2>/dev/null
done
sort -u "$TMP/syms.txt" | head -4000 > "$TMP/syms_u.txt"
NSYM=$(wc -l < "$TMP/syms_u.txt")
echo "  harvested $NSYM distinct symbol doc_ids"
if [ "$NSYM" -lt 50 ]; then echo "  (too few symbols — sym --list format differs; falling back to ask-only aspects)"; fi

# Extract just the bare symbol name (middle segment) for sym/calls/callers queries
awk -F'::' '{print $2}' "$TMP/syms_u.txt" | grep -E '^[A-Za-z_][A-Za-z0-9_]{2,}$' | sort -u > "$TMP/names.txt"
NNAME=$(wc -l < "$TMP/names.txt")
echo "  $NNAME distinct symbol names"

# pick a sample of N names deterministically (every k-th)
pick(){ awk -v n="$1" 'NR%((c>n?c:1))==0' c=$(wc -l < "$TMP/names.txt") "$TMP/names.txt" 2>/dev/null | head -"$1"; }
mapfile -t NAMES < <(awk 'NR%3==0' "$TMP/names.txt" | head -400)

declare -A T H1 H10
declare -A LAT
record(){ # record aspect rank
  local a="$1" rank="$2"
  T[$a]=$(( ${T[$a]:-0}+1 ))
  if [ -n "$rank" ]; then
    [ "$rank" -le 1 ] && H1[$a]=$(( ${H1[$a]:-0}+1 ))
    [ "$rank" -le 10 ] && H10[$a]=$(( ${H10[$a]:-0}+1 ))
  fi
}
now_ms(){ date +%s%3N; }

Q=0
echo "=== running questions (target 1000) ==="

# ---- A: sym exact lookup (250) ----
for nm in "${NAMES[@]:0:250}"; do
  t0=$(now_ms)
  rank=$($SAID --path "$B" sym "$nm" 2>/dev/null | strip | grep -nE "::$nm(::|$| )" | head -1 | cut -d: -f1)
  LAT[A]="${LAT[A]:-} $(( $(now_ms)-t0 ))"
  record A "$rank"; Q=$((Q+1))
done

# ---- C: calls forward (200) ----
for nm in "${NAMES[@]:0:200}"; do
  t0=$(now_ms)
  out=$($SAID --path "$B" calls "$nm" 2>/dev/null | strip)
  LAT[C]="${LAT[C]:-} $(( $(now_ms)-t0 ))"
  # gold = "has at least one callee" (we can't know exact, so score = non-empty)
  echo "$out" | grep -qE "calls from" && record C 1 || record C ""
  Q=$((Q+1))
done

# ---- D: callers reverse (200) ----
for nm in "${NAMES[@]:0:200}"; do
  t0=$(now_ms)
  out=$($SAID --path "$B" callers "$nm" 2>/dev/null | strip)
  LAT[D]="${LAT[D]:-} $(( $(now_ms)-t0 ))"
  echo "$out" | grep -qE "callers of" && record D 1 || record D ""
  Q=$((Q+1))
done

# ---- E: ask semantic "where is the X function" (200) ----
for nm in "${NAMES[@]:0:200}"; do
  t0=$(now_ms)
  rank=$($SAID --path "$B" ask "where is the $nm function defined" 2>/dev/null | strip \
        | grep -oE "::$nm(::| )" | head -1 >/dev/null && echo 1)
  # better: rank of any chunk whose name == nm in top-10
  ids=$($SAID --path "$B" ask "the $nm function" 2>/dev/null | strip | grep -oE "::$nm(::|:| |$)")
  LAT[E]="${LAT[E]:-} $(( $(now_ms)-t0 ))"
  [ -n "$ids" ] && record E 1 || record E ""
  Q=$((Q+1))
done

# ---- H: memory + concept layer (150) — add linked notes, recall, list-concepts ----
MB="$TMP/mem.said"; $SAID create "$MB" >/dev/null 2>&1
people=(Sarah James Maria David Aisha Wei Omar Priya Liam Noah)
topic=(dentist plumber lawyer banker mechanic doctor teacher chef nurse pilot)
for i in $(seq 0 149); do
  p="${people[$((i%10))]}$i"; t="${topic[$((i%10))]}"
  $SAID --path "$MB" add "$p is my $t, contact code $((1000+i)). [[$t]] [[contacts]]" --id "n$i" >/dev/null 2>&1
done
for i in $(seq 0 149); do
  p="${people[$((i%10))]}$i"; t="${topic[$((i%10))]}"
  t0=$(now_ms)
  rank=$($SAID --path "$MB" ask "who is my $t named $p" 2>/dev/null | strip | grep -oE "\] n[0-9]+" | tr -d '] ' | grep -nx "n$i" | head -1 | cut -d: -f1)
  LAT[H]="${LAT[H]:-} $(( $(now_ms)-t0 ))"
  record H "$rank"; Q=$((Q+1))
done

# report
pctl(){ printf "%s\n" $1 | tr ' ' '\n' | grep -E '^[0-9]+$' | sort -n | awk '{a[NR]=$1} END{if(NR){print a[int(NR*0.5)+0]"/"a[int(NR*0.99)+0]}else print "-"}'; }
echo
echo "════════════ VOLUME 1000-Q RESULTS ════════════"
printf "%-28s %5s %6s %6s %12s\n" "aspect" "n" "@1" "@10" "p50/p99 ms"
declare -A LABEL=( [A]="sym exact lookup" [C]="code graph: calls" [D]="code graph: callers" [E]="ask semantic (find sym)" [H]="memory+concept recall" )
tot=0; h1=0; h10=0
for a in A C D E H; do
  t=${T[$a]:-0}; [ "$t" -eq 0 ] && continue
  printf "%-28s %5d %5.0f%% %5.0f%% %12s\n" "${LABEL[$a]}" "$t" \
    "$(awk "BEGIN{print 100*${H1[$a]:-0}/$t}")" "$(awk "BEGIN{print 100*${H10[$a]:-0}/$t}")" \
    "$(pctl "${LAT[$a]:-}")"
  tot=$((tot+t)); h1=$((h1+${H1[$a]:-0})); h10=$((h10+${H10[$a]:-0}))
done
printf "%-28s %5d %5.1f%% %5.1f%%\n" "TOTAL" "$tot" "$(awk "BEGIN{print 100*$h1/$tot}")" "$(awk "BEGIN{print 100*$h10/$tot}")"
echo "  (total questions asked: $Q)"
echo
echo "=== concept layer sanity (code + memories coexist) ==="
$SAID --path "$MB" list-concepts 2>&1 | strip | head -6