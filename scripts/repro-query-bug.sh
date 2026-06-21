#!/usr/bin/env bash
# Red/green loop for issue #1: SCA semantic search returns nothing.
#
# Tightened: the bug is NOT "query vs ask" — `ask` only seemed to work because its
# GREP engine caught keyword overlap. The real failure is that SCA semantic recall
# (1-bit fingerprint search) returns 0 hits. So we assert on a PURE-SEMANTIC match:
# a query with NO lexical overlap with the stored doc, which only SCA can satisfy.
#
# RED   = both `query` and a no-keyword `ask` return 0 results (SCA dead).
# GREEN = the pure-semantic query returns the doc (SCA fingerprint search works).
#
# Usage: scripts/repro-query-bug.sh /path/to/said.exe   (relative ok; resolved abs)
# Exit 1 = red (bug present), 0 = green (fixed).
set -u
BIN="${1:-./target/release/said.exe}"
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
SBX="$(mktemp -d -t query-bug-XXXX)"
trap 'rm -rf "$SBX"' EXIT
cd "$SBX"

# Stored doc and probes share MEANING but as little vocabulary as possible, so a hit
# can only come from semantic fingerprints, not literal keyword grep.
"$BIN" create test.said >/dev/null 2>&1
"$BIN" --path test.said add "The feline curled up beside the warm hearth and slept." --id doc1 >/dev/null 2>&1

SEM_QUERY="a cat napping near a cozy fireplace"

Q_JSON="$("$BIN" --path test.said --json query "$SEM_QUERY" --top 5 2>/dev/null)"
A_OUT="$("$BIN" --path test.said ask "$SEM_QUERY" 2>/dev/null)"

echo "--- query --json (pure-semantic probe) ---"
echo "$Q_JSON"
echo "--- ask (same pure-semantic probe) ---"
echo "$A_OUT"
echo "------------------------------------------"

Q_HITS="$(printf '%s' "$Q_JSON" | grep -o '"doc_id"' | wc -l | tr -d ' ')"
A_HITS="$(printf '%s' "$A_OUT" | grep -c 'doc1')"

if [ "$Q_HITS" -ge 1 ] || [ "$A_HITS" -ge 1 ]; then
  echo "GREEN: semantic recall returned the doc (query=$Q_HITS ask=$A_HITS). SCA works."
  exit 0
else
  echo "RED: pure-semantic recall returned 0 from both query and ask. SCA fingerprint search is dead (bug #1)."
  exit 1
fi
