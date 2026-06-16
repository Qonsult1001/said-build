#!/usr/bin/env bash
# Hard-eval Arm: said-orchestrate on the 4 hard tasks. Model/provider from env.
# Usage examples:
#   PROVIDER=groq MODEL=openai/gpt-oss-120b EFFORT=medium ./run-said.sh
#   PROVIDER=openrouter MODEL=moonshotai/kimi-k2.6 EFFORT=off TEMP=1.0 \
#     PORDER="Parasail,Novita,SiliconFlow" ./run-said.sh
set -u
ROOT="/g/development/said-build/hard-eval"
REPO="$ROOT/repo"; SEED="$ROOT/seed"
NODE="/c/nvm4w/nodejs/node"
SAID="/g/development/said-build/target/release/said-orchestrate.exe"
BRAIN="/g/development/Advisory/Advisory.said"
MAXR="${MAXR:-4}"            # hard tasks -> bigger repair budget
TAG="${TAG:-said}"
OUT="$ROOT/results/$TAG.csv"

if [ "${PROVIDER:-groq}" = "groq" ]; then
  export GROQ_API_KEY="$(grep PKGFW_GROQ_API_KEY /g/development/Advisory/.env | cut -d= -f2- | tr -d '\r')"
  export GROQ_MODEL="${MODEL:-openai/gpt-oss-120b}"
  unset OPENAI_API_KEY SAID_LLM_BASE_URL SAID_LLM_MODEL SAID_LLM_PROVIDER_ORDER
else
  export OPENAI_API_KEY="${OPENROUTER_API_KEY}"
  export SAID_LLM_BASE_URL="https://openrouter.ai/api/v1"
  export SAID_LLM_MODEL="${MODEL:-moonshotai/kimi-k2.6}"
  export SAID_LLM_PROVIDER_ORDER="${PORDER:-Parasail,Novita,SiliconFlow}"
  unset GROQ_API_KEY GROQ_MODEL
fi
export SAID_LLM_REASONING_EFFORT="${EFFORT:-medium}"
[ -n "${TEMP:-}" ] && export SAID_LLM_TEMPERATURE="$TEMP"

reset() { rm -rf "$REPO/src" "$REPO/test"; cp -r "$SEED/src" "$REPO/"; cp -r "$SEED/test" "$REPO/"; }
gate()  { "$NODE" "$REPO/test/$1.test.js" >/dev/null 2>&1; }

echo "ticket,type,tag,model,pass,attempts,secs" > "$OUT"
mapfile -t ROWS < <(cd "$ROOT" && "$NODE" -e 'for(const x of require("./tasks.json").tasks) console.log([x.id,x.type,x.files,x.task].join("\t"))')

for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r id type file task <<< "$row"
  reset; s=$(date +%s)
  out=$("$SAID" --brain "$BRAIN" --repo "$REPO" --task "$task" --files "$file" \
        --build "$NODE test/$id.test.js" --max-attempts $((MAXR+1)) 2>&1)
  e=$(date +%s)
  att=$(echo "$out" | grep -oE "attempts=[0-9]+" | grep -oE "[0-9]+" | tail -1); [ -z "$att" ] && att=0
  if gate "$id"; then p=1; else p=0; fi
  echo "$id  $type  pass=$p  att=$att  $((e-s))s"
  echo "$id,$type,$TAG,${MODEL:-gpt-oss-120b},$p,$att,$((e-s))" >> "$OUT"
done
echo "=== $TAG done -> $OUT ==="; cat "$OUT"
