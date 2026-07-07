#!/usr/bin/env bash
# Build a said .deb from already-built Linux binaries.
# Usage: build-deb.sh <version> <path-to-said> <path-to-said-mcp> <out-dir>
# Produces: <out-dir>/said_<version>_amd64.deb   (installs to /usr/bin, no PATH edit needed)
set -euo pipefail

VERSION="$1"; SAID_BIN="$2"; MCP_BIN="$3"; OUT="${4:-dist}"
here="$(cd "$(dirname "$0")" && pwd)"

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT

mkdir -p "$root/usr/bin" "$root/DEBIAN" "$root/usr/share/doc/said"
install -m 0755 "$SAID_BIN" "$root/usr/bin/said"
install -m 0755 "$MCP_BIN"  "$root/usr/bin/said-mcp"

sed "s/__VERSION__/${VERSION}/" "$here/deb/control.template" > "$root/DEBIAN/control"
# copyright / doc (keeps lintian quieter; harmless)
printf 'said %s — portable single-file brain.\nhttps://github.com/Qonsult1001/said-build\n' "$VERSION" \
  > "$root/usr/share/doc/said/README"

mkdir -p "$OUT"
deb="$OUT/said_${VERSION}_amd64.deb"
dpkg-deb --build --root-owner-group "$root" "$deb"
echo "built $deb"
