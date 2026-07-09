#!/usr/bin/env bash
# Production build of the .said coding binaries — compiled to run NATIVELY fast on this host.
# See docs/said-structure/35-production-build.md for what each lever does.
#
#   - profile = production  : release + fat LTO (whole-program inline) + panic=abort + strip
#   - features = coding     : embed-model (16MB encoder baked in) + code (AST/SYMS) + simd (AVX Hamming)
#   - RUSTFLAGS target-cpu=native : the SCA fingerprint popcount + encoder mean-pool compile to the host's
#                                   exact AVX2/AVX-512 instructions (simsimd auto-dispatches at runtime, but
#                                   native lets the surrounding scalar/vector code use them too).
#
# Fingerprints are bit-identical to a normal build (same float ops) — this is purely a speed compile.
set -euo pipefail
cd "$(dirname "$0")/.."

export RUSTFLAGS="-C target-cpu=native ${RUSTFLAGS:-}"
echo "== building said-cli + said-mcp (profile=production, features=coding, target-cpu=native) =="
cargo build --profile production -p said-cli -p said-mcp --no-default-features --features coding

OUT=target/production
echo ""
echo "== built =="
ls -la "$OUT"/said.exe "$OUT"/said-mcp.exe 2>/dev/null | awk '{printf "  %-18s %.1f MB\n", $NF, $5/1e6}' \
  || ls -la "$OUT"/said "$OUT"/said-mcp 2>/dev/null | awk '{printf "  %-18s %.1f MB\n", $NF, $5/1e6}'
echo ""
echo "  use these for production / benchmarking. The MCP server (said-mcp) is the RESIDENT path:"
echo "  it loads the encoder ONCE and serves every query — the fast deployment for coding agents."
