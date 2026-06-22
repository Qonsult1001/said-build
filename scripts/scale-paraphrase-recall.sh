#!/usr/bin/env bash
# HONEST recall@1/5/10 at volume: notes are distinct facts, queries are PARAPHRASES that
# share an entity name with the note (so there's exactly one gold) but rephrase the rest —
# the realistic personal-memory case ("what's my dentist's name" style), NOT the
# identifier-only case (Quzix42) that the rare-token boost makes trivially perfect.
#
# Usage: SAID=/path/to/said.exe bash scale-paraphrase-recall.sh [N]
set -u
SAID="${SAID:-g:/development/said-build/said.exe}"
N="${1:-400}"
WORK="$(mktemp -d)"; BRAIN="$WORK/p.said"
strip(){ grep -v "Loaded embedded model"; }
trap 'rm -rf "$WORK"' EXIT

# Each note: a unique PERSON (coined surname+index) + a distinct fact. The query names the
# person and paraphrases the fact. Gold = that one note. The person name is the anchor a
# real user would remember; the rest is meaning-match.
gen(){ # i -> "id|note|query"
  local i="$1" who="Person$i"
  case $(( i % 8 )) in
    0) echo "n$i|$who is allergic to shellfish and carries an epi-pen everywhere.|what should I avoid serving $who at dinner";;
    1) echo "n$i|$who works the night shift at the hospital and sleeps during the day.|when is $who usually awake";;
    2) echo "n$i|$who is saving up to buy a small sailboat next summer.|what big purchase is $who planning";;
    3) echo "n$i|$who speaks three languages and grew up in Montreal.|where did $who spend their childhood";;
    4) echo "n$i|$who broke their ankle skiing and is in a cast for six weeks.|why is $who on crutches right now";;
    5) echo "n$i|$who runs a small bakery that is famous for its cinnamon rolls.|what business does $who own";;
    6) echo "n$i|$who is terrified of flying and always takes the train instead.|how does $who prefer to travel";;
    7) echo "n$i|$who volunteers at the animal shelter every Saturday morning.|how does $who spend weekends";;
  esac
}

echo "=== HONEST paraphrase recall: $N notes, $N queries, REAL said.exe ==="
$SAID create "$BRAIN" >/dev/null 2>&1
declare -a QID QTEXT
for ((i=0;i<N;i++)); do
  IFS='|' read -r id note q <<< "$(gen "$i")"
  $SAID --path "$BRAIN" add "$note" --id "$id" >/dev/null 2>&1
  QID[$i]="$id"; QTEXT[$i]="$q"
done
mem=$($SAID --path "$BRAIN" stats 2>/dev/null | strip | grep -iE "Memories:" | grep -oE "[0-9]+" | head -1)
echo "  loaded: $mem memories"

h1=0;h5=0;h10=0;tot=0;retsum=0
for ((i=0;i<N;i++)); do
  out=$($SAID --path "$BRAIN" ask "${QTEXT[$i]}" 2>/dev/null | strip)
  ids=$(echo "$out" | grep -oE "\] n[0-9]+" | tr -d '] ')
  rank=$(echo "$ids" | grep -nxF "${QID[$i]}" | head -1 | cut -d: -f1)
  ret=$(echo "$ids" | grep -cE "^n[0-9]+$"); retsum=$((retsum+ret)); tot=$((tot+1))
  if [ -n "$rank" ]; then
    [ "$rank" -le 1 ] && h1=$((h1+1)); [ "$rank" -le 5 ] && h5=$((h5+1)); [ "$rank" -le 10 ] && h10=$((h10+1))
  fi
done
echo
echo "=== HONEST RECALL (paraphrase, name-anchored) N=$N ==="
printf "  recall@1  = %.4f  (%d/%d)\n" "$(awk "BEGIN{print $h1/$tot}")" "$h1" "$tot"
printf "  recall@5  = %.4f  (%d/%d)\n" "$(awk "BEGIN{print $h5/$tot}")" "$h5" "$tot"
printf "  recall@10 = %.4f  (%d/%d)\n" "$(awk "BEGIN{print $h10/$tot}")" "$h10" "$tot"
printf "  avg results/query = %.2f\n" "$(awk "BEGIN{print $retsum/$tot}")"
