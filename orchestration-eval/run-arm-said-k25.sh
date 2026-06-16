#!/usr/bin/env bash
# Aligned Arm A: said-orchestrate + kimi-k2.5 (OpenRouter, pinned to fast provider)
# — same base model as Composer-2.5. 12 tickets, node gate, records pass+attempts.
set -u
ROOT="/g/development/said-build/orchestration-eval"
REPO="$ROOT/repo"
NODE="/c/nvm4w/nodejs/node"
SAID="/g/development/said-build/target/release/said-orchestrate.exe"
BRAIN="/g/development/Advisory/Advisory.said"
OUT="$ROOT/results/arm-said-k26.csv"
MAXR="${MAXR:-2}"

# Aligned arm: kimi-k2.6 INSTANT (no thinking) — the fast config (~1.3s/call on
# Parasail), temp 1.0. Thinking-mode k2.x is 35-80s/call; instant is the usable one.
export OPENAI_API_KEY="${OPENROUTER_API_KEY}"
export SAID_LLM_BASE_URL="https://openrouter.ai/api/v1"
export SAID_LLM_MODEL="moonshotai/kimi-k2.6"
export SAID_LLM_PROVIDER_ORDER="Parasail,Novita,SiliconFlow"
export SAID_LLM_REASONING_EFFORT="off"
export SAID_LLM_TEMPERATURE="1.0"
unset GROQ_API_KEY GROQ_MODEL

echo "ticket,bucket,arm,model,pass,attempts,secs" > "$OUT"
mapfile -t ROWS < <(cd "$ROOT" && "$NODE" -e 'for(const x of require("./tickets.json").tickets) console.log([x.id,x.bucket,x.file,x.task].join("\t"))')
regen() { ( cd "$ROOT" && "$NODE" gen-seeds.js >/dev/null 2>&1 ); }
gate()  { "$NODE" "$REPO/test/$1.test.js" >/dev/null 2>&1; }

for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r id bucket file task <<< "$row"
  regen; s=$(date +%s)
  out=$("$SAID" --brain "$BRAIN" --repo "$REPO" --task "$task" --files "$file" \
        --build "$NODE test/$id.test.js" --max-attempts $((MAXR+1)) 2>&1)
  e=$(date +%s)
  att=$(echo "$out" | grep -oE "attempts=[0-9]+" | grep -oE "[0-9]+" | tail -1); [ -z "$att" ] && att=0
  if gate "$id"; then p=1; else p=0; fi
  echo "$id  $bucket  pass=$p  attempts=$att  $((e-s))s"
  echo "$id,$bucket,said-k2.6-instant,kimi-k2.6,$p,$att,$((e-s))" >> "$OUT"
done
echo "=== Arm A (k2.5) done -> $OUT ==="; cat "$OUT"
