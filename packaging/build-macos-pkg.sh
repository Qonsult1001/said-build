#!/usr/bin/env bash
# Build a said macOS .pkg from already-built arm64 binaries.
# Usage: build-macos-pkg.sh <version> <path-to-said> <path-to-said-mcp> <out-dir>
# Produces: <out-dir>/said-<version>-arm64.pkg  (installs said + said-mcp into /usr/local/bin)
#
# NOTE: this pkg is UNSIGNED. Gatekeeper will warn on download-and-run until we add an Apple
# Developer ID signature + notarisation (CIRCLE_PLAN distribution step — needs the cert). Local
# install (installer -pkg) works; a downloaded pkg needs right-click > Open the first time.
set -euo pipefail

VERSION="$1"; SAID_BIN="$2"; MCP_BIN="$3"; OUT="${4:-dist}"

payload="$(mktemp -d)"
trap 'rm -rf "$payload"' EXIT
mkdir -p "$payload/usr/local/bin"
install -m 0755 "$SAID_BIN" "$payload/usr/local/bin/said"
install -m 0755 "$MCP_BIN"  "$payload/usr/local/bin/said-mcp"

mkdir -p "$OUT"
pkg="$OUT/said-${VERSION}-arm64.pkg"
pkgbuild \
  --identifier dev.said.brain \
  --version "$VERSION" \
  --install-location / \
  --root "$payload" \
  "$pkg"
echo "built $pkg"
