#!/usr/bin/env bash
# C# skill factory: verify a curated skill's reference against the dotnet gate, then
# store it into csharp.said. Reusable per-skill step (curation is authored per entry).
#
# Usage: bash build-skill.sh <entry-id>
# Expects: entries/<id>/meta.env  (PROBLEM, LEARNINGS, ERRORS, TAGS, GATE_TEST, REF_SRC)
#          entries/<id>/src/*.cs   (the curated reference impl)
#          entries/<id>/test/*.cs  (the xunit gate)
set -u
ID="$1"
ROOT="/g/development/said-build/learning-factory/csharp"
E="$ROOT/entries/$ID"
H="$ROOT/harness"
SAID="/g/development/said-build/target/release/said.exe"
PACK="/g/development/said-build/.said/skills/csharp.said"
NODE="/c/nvm4w/nodejs/node"
[ -f "$E/meta.env" ] || { echo "[$ID] no meta.env"; exit 1; }
source "$E/meta.env"

# 1) swap this skill's src+test into the shared harness project
rm -f "$H/src"/*.cs "$H/test"/*.cs
cp "$E/src/"*.cs "$H/src/" 2>/dev/null
cp "$E/test/"*.cs "$H/test/" 2>/dev/null

# 2) GATE: the curated reference MUST pass dotnet test
if dotnet test "$H/test/Test.csproj" -v q --nologo >/dev/null 2>&1; then
  echo "[$ID] gate GREEN"
else
  echo "[$ID] gate RED — reference does not pass; NOT stored"; exit 2
fi

# 3) store into csharp.said (edits = the reference src as a write-file; dedup by label)
REFFILE="$E/src/$REF_SRC"
"$NODE" -e 'const fs=require("fs");console.log(JSON.stringify([{file:process.argv[2],mode:"write-file",content:fs.readFileSync(process.argv[1],"utf8")}]))' "$REFFILE" "src/$REF_SRC" > "$E/.edits.json"
"$SAID" learn-fix --path "$PACK" --label "$ID" \
  --problem "$PROBLEM" --edits-file "$E/.edits.json" --files "$TAGS" \
  --learnings "$LEARNINGS" --errors "$ERRORS" --json >/dev/null 2>&1 \
  && echo "[$ID] stored (tags: $TAGS)" || { echo "[$ID] store FAILED"; exit 3; }
rm -f "$E/.edits.json"
