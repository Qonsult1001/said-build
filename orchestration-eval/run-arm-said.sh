#!/usr/bin/env bash
# Arm A: said-orchestrate driving a FAST model (Groq gpt-oss-120b by default).
# Runs all 12 tickets, gate = node test/<id>.test.js, records pass + attempts.
# Usage: MODEL=openai/gpt-oss-120b ./run-arm-said.sh
set -u
ROOT="/g/development/said-build/orchestration-eval"
REPO="$ROOT/repo"
NODE="/c/nvm4w/nodejs/node"
SAID="/g/development/said-build/target/release/said-orchestrate.exe"
BRAIN="/g/development/Advisory/Advisory.said"
OUT="$ROOT/results/arm-said.csv"
MAXR="${MAXR:-2}"
MODEL="${MODEL:-openai/gpt-oss-120b}"

export GROQ_API_KEY="$(grep PKGFW_GROQ_API_KEY /g/development/Advisory/.env | cut -d= -f2- | tr -d '\r')"
export GROQ_MODEL="$MODEL"
unset OPENAI_API_KEY SAID_LLM_BASE_URL SAID_LLM_MODEL SAID_LLM_PROVIDER_ORDER

echo "ticket,bucket,arm,model,pass,attempts,secs" > "$OUT"
# Read tickets via a small node script (cd into ROOT so the relative require works).
mapfile -t ROWS < <(cd "$ROOT" && "$NODE" -e 'for(const x of require("./tickets.json").tickets) console.log([x.id,x.bucket,x.file,x.task].join("\t"))')

regen() { ( cd "$ROOT" && "$NODE" gen-seeds.js >/dev/null 2>&1 ); }
gate()  { "$NODE" "$REPO/test/$1.test.js" >/dev/null 2>&1; }

for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r id bucket file task <<< "$row"
  regen
  s=$(date +%s)
  out=$("$SAID" --brain "$BRAIN" --repo "$REPO" --task "$task" --files "$file" \
        --build "$NODE test/$id.test.js" --max-attempts $((MAXR+1)) 2>&1)
  e=$(date +%s)
  att=$(echo "$out" | grep -oE "attempts=[0-9]+" | grep -oE "[0-9]+" | tail -1); [ -z "$att" ] && att=0
  if gate "$id"; then p=1; else p=0; fi
  echo "$id  $bucket  pass=$p  attempts=$att  $((e-s))s"
  echo "$id,$bucket,said,$MODEL,$p,$att,$((e-s))" >> "$OUT"
done
echo "=== Arm A done -> $OUT ==="; cat "$OUT"
