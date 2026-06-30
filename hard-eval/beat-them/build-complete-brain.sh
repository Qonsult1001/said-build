#!/usr/bin/env bash
# Build the ONE complete suite brain the DOC-FAITHFUL way (docs/07-cli-reference/init.md + 14.15):
# a SINGLE full `said init` over the whole repo root -> AST + sym + trigram + OKF + harvest-blueprints,
# all in one pass (no piecemeal per-subdir inits that clobber SYMS). Then APPEND the distilled Claude
# memories + git history via save-memory/add (append, no SYMS rebuild). This is the "entire init process,
# everything for coding included" the owner mandated.
set -u
SAID="g:/development/said-build/target/debug/said.exe"
ROOT="g:/development/said-build"
BR="$ROOT/hard-eval/beat-them/claude-import/claude.said"
GITDIR="$ROOT/hard-eval/beat-them/claude-import/git-history"

rm -f "$BR" "$BR.spill"
"$SAID" create "$BR" >/dev/null 2>&1
export SAID_PROJECT=said-build SAID_OKF_LINKS=1 SAID_INIT_HARVEST=1

# dump full git history to searchable files (commits = episodic memory)
mkdir -p "$GITDIR"; rm -f "$GITDIR"/*.txt
for h in $(git -C "$ROOT" rev-list HEAD); do
  { git -C "$ROOT" show -s --format='commit %H%nauthor %an%ndate %ad%n%n%s%n%n%b' "$h"
    echo; echo '--- files changed ---'; git -C "$ROOT" show --stat --format='' "$h"; } > "$GITDIR/$h.txt"
done
echo "git history: $(ls "$GITDIR"/*.txt 2>/dev/null | wc -l) commits"

# ONE FULL INIT over the whole repo root -> code(AST/sym) + docs + git-history, with OKF + harvest on.
# (the harness git-history dir lives under hard-eval which is inside root, so it's included)
echo "=== single full init over repo root (AST + sym + OKF + harvest) ==="
"$SAID" --path "$BR" init "$ROOT" 2>&1 | grep -iE "Memories added|harvest|okf|Symbol" | tail -6

echo "=== verify ALL coding-brain kinds ==="
echo -n "  symbols: "; "$SAID" --path "$BR" stats --verbose 2>/dev/null | grep -i "Symbol table" | grep -oE "[0-9]+ unique"
echo -n "  sym build_concept_links: "; "$SAID" --path "$BR" sym build_concept_links --json 2>/dev/null | grep -c '"name"'
echo -n "  blueprints (harvested): "; "$SAID" --path "$BR" recall-blueprint --shape "handler" --min-similarity 0.0 2>/dev/null | grep -c "Blueprint"
"$SAID" --path "$BR" stats 2>/dev/null | grep -iE "Memories:|File size"
echo "BUILD_COMPLETE_DONE"
