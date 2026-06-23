#!/usr/bin/env bash
# 400 GENUINELY DISTINCT memories (pure bash, no python) across semantic / lexical /
# numeric / combination / wifi / date / profession types. Shows sample Q&A with
# question, returned id+score+count, and per-type recall.
set -u
SAID="${SAID:-g:/development/said-build/said.exe}"
WORK="$(mktemp -d)"; B="$WORK/real.said"
strip(){ grep -v "Loaded embedded model"; }
trap 'rm -rf "$WORK"' EXIT

people=(Sarah James Maria David Aisha Wei Omar Priya Liam Noah Emma Yuki Carlos Fatima Ravi Hana Tomas Ingrid Pavel Zara Mateo Lena Hugo Nadia Bjorn Sofia Kenji Amara Diego Freya Ibrahim Mei Lukas Tara Pablo Esme Arjun Nora Cyrus Lila)
cities=(Lisbon Osaka Nairobi Bogota Oslo Cairo Pune Lyon Perth Tbilisi Quito Riga Accra Davao Cusco Bergen Galway Hue Split Lviv)
foods=(ramen tacos biryani pierogi falafel pho gnocchi paella sushi goulash)
profs=(cardiologist:heart dermatologist:skin optometrist:eyes dentist:teeth physiotherapist:back)

ID=(); MEM=(); Q=(); TY=()
add(){ ID+=("m${#ID[@]}"); MEM+=("$1"); Q+=("$2"); TY+=("$3"); }

# SEMANTIC (paraphrase, person-anchored) — 40
for p in "${people[@]:0:20}"; do
  add "$p is terrified of spiders and refuses to go in the basement." "what is $p afraid of" semantic
  add "$p grew up on a dairy farm in the countryside." "where did $p spend childhood" semantic
done
# LEXICAL (unique code the query repeats) — 40
for n in $(seq 0 39); do
  code="VAULT-$((1000+n*7))"
  add "The $code access badge is kept in the top drawer." "where is the $code access badge" lexical
done
# NUMERIC — 40
for n in $(seq 0 39); do
  add "The conference room on floor $((n+1)) seats $((12+n)) people." "how many people does the floor $((n+1)) conference room seat" numeric
done
# COMBINATION (person + topic + detail) — 40
for idx in $(seq 0 39); do
  p="${people[$idx]}"; c="${cities[$((idx%20))]}"; f="${foods[$((idx%10))]}"
  add "$p's favourite restaurant is the $f place in $c, opened in $((1990+idx))." "what is $p's favourite restaurant" combination
done
# WIFI (distinct per city) — 20
for idx in "${!cities[@]}"; do
  c="${cities[$idx]}"
  add "The wifi password at the $c apartment is ${foods[$((idx%10))]}-$((100+idx))." "what is my wifi password at the $c apartment" wifi
done
# DATE — 20
for idx in $(seq 0 19); do
  p="${people[$idx]}"
  add "$p's wedding anniversary is on the $((idx+1))th of October." "when is $p's wedding anniversary" date
done
# PROFESSION (symptom->specialist) — 40
for idx in $(seq 0 39); do
  p="${people[$idx]}"; pr="${profs[$((idx%5))]}"; prof="${pr%%:*}"; sym="${pr##*:}"
  add "Dr. $p is the $prof I have been seeing since 2020." "who do I see about my $sym" profession
done
# MISC distinct trivia — pad to 400
extras=("the office printer jams on heavy paper above 120gsm|what paper weight jams the office printer"
        "the basement freezer keeps backup vaccines at -20C|where are the backup vaccines stored"
        "the rooftop solar array generates 14 kWh on a sunny day|how much power does the rooftop solar make"
        "the company retreat is booked for the second week of August|when is the company retreat"
        "the fire extinguisher is mounted beside the kitchen exit|where is the fire extinguisher")
ei=0
while [ "${#ID[@]}" -lt 400 ]; do
  e="${extras[$((ei%5))]}"; t="${e%%|*}"; q="${e##*|}"; b="${#ID[@]}"
  add "At branch number $b: $t." "$q at branch $b" misc; ei=$((ei+1))
done

echo "=== loading ${#ID[@]} distinct memories ==="
for k in "${!ID[@]}"; do $SAID --path "$B" add "${MEM[$k]}" --id "${ID[$k]}" >/dev/null 2>&1; done

declare -A T H1 H10
seen=""
echo
echo "=== SAMPLE Q&A per type (question / gold / rank / count / top result) ==="
for k in "${!ID[@]}"; do
  ty="${TY[$k]}"
  out=$($SAID --path "$B" ask "${Q[$k]}" 2>/dev/null | strip)
  ids=$(echo "$out" | grep -oE "\] m[0-9]+" | tr -d '] ')
  cnt=$(echo "$out" | grep -cE "^\s+[0-9]+\.")
  rank=$(echo "$ids" | grep -nx "${ID[$k]}" | head -1 | cut -d: -f1)
  T[$ty]=$(( ${T[$ty]:-0}+1 ))
  [ -n "$rank" ] && { [ "$rank" -le 1 ] && H1[$ty]=$(( ${H1[$ty]:-0}+1 )); [ "$rank" -le 10 ] && H10[$ty]=$(( ${H10[$ty]:-0}+1 )); }
  if [[ "$seen" != *"|$ty|"* ]]; then
    seen="$seen|$ty|"
    top=$(echo "$out" | grep -E "^\s+1\." | sed 's/^ *//')
    echo "[$ty]"
    echo "  Q: \"${Q[$k]}\""
    echo "  gold ${ID[$k]}: ${MEM[$k]}"
    echo "  rank=${rank:-MISS}  returned=$cnt  top: $top"
  fi
done

echo
echo "=== PER-TYPE RECALL ==="
printf "%-12s %4s %6s %6s\n" type n @1 @10
a1=0;a10=0;n=0
for ty in semantic lexical numeric combination wifi date profession misc; do
  t=${T[$ty]:-0}; [ "$t" -eq 0 ] && continue
  printf "%-12s %4d %5.0f%% %5.0f%%\n" "$ty" "$t" "$(awk "BEGIN{print 100*${H1[$ty]:-0}/$t}")" "$(awk "BEGIN{print 100*${H10[$ty]:-0}/$t}")"
  a1=$((a1+${H1[$ty]:-0})); a10=$((a10+${H10[$ty]:-0})); n=$((n+t))
done
printf "%-12s %4d %5.1f%% %5.1f%%  <== OVERALL\n" ALL "$n" "$(awk "BEGIN{print 100*$a1/$n}")" "$(awk "BEGIN{print 100*$a10/$n}")"

echo
echo "=== 'what is my wifi password' — show count returned (your example) ==="
for c in Lisbon Osaka Cairo Perth; do
  echo "Q: \"what is my wifi password at the $c apartment\""
  $SAID --path "$B" ask "what is my wifi password at the $c apartment" 2>/dev/null | strip | grep -E "results|^\s+1\." | head -2 | sed 's/^/   /'
done
