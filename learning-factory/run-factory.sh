#!/usr/bin/env bash
# Learning Factory runner: for each task in the catalog, a STRONG model solves it
# against the real gate; on GREEN the orchestrator's learn step records the verified
# learning into the per-language brain (brains/<lang>.said). Run with whatever model
# you want as the "factory" (Groq gpt-oss for cheap, OpenRouter kimi/opus for strong).
#
# Usage:
#   PROVIDER=groq MODEL=openai/gpt-oss-120b bash run-factory.sh            # all js tasks
#   PROVIDER=openrouter MODEL=moonshotai/kimi-k2.5 LANG=javascript bash run-factory.sh
#   ONLY=lru_cache,token_bucket bash run-factory.sh                        # subset
set -u
ROOT="$(cd "$(dirname "$0")" && pwd)"
SAID_BUILD="/g/development/said-build"
NODE="/c/nvm4w/nodejs/node"
ORCH="$SAID_BUILD/target/release/said-orchestrate.exe"
# NOTE: do NOT use $LANG (that's the shell locale). Use $FLANG for the factory language.
LANG_SEL="${FLANG:-javascript}"
TASKS="$ROOT/tasks/$LANG_SEL"
BRAIN="$ROOT/brains/${BRAIN_NAME:-code}.said"       # built per-language brain
SAID="$SAID_BUILD/target/release/said.exe"
WORK="$ROOT/.work"                                  # scratch workspace (gitignored)
MAXR="${MAXR:-5}"

# Provider wiring (same pattern as hard-eval/run-said.sh).
if [ "${PROVIDER:-groq}" = "groq" ]; then
  export GROQ_API_KEY="$(grep PKGFW_GROQ_API_KEY /g/development/Advisory/.env | cut -d= -f2- | tr -d '\r')"
  export GROQ_MODEL="${MODEL:-openai/gpt-oss-120b}"
  unset OPENAI_API_KEY SAID_LLM_BASE_URL SAID_LLM_MODEL SAID_LLM_PROVIDER_ORDER
else
  export OPENAI_API_KEY="$(grep '^OPENROUTER_API_KEY' /g/development/Advisory/.env | cut -d= -f2- | tr -d '\r')"
  export SAID_LLM_BASE_URL="https://openrouter.ai/api/v1"
  export SAID_LLM_MODEL="${MODEL:-moonshotai/kimi-k2.5}"
  export SAID_LLM_PROVIDER_ORDER="${PORDER:-Parasail,Novita,SiliconFlow}"
  unset GROQ_API_KEY GROQ_MODEL
fi
export SAID_LLM_REASONING_EFFORT="${EFFORT:-medium}"

mkdir -p "$ROOT/brains" "$WORK/src" "$WORK/test"
[ -f "$BRAIN" ] || "$SAID" create "$BRAIN" >/dev/null 2>&1

# Read catalog rows for the selected language (id, files, task), filtered by ONLY.
mapfile -t ROWS < <("$NODE" -e '
  const fs=require("fs");
  const cat=JSON.parse(fs.readFileSync(process.argv[1],"utf8"));
  const lang=process.argv[2];
  const only=(process.argv[3]||"").split(",").filter(Boolean);
  for(const t of cat.tasks){
    if(t.lang!==lang) continue;
    if(only.length && !only.includes(t.id)) continue;
    console.log([t.id, t.files, t.task].join("\t"));
  }' "$ROOT/tasks/catalog.json" "$LANG_SEL" "${ONLY:-}")

echo "factory: lang=$LANG_SEL  model=${MODEL:-default}  brain=$BRAIN  tasks=${#ROWS[@]}"
green=0; total=0
for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r id files task <<< "$row"
  total=$((total+1))
  # Reset the seed into the scratch workspace.
  rm -rf "$WORK/src" "$WORK/test"; mkdir -p "$WORK/src" "$WORK/test"
  cp "$TASKS/seed/$id.js" "$WORK/src/$id.js"
  cp "$TASKS/test/$id.test.js" "$WORK/test/$id.test.js"
  # Strong model solves it; on green the learn step records into $BRAIN.
  "$ORCH" --brain "$BRAIN" --repo "$WORK" --task "$task" --files "$files" \
    --build "$NODE test/$id.test.js" --max-attempts "$MAXR" >/dev/null 2>&1
  if "$NODE" "$WORK/test/$id.test.js" >/dev/null 2>&1; then
    echo "  [GREEN] $id  -> learned"
    green=$((green+1))
  else
    echo "  [red]   $id  (not learned; gate never passed)"
  fi
done
echo "=== factory done: $green/$total green+learned into $BRAIN ==="
"$SAID" --path "$BRAIN" stats 2>&1 | grep -iE "Active frames|SCA docs"
