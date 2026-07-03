#!/bin/bash
# INCREMENTAL MEGA-INGEST: add projects one-by-one into ONE brain, measure after each, until a wall.
# Answers "how far can we go" empirically. Bounded: a per-project time cap + a peak-RAM ceiling stop.
export PATH="$HOME/.cargo/bin:$PATH"
export SAID_PROJECT=""   # set per project below
PS="G:/development/said-build/target/production/said.exe"
BR="G:/development/said-build/hard-eval/mega/mega.said"
LOG="G:/development/said-build/hard-eval/mega/mega-ingest.log"
CSV="G:/development/said-build/hard-eval/mega/mega-ingest.csv"
DEV="/g/development"

# order: small -> large, so we get the growth curve and early wins before the giants.
PROJECTS=(
  "Vivere Onboarding|Vivere Onboarding"
  "dt-strorefront|dt-strorefront"
  "Agents|Agents"
  "Advisory|Advisory"
  "MCP|MCP"
  "SDK-DT|SDK-DT"
  "said-build|said-build/crates"
  "dt-gateway|dt-gateway"
  "Law|Law"
  "Wonga|Wonga"
  "AlphaGo|AlphaGo"
  "NeoroCore|NeoroCore"
  "SAID-ECHO|SAID-ECHO"
)

RAM_CEILING_MB=2500      # stop if a project's peak private exceeds this
PROJECT_TIMEOUT_S=1200   # 20 min per project cap

: > "$LOG"
echo "project,frames_total,added,brain_MB,ingest_s,peak_MB,recall_ok" > "$CSV"
rm -f "$BR" "$BR.spill" 2>/dev/null
"$PS" create "$BR" >/dev/null 2>&1
prev_frames=0

for entry in "${PROJECTS[@]}"; do
  name="${entry%%|*}"; sub="${entry##*|}"; dir="$DEV/$sub"
  [ -d "$dir" ] || { echo "SKIP (missing): $name" | tee -a "$LOG"; continue; }
  echo "==== INGEST: $name ($dir) ====" | tee -a "$LOG"

  # launch ingest (project-tagged) in background; poll peak private MB via powershell
  start=$(date +%s)
  SAID_PROJECT="$name" timeout "$PROJECT_TIMEOUT_S" "$PS" --path "$BR" init "$dir" >>"$LOG" 2>&1 &
  pid=$!
  peak=$(powershell -Command "
    \$pk=0
    while(\$true){ \$p=Get-Process said -EA SilentlyContinue; if(-not \$p){break}
      \$m=(\$p|Measure-Object PrivateMemorySize64 -Maximum).Maximum/1MB
      if(\$m -gt \$pk){\$pk=[math]::Round(\$m)}; Start-Sleep -Milliseconds 700 }
    \$pk" 2>/dev/null | tr -d '\r ')
  wait $pid 2>/dev/null
  end=$(date +%s)

  frames=$("$PS" --path "$BR" stats 2>/dev/null | grep -aoE "Memories:\s*[0-9,]+" | grep -oE "[0-9,]+" | tr -d ',')
  frames=${frames:-0}
  added=$((frames - prev_frames)); prev_frames=$frames
  mb=$(ls -la "$BR" 2>/dev/null | awk '{printf "%.0f",$5/1048576}')
  # recall check: ask something generic, pass if any result
  rok=$("$PS" --path "$BR" ask "error handling" --top 3 2>/dev/null | grep -aciE "result|\.cs|\.rs|\.sql|\.py" )
  rok=$([ "$rok" -gt 0 ] && echo yes || echo NO)

  echo "$name,$frames,$added,$mb,$((end-start)),${peak:-0},$rok" | tee -a "$CSV"
  echo "  -> total $frames frames (+$added), ${mb}MB, $((end-start))s, peak ${peak:-?}MB, recall=$rok" | tee -a "$LOG"

  # WALL checks
  if [ "${peak:-0}" -gt "$RAM_CEILING_MB" ]; then
    echo "!! WALL: peak ${peak}MB > ceiling ${RAM_CEILING_MB}MB after $name. Stopping." | tee -a "$LOG"; break
  fi
  if [ "$((end-start))" -ge "$PROJECT_TIMEOUT_S" ]; then
    echo "!! WALL: $name hit the ${PROJECT_TIMEOUT_S}s timeout. Stopping." | tee -a "$LOG"; break
  fi
done

echo "==== MEGA-INGEST COMPLETE ====" | tee -a "$LOG"
echo "final: $prev_frames frames, $(ls -la "$BR" | awk '{printf "%.0f MB",$5/1048576}')" | tee -a "$LOG"
cat "$CSV"
