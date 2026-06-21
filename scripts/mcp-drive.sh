#!/usr/bin/env bash
# Drive the real said-mcp server over stdio JSON-RPC.
# Reads newline-delimited JSON-RPC requests from a file ($2) on stdin, prints
# the server's stdout. Runs in an isolated temp dir with an isolated brain.
#
# Usage: scripts/mcp-drive.sh /abs/path/to/said-mcp.exe requests.jsonl [workdir]
set -u
BIN="$1"; REQ="$2"; WORK="${3:-}"
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
REQ="$(cd "$(dirname "$REQ")" && pwd)/$(basename "$REQ")"
if [ -z "$WORK" ]; then WORK="$(mktemp -d -t mcp-XXXX)"; fi
cd "$WORK"
# Feed requests on stdin; MCP stdio server reads line-delimited JSON-RPC.
"$BIN" < "$REQ"
