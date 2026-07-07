#!/bin/sh
# said — one-line installer for Linux and macOS.
#
#   curl -fsSL https://github.com/Qonsult1001/said-build/releases/latest/download/install.sh | sh
#
# Downloads the correct `said` + `said-mcp` binaries for this OS/arch from the latest GitHub Release,
# installs them to ~/.local/bin (or /usr/local/bin with sudo), and verifies. No build, no dependencies.
# Override the bundle with SAID_BUNDLE=coding (default: brain, the free tier).
set -eu

REPO="Qonsult1001/said-build"
BUNDLE="${SAID_BUNDLE:-brain}"

# ── detect OS + arch → the release artifact name (matches build-binaries.yml matrix) ──
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux)  plat="linux-x64" ;;
  Darwin)
    case "$arch" in
      arm64|aarch64) plat="macos-arm64" ;;
      *) echo "said: only Apple Silicon (arm64) macOS builds are published; got $arch" >&2; exit 1 ;;
    esac ;;
  *) echo "said: unsupported OS '$os' (use install.ps1 on Windows)" >&2; exit 1 ;;
esac

asset="said-${BUNDLE}-${plat}.zip"
url="https://github.com/${REPO}/releases/latest/download/${asset}"

# ── pick an install dir (prefer ~/.local/bin, no sudo) ──
bindir="${SAID_BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$bindir"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "said: downloading $asset …"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$url" -o "$tmp/said.zip"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "$tmp/said.zip" "$url"
else
  echo "said: need curl or wget" >&2; exit 1
fi

echo "said: extracting …"
if command -v unzip >/dev/null 2>&1; then
  unzip -q "$tmp/said.zip" -d "$tmp"
else
  echo "said: need unzip" >&2; exit 1
fi

# ── install the two brain binaries ──
for b in said said-mcp; do
  if [ -f "$tmp/$b" ]; then
    install -m 0755 "$tmp/$b" "$bindir/$b"
  fi
done

echo "said: installed to $bindir"
"$bindir/said" --version || true

# ── PATH hint ──
case ":$PATH:" in
  *":$bindir:"*) : ;;
  *)
    echo ""
    echo "  Add $bindir to your PATH (then reopen your shell):"
    echo "    echo 'export PATH=\"$bindir:\$PATH\"' >> ~/.profile"
    ;;
esac

echo ""
echo "Done. Try:  said create my-brain.said  &&  said --path my-brain.said add \"my first note\""
