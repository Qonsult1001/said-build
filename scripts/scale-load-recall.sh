#!/usr/bin/env bash
# verify-scale — load the REAL said CLI (brain bundle) at volume the way a user adds
# data (walkthrough `add ... --id`), then recall via `ask` (the documented primary
# verb) and score recall @1/@5/@10/@50 against the known gold id. Checks dedup,
# reports throughput/latency, and ALWAYS removes its temp brain.
#
# DATA: every note is a GENUINELY DISTINCT real-world fact (unique subject + unique
# attribute), NOT a recycled template with a number. No two notes share meaning and
# none are identical text — so recall@1 is actually measurable (no arbitrary ties
# between duplicate content). The query is a natural paraphrase using different words.
#
# Usage: scripts/scale-load-recall.sh /path/to/said.exe N   (N up to pool size)
set -u
BIN="${1:?need binary path}"; N="${2:-200}"
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
SBX="$(mktemp -d -t scale-XXXX)"
trap 'rm -rf "$SBX"' EXIT INT TERM
cd "$SBX"
BRAIN="brain.said"
"$BIN" create "$BRAIN" >/dev/null 2>&1

# Distinct subjects (people) × distinct life-facts. Each (person, fact) pair is a
# unique sentence with a unique paraphrase query. person index and fact index are
# both derived from i so every i yields different meaning; we use enough people that
# N notes never reuse the same (person,fact) pair.
people=(Mara Tomas Lena Owen Priya Diego Yuki Hassan Nora Felix Ingrid Carlos Aisha Bjorn Mei Raj Sofia Kwame Elsa Viktor \
        Amara Tariq Linnea Pablo Sana Erik Noor Gustavo Aria Mateo Freya Omar Lucia Anders Zara Hugo Ines Marco Saoirse Niko)
