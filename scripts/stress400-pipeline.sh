#!/usr/bin/env bash
# 400-scale, ALL-nuance recall through the REAL said.exe pipeline (grep + semantic +
# rerank + gates) — captures the keyword engine that raw-cosine probes miss. Scores
# @1/@3/@10 per category. Run once per embedded model (rebuild binary between runs).
#
# Usage: SAID=/path/to/said.exe bash stress400-pipeline.sh
set -u
SAID="${SAID:-g:/development/said-build/said.exe}"
WORK="$(mktemp -d)"; B="$WORK/s.said"
strip(){ grep -v "Loaded embedded model"; }
trap 'rm -rf "$WORK"' EXIT

pet=(cat dog parrot rabbit hamster turtle goldfish ferret canary gecko)
hob=(pottery climbing birdwatching cycling sketching kayaking gardening woodworking baking astronomy)
city=(Lisbon Osaka Nairobi Bogota Oslo Cairo Pune Lyon Perth Quito)

note_for(){ local i="$1"; case $((i%8)) in
  0) echo "The wifi password for the ${city[$((i%10))]} office is hunter$i.";;
  1) echo "Colleague Vortek$i is allergic to peanuts and shellfish.";;
  2) echo "Plumb-tech Brennan$i unclogs drains and fixes burst pipes.";;
  3) echo "Friend Marlow$i spends every weekend ${hob[$((i%10))]}.";;
  4) echo "Neighbour Sindri$i owns a ${pet[$((i%10))]} named Pepper.";;
  5) echo "Locker $i at the gym opens with code $((1000+i)).";;
  6) echo "Dr. Halloran$i is the cardiologist at the downtown clinic.";;
  *) echo "The Pellican$i project deadline is the last Friday of March.";;
esac; }
query_for(){ local i="$1"; case $((i%8)) in
  0) echo "lexical|what is the wifi password for the ${city[$((i%10))]} office";;
  1) echo "anchored-para|what foods can't Vortek$i eat";;
  2) echo "conceptual|who do I call about a water leak number $i";;
  3) echo "semantic|what does Marlow$i do for fun";;
  4) echo "entity-attr|what kind of animal does Sindri$i keep";;
  5) echo "numeric|what is the code for locker $i";;
  6) echo "multihop|who treats heart problems case $i";;
  *) echo "time-para|when is the Pellican$i project due";;
esac; }

echo "=== loading 400 notes into REAL said.exe ($($SAID --version 2>/dev/null|strip)) ==="
$SAID create "$B" >/dev/null 2>&1
for ((i=0;i<400;i++)); do $SAID --path "$B" add "$(note_for $i)" --id "n$i" >/dev/null 2>&1; done

declare -A T H1 H3 H10
for ((i=0;i<400;i++)); do
  IFS='|' read -r cat q <<< "$(query_for $i)"
  ids=$($SAID --path "$B" ask "$q" 2>/dev/null | strip | grep -oE "\] n[0-9]+" | tr -d '] ')
  rank=$(echo "$ids" | grep -nx "n$i" | head -1 | cut -d: -f1)
  T[$cat]=$(( ${T[$cat]:-0} + 1 ))
  if [ -n "$rank" ]; then
    [ "$rank" -le 1 ] && H1[$cat]=$(( ${H1[$cat]:-0} + 1 ))
    [ "$rank" -le 3 ] && H3[$cat]=$(( ${H3[$cat]:-0} + 1 ))
    [ "$rank" -le 10 ] && H10[$cat]=$(( ${H10[$cat]:-0} + 1 ))
  fi
done

echo
printf "%-14s %4s %5s %5s %5s\n" category n @1 @3 @10
a1=0;a3=0;a10=0;n=0
for cat in lexical anchored-para conceptual semantic entity-attr numeric multihop time-para; do
  t=${T[$cat]:-0}; [ "$t" -eq 0 ] && continue
  printf "%-14s %4d %4.0f%% %4.0f%% %4.0f%%\n" "$cat" "$t" \
    "$(awk "BEGIN{print 100*${H1[$cat]:-0}/$t}")" \
    "$(awk "BEGIN{print 100*${H3[$cat]:-0}/$t}")" \
    "$(awk "BEGIN{print 100*${H10[$cat]:-0}/$t}")"
  a1=$((a1+${H1[$cat]:-0})); a3=$((a3+${H3[$cat]:-0})); a10=$((a10+${H10[$cat]:-0})); n=$((n+t))
done
printf "%-14s %4d %4.1f%% %4.1f%% %4.1f%%  <== OVERALL\n" ALL "$n" \
  "$(awk "BEGIN{print 100*$a1/$n}")" "$(awk "BEGIN{print 100*$a3/$n}")" "$(awk "BEGIN{print 100*$a10/$n}")"
