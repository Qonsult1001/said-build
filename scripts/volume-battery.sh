#!/usr/bin/env bash
# Lean volume battery over a large real-code brain. Harvests real symbols once, then runs
# a focused per-aspect sample. Writes a result line per aspect (flushed). Faster than the
# 1000-q version (fewer, representative queries; same aspects).
set -u
SAID="${SAID:-g:/development/said-build/said-coding.exe}"
B="${B:-g:/development/said-build/model-eval/volume/ab.said}"
N="${N:-100}"   # questions per code aspect
strip(){ grep -v "Loaded embedded model"; }
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
RESULT="${RESULT:-/dev/stdout}"

echo "brain: $($SAID --path "$B" stats 2>/dev/null | strip | grep -iE 'Memories:')" > "$RESULT"

# harvest real symbol names once (one big sym --list per prefix, dedup)
for p in get set create handle process User Account Card Payment Order add update; do
  $SAID --path "$B" sym "$p" --list 2>/dev/null | strip \
    | grep -oE "::[A-Za-z_][A-Za-z0-9_]+" | sed 's/^:://' >> "$TMP/n.txt"
done
sort -u "$TMP/n.txt" | grep -E '^[A-Za-z_][A-Za-z0-9_]{3,}$' > "$TMP/names.txt"
echo "harvested $(wc -l < "$TMP/names.txt") symbol names" >> "$RESULT"
mapfile -t NAMES < "$TMP/names.txt"
TAKE=$(( ${#NAMES[@]} < N ? ${#NAMES[@]} : N ))

asp(){ # asp "label" hits total ms_list
  local hits="$2" tot="$3"
  local p50 p99
  p50=$(printf "%s\n" $4 | sort -n | awk '{a[NR]=$1} END{print a[int(NR*0.5)+0]}')
  p99=$(printf "%s\n" $4 | sort -n | awk '{a[NR]=$1} END{print a[int(NR*0.99)+0]}')
  printf "%-26s %4d  %5.0f%%  p50=%sms p99=%sms\n" "$1" "$tot" "$(awk "BEGIN{print 100*$hits/$tot}")" "${p50:-?}" "${p99:-?}" >> "$RESULT"
}
ms(){ date +%s%3N; }

# A: sym exact lookup
h=0; lat=""; for nm in "${NAMES[@]:0:$TAKE}"; do
  t=$(ms); ok=$($SAID --path "$B" sym "$nm" 2>/dev/null | strip | grep -cE "::$nm(:|::| |$)")
  lat="$lat $(( $(ms)-t ))"; [ "${ok:-0}" -ge 1 ] && h=$((h+1)); done
asp "A. sym exact lookup" "$h" "$TAKE" "$lat"

# C: calls (non-empty = has graph edges)
h=0; lat=""; for nm in "${NAMES[@]:0:$TAKE}"; do
  t=$(ms); $SAID --path "$B" calls "$nm" 2>/dev/null | strip | grep -q "calls from" && h=$((h+1))
  lat="$lat $(( $(ms)-t ))"; done
asp "C. code graph: calls" "$h" "$TAKE" "$lat"

# D: callers
h=0; lat=""; for nm in "${NAMES[@]:0:$TAKE}"; do
  t=$(ms); $SAID --path "$B" callers "$nm" 2>/dev/null | strip | grep -q "callers of" && h=$((h+1))
  lat="$lat $(( $(ms)-t ))"; done
asp "D. code graph: callers" "$h" "$TAKE" "$lat"

# E: ask semantic — find the symbol's chunk by NL query, gold = any chunk named nm in top10
h=0; lat=""; for nm in "${NAMES[@]:0:$TAKE}"; do
  t=$(ms); ok=$($SAID --path "$B" ask "the $nm function" 2>/dev/null | strip | grep -cE "::$nm(:|::| |$)")
  lat="$lat $(( $(ms)-t ))"; [ "${ok:-0}" -ge 1 ] && h=$((h+1)); done
asp "E. ask semantic (find sym)" "$h" "$TAKE" "$lat"

# F: ask lexical — query the bare symbol token, gold = chunk named nm in top10
h=0; lat=""; for nm in "${NAMES[@]:0:$TAKE}"; do
  t=$(ms); ok=$($SAID --path "$B" ask "$nm" 2>/dev/null | strip | grep -cE "::$nm(:|::| |$)")
  lat="$lat $(( $(ms)-t ))"; [ "${ok:-0}" -ge 1 ] && h=$((h+1)); done
asp "F. ask lexical (token)" "$h" "$TAKE" "$lat"

# H: memory + concept layer alongside the code brain (separate mem brain)
MB="$TMP/mem.said"; $SAID create "$MB" >/dev/null 2>&1
ppl=(Sarah James Maria David Aisha Wei Omar Priya Liam Noah); top=(dentist plumber lawyer banker mechanic doctor teacher chef nurse pilot)
for i in $(seq 0 99); do $SAID --path "$MB" add "${ppl[$((i%10))]}$i is my ${top[$((i%10))]}, code $((1000+i)). [[${top[$((i%10))]}]]" --id "n$i" >/dev/null 2>&1; done
h=0; lat=""; for i in $(seq 0 99); do
  t=$(ms); rank=$($SAID --path "$MB" ask "who is my ${top[$((i%10))]} named ${ppl[$((i%10))]}$i" 2>/dev/null | strip | grep -oE "\] n[0-9]+" | tr -d '] ' | grep -nx "n$i" | head -1 | cut -d: -f1)
  lat="$lat $(( $(ms)-t ))"; [ -n "$rank" ] && [ "$rank" -le 10 ] && h=$((h+1)); done
asp "H. memory+concept @10" "$h" 100 "$lat"

echo "=== concept layer (code + memories coexist, no junk) ===" >> "$RESULT"
$SAID --path "$MB" list-concepts 2>/dev/null | strip | head -4 >> "$RESULT"
echo "DONE" >> "$RESULT"