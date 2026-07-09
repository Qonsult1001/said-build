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

# ── AUTO-REGISTER the said-mcp server into any AI agents found (Claude Code / Desktop / Cursor /
#    Copilot). On by default; opt out with SAID_NO_CONNECT=1. We MERGE into each agent's existing
#    config (never clobber their other servers) and are idempotent (re-running just updates our entry).
#    Default brain: ~/.said/brain.said (created on first use). Override with SAID_BRAIN=/path.
if [ "${SAID_NO_CONNECT:-0}" != "1" ]; then
  brain="${SAID_BRAIN:-$HOME/.said/brain.said}"
  mkdir -p "$(dirname "$brain")"
  # Always use the ABSOLUTE binary path, not the bare name: agents launch MCP servers WITHOUT the
  # user's freshly-updated PATH (and already-running agents have the old PATH), so a bare "said-mcp"
  # fails to spawn until every agent is restarted. The absolute path always resolves.
  mcp_ref="$bindir/said-mcp"
  registered=""

  # Portable JSON merge helper: add {mcpServers|servers}.said-brain = {command,args} to a config file,
  # creating/backing-up as needed, without disturbing existing keys. Uses node (present with most
  # agent installs); if node is absent we skip JSON agents and print manual steps at the end.
  json_register() {  # $1=config file  $2=top key (mcpServers|servers)  $3=parent (""|mcp)
    cfg="$1"; topkey="$2"; parent="$3"
    command -v node >/dev/null 2>&1 || return 2
    mkdir -p "$(dirname "$cfg")"
    [ -f "$cfg" ] && cp "$cfg" "$cfg.said-bak" 2>/dev/null || true
    SAID_CFG="$cfg" SAID_TOPKEY="$topkey" SAID_PARENT="$parent" \
    SAID_MCPREF="$mcp_ref" SAID_BRAINPATH="$brain" node -e '
      const fs=require("fs");
      const f=process.env.SAID_CFG, top=process.env.SAID_TOPKEY, parent=process.env.SAID_PARENT;
      let j={}; try{ j=JSON.parse(fs.readFileSync(f,"utf8")); }catch(e){ j={}; }
      let host=j; if(parent){ host[parent]=host[parent]||{}; host=host[parent]; }
      host[top]=host[top]||{};
      host[top]["said-brain"]={ command:process.env.SAID_MCPREF, args:["--path",process.env.SAID_BRAINPATH] };
      fs.writeFileSync(f, JSON.stringify(j,null,2));
    ' && return 0 || return 1
  }

  # 1) Claude Code — write its user config (~/.claude.json) DIRECTLY with a proper command/args
  #    split, via the same node merge as the other agents. We do NOT use `claude mcp add`: its
  #    `-- <cmd> --path X` form rejects `--path` as an unknown option, and its single-string form
  #    ("said-mcp --path X") stuffs the WHOLE string into `command` with empty `args`, so Claude Code
  #    can't spawn it → "Failed to connect". node also tolerates the duplicate project keys that can
  #    live in ~/.claude.json (which would otherwise break a stricter parser) and only touches
  #    mcpServers. Register when the file exists OR the claude CLI is present.
  if [ -f "$HOME/.claude.json" ] || command -v claude >/dev/null 2>&1; then
    json_register "$HOME/.claude.json" "mcpServers" "" && registered="$registered claude-code" || true
  fi
  # 2) Claude Desktop — mcpServers, JSON.
  case "$(uname -s)" in
    Darwin) cdesk="$HOME/Library/Application Support/Claude/claude_desktop_config.json" ;;
    *)      cdesk="$HOME/.config/Claude/claude_desktop_config.json" ;;
  esac
  if [ -d "$(dirname "$cdesk")" ] || command -v claude >/dev/null 2>&1; then
    json_register "$cdesk" "mcpServers" "" && registered="$registered claude-desktop" || true
  fi
  # 3) Cursor — global ~/.cursor/mcp.json, mcpServers.
  if [ -d "$HOME/.cursor" ] || command -v cursor >/dev/null 2>&1; then
    json_register "$HOME/.cursor/mcp.json" "mcpServers" "" && registered="$registered cursor" || true
  fi
  # 4) GitHub Copilot (VS Code) — settings.json, mcp.servers (note the different schema).
  for vs in "$HOME/.config/Code/User/settings.json" \
            "$HOME/Library/Application Support/Code/User/settings.json"; do
    if [ -f "$vs" ]; then
      json_register "$vs" "servers" "mcp" && registered="$registered copilot(vscode)" || true
    fi
  done

  echo ""
  if [ -n "$registered" ]; then
    echo "said: connected the brain to your agent(s):$registered"
    echo "  brain file: $brain   (restart / reload each agent to pick it up)"
  else
    echo "said: no AI agent auto-detected. To connect one, see the connect guide, or set it up with the"
    echo "  MCP config:  command \"$mcp_ref\"  args [\"--path\", \"$brain\"]"
  fi
fi

echo ""
echo "Done. Your agent can now remember things — just tell it \"remember …\" and ask later."
echo "Terminal use:  said create my-brain.said  &&  said --path my-brain.said add \"my first note\""
