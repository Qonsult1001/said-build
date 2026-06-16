#!/usr/bin/env bash
# Paired eval: K2.5(.said) vs Composer-2.5, 12 gate-checkable tickets.
# Arm A = said-orchestrate on kimi-k2.5 via OpenRouter.
# Arm B = cursor-agent on composer-2.5 (WSL, headless).
# Same starting seed per ticket per arm; gate = `node test/<id>.test.js` (exit 0).
# Records first-attempt pass + attempts/pass per arm into results/results.csv.
set -u
ROOT="/g/development/said-build/orchestration-eval"
REPO="$ROOT/repo"
NODE="/c/nvm4w/nodejs/node"
SAID="/g/development/said-build/target/debug/said-orchestrate.exe"
BRAIN="/g/development/Advisory/Advisory.said"   # any valid .said brain (recall optional here)
OUT="$ROOT/results/results.csv"
MAXR="${MAXR:-2}"   # retry budget (protocol default: max 2 repairs)

export OPENAI_API_KEY="${OPENROUTER_KEY}"
export SAID_LLM_BASE_URL="https://openrouter.ai/api/v1"
export SAID_LLM_MODEL="moonshotai/kimi-k2.5"
unset GROQ_API_KEY GROQ_MODEL

echo "ticket,bucket,arm,pass,attempts" > "$OUT"

# read tickets via node (id,bucket,file,fn,task)
mapfile -t ROWS < <("$NODE" -e '
  const t=require("'"$ROOT"'/tickets.json").tickets;
  for(const x of t) console.log([x.id,x.bucket,x.file,x.fn,x.task].join("\t"));
')

reset_ticket() { # restore one ticket file to its stub
  local id="$1" fn="$2"
  printf "// TODO: implement %s to make the test pass.\nfunction %s() {\n  throw new Error('not implemented');\n}\nmodule.exports = { %s };\n" "$fn" "$fn" "$fn" > "$REPO/$1file"
}

gate() { # returns 0 green
  local id="$1"; "$NODE" "$REPO/test/$id.test.js" >/dev/null 2>&1
}

regen() { cd "$ROOT" && "$NODE" gen-seeds.js >/dev/null 2>&1; }

run_said() { # ARM A: returns "pass attempts"
  local id="$1" file="$2" task="$3"
  regen
  local out; out=$("$SAID" --brain "$BRAIN" --repo "$REPO" \
    --task "$task" --files "$file" \
    --build "$NODE test/$id.test.js" --max-attempts $((MAXR+1)) 2>&1)
  local attempts; attempts=$(echo "$out" | grep -oE "attempts=[0-9]+" | grep -oE "[0-9]+" | tail -1)
  [ -z "$attempts" ] && attempts=0
  if gate "$id"; then echo "1 $attempts"; else echo "0 $attempts"; fi
}

run_composer() { # ARM B: cursor-agent loops itself; we gate after, and re-prompt with the gate error up to MAXR.
  local id="$1" file="$2" task="$3"
  regen
  local attempt=1 maxa=$((MAXR+1)) prompt="$task The gate is: node test/$id.test.js (must exit 0). Edit $file in this repo to make it pass."
  while [ $attempt -le $maxa ]; do
    wsl.exe bash -lic "cd '$REPO' && cursor-agent -p --force --model composer-2.5 --output-format text '$(echo "$prompt" | sed "s/'/'\\\\''/g")' >/dev/null 2>&1" >/dev/null 2>&1
    if gate "$id"; then echo "1 $attempt"; return; fi
    local err; err=$("$NODE" "$REPO/test/$id.test.js" 2>&1 | head -5 | tr '\n' ' ' | sed "s/'/ /g")
    prompt="$task The test still fails with: $err . Fix $file so node test/$id.test.js exits 0."
    attempt=$((attempt+1))
  done
  echo "0 $maxa"
}

for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r id bucket file fn task <<< "$row"
  echo "=== $id ($bucket) ==="
  # Arm A
  read pa aa <<< "$(run_said "$id" "$file" "$task")"
  echo "  said(K2.5):   pass=$pa attempts=$aa"
  echo "$id,$bucket,said-k2.5,$pa,$aa" >> "$OUT"
  # Arm B
  read pb ab <<< "$(run_composer "$id" "$file" "$task")"
  echo "  composer-2.5: pass=$pb attempts=$ab"
  echo "$id,$bucket,composer-2.5,$pb,$ab" >> "$OUT"
done

echo "=== DONE — results in $OUT ==="
cat "$OUT"
