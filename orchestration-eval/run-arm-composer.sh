#!/usr/bin/env bash
# Arm B: Cursor Composer 2.5 via cursor-agent (WSL, headless), same 12 tickets,
# same seed state, same node gate. cursor-agent edits files directly; we gate
# after each attempt and re-prompt with the failure up to the retry budget.
set -u
ROOT="/g/development/said-build/orchestration-eval"
REPO="$ROOT/repo"
REPO_WSL="/mnt/g/development/said-build/orchestration-eval/repo"  # WSL mount path
NODE="/c/nvm4w/nodejs/node"
OUT="$ROOT/results/arm-composer.csv"
MAXR="${MAXR:-2}"
MODEL="${MODEL:-composer-2.5}"

echo "ticket,bucket,arm,model,pass,attempts,secs" > "$OUT"
mapfile -t ROWS < <(cd "$ROOT" && "$NODE" -e 'for(const x of require("./tickets.json").tickets) console.log([x.id,x.bucket,x.file,x.task].join("\t"))')

regen() { ( cd "$ROOT" && "$NODE" gen-seeds.js >/dev/null 2>&1 ); }
gate()  { "$NODE" "$REPO/test/$1.test.js" >/dev/null 2>&1; }

run_composer() { # id file task -> "pass attempts"
  local id="$1" file="$2" task="$3"
  local attempt=1 maxa=$((MAXR+1))
  local base="$task The gate is: node test/$id.test.js (must exit 0). Edit $file to make it pass. Output only the edited file."
  local prompt="$base"
  while [ $attempt -le $maxa ]; do
    # cursor-agent runs in WSL; the repo path is a drvfs mount, usable as-is.
    local esc; esc=$(printf '%s' "$prompt" | sed "s/'/'\\\\''/g")
    wsl.exe bash -lic "cd $REPO_WSL && cursor-agent -p --force --model $MODEL --output-format text '$esc'" >/dev/null 2>&1
    if gate "$id"; then echo "1 $attempt"; return; fi
    local err; err=$("$NODE" "$REPO/test/$id.test.js" 2>&1 | head -4 | tr '\n' ' ' | sed "s/'/ /g")
    prompt="$base It still fails: $err . Fix $file."
    attempt=$((attempt+1))
  done
  echo "0 $MAXR"
}

for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r id bucket file task <<< "$row"
  regen
  s=$(date +%s)
  read p a <<< "$(run_composer "$id" "$file" "$task")"
  e=$(date +%s)
  echo "$id  $bucket  pass=$p  attempts=$a  $((e-s))s"
  echo "$id,$bucket,composer,$MODEL,$p,$a,$((e-s))" >> "$OUT"
done
echo "=== Arm B done -> $OUT ==="; cat "$OUT"
