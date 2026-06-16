#!/usr/bin/env bash
# Hard-eval Arm: Cursor Composer 2.5 via cursor-agent (WSL headless) on 4 hard tasks.
set -u
ROOT="/g/development/said-build/hard-eval"
REPO="$ROOT/repo"; SEED="$ROOT/seed"
REPO_WSL="/mnt/g/development/said-build/hard-eval/repo"
NODE="/c/nvm4w/nodejs/node"
MAXR="${MAXR:-4}"; MODEL="${MODEL:-composer-2.5}"; TAG="${TAG:-composer}"
OUT="$ROOT/results/$TAG.csv"

reset() { rm -rf "$REPO/src" "$REPO/test"; cp -r "$SEED/src" "$REPO/"; cp -r "$SEED/test" "$REPO/"; }
gate()  { "$NODE" "$REPO/test/$1.test.js" >/dev/null 2>&1; }

echo "ticket,type,tag,model,pass,attempts,secs" > "$OUT"
mapfile -t ROWS < <(cd "$ROOT" && "$NODE" -e 'for(const x of require("./tasks.json").tasks) console.log([x.id,x.type,x.files,x.task].join("\t"))')

run() { # id file task -> "pass attempts"
  local id="$1" file="$2" task="$3" attempt=1 maxa=$((MAXR+1))
  local base="$task The gate is: node test/$id.test.js (must exit 0). Edit $file."
  local prompt="$base"
  while [ $attempt -le $maxa ]; do
    local esc; esc=$(printf '%s' "$prompt" | sed "s/'/'\\\\''/g")
    wsl.exe bash -lic "cd $REPO_WSL && cursor-agent -p --force --model $MODEL --output-format text '$esc'" >/dev/null 2>&1
    if gate "$id"; then echo "1 $attempt"; return; fi
    local err; err=$("$NODE" "$REPO/test/$id.test.js" 2>&1 | head -5 | tr '\n' ' ' | sed "s/'/ /g")
    prompt="$base Still failing: $err . Fix $file."
    attempt=$((attempt+1))
  done
  echo "0 $MAXR"
}

for row in "${ROWS[@]}"; do
  IFS=$'\t' read -r id type file task <<< "$row"
  reset; s=$(date +%s)
  read p a <<< "$(run "$id" "$file" "$task")"
  e=$(date +%s)
  echo "$id  $type  pass=$p  att=$a  $((e-s))s"
  echo "$id,$type,$TAG,$MODEL,$p,$a,$((e-s))" >> "$OUT"
done
echo "=== $TAG done -> $OUT ==="; cat "$OUT"