# 25 fact templates, each with a DISTINCT semantic domain + a distinct paraphrase.
facts=(
"loves hiking the volcanic trails of Iceland.|||which outdoor activity does %s enjoy on Icelandic volcanoes"
"collects vintage typewriters from the 1930s.|||what old machines does %s gather"
"is allergic to shellfish but loves spicy curry.|||what food must %s avoid"
"plays the cello in a weekend jazz quartet.|||which instrument does %s perform on at weekends"
"runs a small bakery famous for cardamom buns.|||what business does %s operate"
"is studying marine biology with a focus on coral.|||what subject does %s research underwater"
"restored a 1968 motorcycle over three winters.|||what vehicle did %s rebuild"
"speaks four languages including Swahili.|||how many tongues can %s speak"
"keeps bees and harvests wildflower honey.|||what insects does %s tend"
"won a regional chess championship at sixteen.|||what board-game title did %s earn young"
"volunteers at the city animal shelter on Sundays.|||where does %s help out on the weekend"
"is writing a novel set during the gold rush.|||what historical book is %s working on"
"trains for marathons in the early morning cold.|||what endurance sport does %s practise at dawn"
"grows heirloom tomatoes on a rooftop garden.|||what does %s cultivate above the building"
"repairs antique clocks as a quiet hobby.|||what old timepieces does %s fix"
"sails a small wooden boat on the northern fjords.|||what watercraft does %s navigate up north"
"teaches pottery to children after school.|||what craft does %s instruct kids in"
"photographs the northern lights every winter.|||what sky phenomenon does %s capture on camera"
"brews experimental sour beers in the garage.|||what tart drinks does %s ferment at home"
"is afraid of heights but loves deep caves.|||what underground places does %s explore"
"composes film scores using only analog synths.|||what music does %s create with vintage electronics"
"breeds rare orchids in a humid greenhouse.|||what exotic flowers does %s raise"
"once cycled the length of the Andes solo.|||what mountain journey did %s complete alone"
"curates a museum of forgotten board games.|||what collection does %s manage publicly"
"forages mushrooms in the autumn pine forests.|||what does %s gather in the fall woods"
)
NP=${#people[@]}; NF=${#facts[@]}; CAP=$(( NP * NF ))
[ "$N" -gt "$CAP" ] && { echo "N=$N exceeds distinct-pool capacity $CAP; cap it."; N=$CAP; }

echo "=== INGEST: $N DISTINCT notes via the walkthrough add flow ($BRAIN) ==="
t0=$(date +%s)
for i in $(seq 0 $(( N - 1 ))); do
  pi=$(( i % NP )); fi=$(( (i / NP) % NF ))
  person="${people[$pi]}"; raw="${facts[$fi]}"
  fact="$person ${raw%%|||*}"
  "$BIN" --path "$BRAIN" add "$fact" --id "note$i" >/dev/null 2>&1
done
t1=$(date +%s); secs=$(( t1 - t0 )); [ "$secs" -eq 0 ] && secs=1
active="$("$BIN" --path "$BRAIN" stats 2>/dev/null | grep -oE 'Active frames:[[:space:]]+[0-9]+' | grep -oE '[0-9]+')"
echo "ingested $N in ${secs}s (~$(( N / secs ))/s); active frames = $active; file = $(wc -c < "$BRAIN") bytes"

echo "=== DEDUP: re-add 5 identical notes (same id+content) — active must NOT grow ==="
for i in 0 1 2 3 4; do
  pi=$(( i % NP )); fi=$(( (i / NP) % NF )); raw="${facts[$fi]}"
  "$BIN" --path "$BRAIN" add "${people[$pi]} ${raw%%|||*}" --id "note$i" >/dev/null 2>&1
done
active2="$("$BIN" --path "$BRAIN" stats 2>/dev/null | grep -oE 'Active frames:[[:space:]]+[0-9]+' | grep -oE '[0-9]+')"
[ "$active2" = "$active" ] && echo "DEDUP OK: active stayed at $active2" || echo "DEDUP FAIL: $active -> $active2"

echo "=== RECALL via 'ask' (primary verb), score @1/@5/@10/@50 ==="
hit1=0; hit5=0; hit10=0; hit50=0; total=0
lat="$SBX/lat.txt"; : > "$lat"
stride=1; [ "$N" -gt 200 ] && stride=$(( N / 200 ))
for i in $(seq 0 "$stride" $(( N - 1 ))); do
  pi=$(( i % NP )); fi=$(( (i / NP) % NF )); person="${people[$pi]}"; raw="${facts[$fi]}"
  qtmpl="${raw##*|||}"; q="${qtmpl//%s/$person}"
  gold="note$i"
  qs=$(date +%s%N)
  out="$("$BIN" --path "$BRAIN" ask "$q" --deep --top 50 2>/dev/null)"
  qe=$(date +%s%N); echo $(( (qe-qs)/1000000 )) >> "$lat"
  ranks="$(printf '%s' "$out" | grep -oE '\]\[[a-z]+\] +note[0-9]+' | grep -oE 'note[0-9]+')"
  rank=0; pos=0
  while IFS= read -r d; do pos=$(( pos+1 )); [ "$d" = "$gold" ] && { rank=$pos; break; }; done <<< "$ranks"
  total=$(( total+1 ))
  [ "$rank" -eq 1 ] && hit1=$(( hit1+1 ))
  [ "$rank" -ge 1 ] && [ "$rank" -le 5 ]  && hit5=$(( hit5+1 ))
  [ "$rank" -ge 1 ] && [ "$rank" -le 10 ] && hit10=$(( hit10+1 ))
  [ "$rank" -ge 1 ] && [ "$rank" -le 50 ] && hit50=$(( hit50+1 ))
done
sort -n "$lat" > "$SBX/ls.txt"; nl=$(wc -l < "$SBX/ls.txt")
p50=$(sed -n "$(( (nl*50+99)/100 ))p" "$SBX/ls.txt"); p99=$(sed -n "$(( (nl*99+99)/100 ))p" "$SBX/ls.txt")

echo
echo "===== RESULT via ask (N=$N, distinct content, queries scored=$total) ====="
awk -v h1="$hit1" -v h5="$hit5" -v h10="$hit10" -v h50="$hit50" -v t="$total" 'BEGIN{
  printf "  recall@1  = %.4f (%d/%d)\n", h1/t, h1, t
  printf "  recall@5  = %.4f (%d/%d)\n", h5/t, h5, t
  printf "  recall@10 = %.4f (%d/%d)  <-- GATE\n", h10/t, h10, t
  printf "  recall@50 = %.4f (%d/%d)\n", h50/t, h50, t
}'
echo "  ask latency: p50=${p50}ms  p99=${p99}ms"
