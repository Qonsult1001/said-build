#!/usr/bin/env bash
# Volume test: load N synthetic PERSONAL-NOTE memories into the real shipped `said.exe`,
# fire a paraphrase query per note (each with ONE unique gold id), and score @1/@5/@10
# plus query latency. Mirrors the walkthrough shape (facts a person stores; asked in their
# own words). No documented gate for this shape -> reported as a BASELINE.
#
# IMPORTANT: each note carries a UNIQUE entity (a distinct person + topic), so its query
# has exactly ONE correct answer. Earlier versions used 8 repeated templates, which made
# 25 notes interchangeable answers to the same query and falsely scored sibling hits as
# misses. Here every note is its own answer.
#
# Usage: SAID=/path/to/said.exe bash scale-personal-notes.sh [N]
set -u
SAID="${SAID:-g:/development/said-build/said.exe}"
N="${1:-200}"
WORK="$(mktemp -d)"; BRAIN="$WORK/scale.said"
strip(){ grep -v "Loaded embedded model"; }
trap 'rm -rf "$WORK"' EXIT

# Each note is about a UNIQUE entity (a coined name + the index), so its query pins
# exactly one correct answer. Varied sentence shapes keep it realistic; the query
# paraphrases the note (different words, same meaning) to test recall by MEANING.
pet=(cat dog parrot rabbit hamster turtle goldfish ferret canary gecko)
hobby=(pottery climbing birdwatching cycling sketching kayaking gardening woodworking baking astronomy)
drink=(espresso matcha chai kombucha cider lemonade horchata cortado lassi mead)

gen(){ # $1=index -> "id|note|query"  (one unique answer per query)
  local i="$1" style=$(( i % 6 ))
  case "$style" in
    0) echo "n$i|Aunt Quzix$i keeps a ${pet[$((i%10))]} that loves sleeping in the garden.|what animal does Aunt Quzix$i own";;
    1) echo "n$i|My colleague Vorlex$i is really into ${hobby[$((i%10))]} every weekend.|what does Vorlex$i do for fun";;
    2) echo "n$i|The Brixham$i locker downtown opens with combination $((1000+i)).|what unlocks the Brixham$i locker";;
    3) echo "n$i|Zephyr$i offered to water the plants while we travel in July.|who is tending the plants for Zephyr$i";;
    4) echo "n$i|Neighbour Tindrel$i always orders a ${drink[$((i%10))]} at the cafe.|what beverage does Tindrel$i prefer";;
    5) echo "n$i|Cousin Marlow$i was born in the town of Pellingate$i by the coast.|where is Cousin Marlow$i originally from";;
  esac
}

echo "=== Volume test: $N personal-note memories into REAL said.exe ($($SAID --version 2>/dev/null|strip)) ==="
echo "    (each note = a unique fact; each query has exactly one correct answer)"
$SAID create "$BRAIN" >/dev/null 2>&1

declare -a QID QTEXT
t0=$(date +%s.%N)
for ((i=0; i<N; i++)); do
  IFS='|' read -r id note query <<< "$(gen "$i")"
  $SAID --path "$BRAIN" add "$note" --id "$id" >/dev/null 2>&1
  QID[$i]="$id"; QTEXT[$i]="$query"
done
t1=$(date +%s.%N)
rate=$(awk "BEGIN{printf \"%.1f\", $N/($t1-$t0)}")
size=$(stat -c%s "$BRAIN" 2>/dev/null || wc -c < "$BRAIN")
mem_count=$($SAID --path "$BRAIN" stats 2>/dev/null | strip | grep -iE "Memories:" | grep -oE "[0-9]+" | head -1)
echo "  ingested: $mem_count memories in $(awk "BEGIN{printf \"%.0f\", $t1-$t0}")s  (${rate}/s)  file=$(awk "BEGIN{printf \"%.2f\", $size/1048576}")MB"

hit1=0; hit5=0; hit10=0; total=0; returned_sum=0; empty=0
declare -a LAT
for ((i=0; i<N; i++)); do
  q="${QTEXT[$i]}"; gold="${QID[$i]}"
  qs=$(date +%s.%N)
  out=$($SAID --path "$BRAIN" ask "$q" 2>/dev/null | strip)
  qe=$(date +%s.%N)
  LAT[$i]=$(awk "BEGIN{printf \"%.2f\", ($qe-$qs)*1000}")
  ids=$(echo "$out" | grep -oE "\] n[0-9]+" | tr -d '] ')
  rank=$(echo "$ids" | grep -nxF "$gold" | head -1 | cut -d: -f1)
  ret=$(echo "$ids" | grep -cE "^n[0-9]+$")
  returned_sum=$(( returned_sum + ret )); [ "$ret" -eq 0 ] && empty=$(( empty + 1 )); total=$(( total + 1 ))
  if [ -n "$rank" ]; then
    [ "$rank" -le 1 ] && hit1=$(( hit1 + 1 ))
    [ "$rank" -le 5 ] && hit5=$(( hit5 + 1 ))
    [ "$rank" -le 10 ] && hit10=$(( hit10 + 1 ))
  fi
done

sorted=$(printf "%s\n" "${LAT[@]}" | sort -n)
p50=$(echo "$sorted" | awk '{a[NR]=$1} END{print a[int(NR*0.50)+0]}')
p99=$(echo "$sorted" | awk '{a[NR]=$1} END{print a[int(NR*0.99)+0]}')
avg_ret=$(awk "BEGIN{printf \"%.2f\", $returned_sum/$total}")

echo
echo "=== RESULTS (N=$N memories, $total queries, REAL said.exe ask) ==="
printf "  recall@1  = %.4f  (%d/%d)\n" "$(awk "BEGIN{print $hit1/$total}")" "$hit1" "$total"
printf "  recall@5  = %.4f  (%d/%d)\n" "$(awk "BEGIN{print $hit5/$total}")" "$hit5" "$total"
printf "  recall@10 = %.4f  (%d/%d)   <- gate depth\n" "$(awk "BEGIN{print $hit10/$total}")" "$hit10" "$total"
echo "  --- client-facing result shape ---"
echo "  avg results returned/query: $avg_ret"
echo "  queries returning nothing:  $empty/$total"
echo "  --- latency (ms, incl. process start) ---"
echo "  p50 = ${p50}ms   p99 = ${p99}ms"
