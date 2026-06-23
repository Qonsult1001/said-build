#!/usr/bin/env bash
# GLOBAL test: every scenario across the real CLI + MCP binaries. Asserts PASS/FAIL per
# check so a regression anywhere shows up. Covers: core memory ops, all query types,
# wikilinks, import safety (code/docs/brackets), persistence, concepts, MCP parity, edges.
set -u
SAID="${SAID:-g:/development/said-build/said.exe}"
MCP="${MCP:-g:/development/said-build/target/release/said-mcp.exe}"
strip(){ grep -v "Loaded embedded model"; }
PASS=0; FAIL=0
ok(){ PASS=$((PASS+1)); printf "  PASS  %s\n" "$1"; }
no(){ FAIL=$((FAIL+1)); printf "  FAIL  %s   <-- %s\n" "$1" "$2"; }
check(){ # check "desc" "expected-substr" "actual"  (bash substring, no grep — avoids
         # a Git Bash crash with grep -qF on piped multiline input)
  case "$3" in
    *"$2"*) ok "$1" ;;
    *) no "$1" "want '$2'" ;;
  esac
}
checkn(){ # checkn "desc" "must NOT contain" "actual"
  case "$3" in
    *"$2"*) no "$1" "found forbidden '$2'" ;;
    *) ok "$1" ;;
  esac
}

W="$(mktemp -d)"; B="$W/g.said"
$SAID create "$B" >/dev/null 2>&1

echo "════ 1. CORE MEMORY OPS ════"
out=$($SAID --path "$B" add "The wifi password is sunflower-42." --id wifi 2>&1|strip); check "add returns ok" "Added" "$out"
out=$($SAID --path "$B" get wifi 2>&1|strip); check "get returns content" "sunflower-42" "$out"
out=$($SAID --path "$B" stats 2>&1|strip); check "stats shows memory" "Memories" "$out"
$SAID --path "$B" add "temp note to delete" --id tmp >/dev/null 2>&1
$SAID --path "$B" delete tmp >/dev/null 2>&1
out=$($SAID --path "$B" get tmp 2>&1|strip); checkn "delete removes from get" "temp note to delete" "$out"

echo "════ 2. QUERY TYPES (ask) ════"
$SAID --path "$B" add "Mom's birthday is June 14." --id bday >/dev/null 2>&1
$SAID --path "$B" add "The garage door code is 4827." --id garage >/dev/null 2>&1
$SAID --path "$B" add "I parked on level 3 of the airport long-stay car park." --id parking >/dev/null 2>&1
out=$($SAID --path "$B" ask "what is the wifi password" 2>&1|strip); check "lexical query" "wifi" "$out"
out=$($SAID --path "$B" ask "where did I leave the car" 2>&1|strip); check "semantic query (paraphrase)" "parking" "$out"
out=$($SAID --path "$B" ask "garage code" 2>&1|strip); check "numeric/keyword query" "garage" "$out"
out=$($SAID --path "$B" ask "when is moms birthday" 2>&1|strip); check "entity query" "bday" "$out"

echo "════ 3. WIKILINK GRAPH (the OKF feature) ════"
$SAID --path "$B" add "Dr. Sarah is the cardiologist I have seen since 2020. [[heart]] [[doctor]]" --id heartdoc >/dev/null 2>&1
$SAID --path "$B" add "Dr. Wei is the pulmonologist since 2020. [[lungs]] [[doctor]]" --id lungdoc >/dev/null 2>&1
out=$($SAID --path "$B" ask "who do I see about my heart" 2>&1|strip); check "wikilink bridge (heart->cardiologist)" "heartdoc" "$out"
out=$($SAID --path "$B" ask "who do I see about my lungs" 2>&1|strip); check "wikilink bridge (lungs->pulmonologist)" "lungdoc" "$out"
out=$($SAID --path "$B" list-concepts 2>&1|strip); check "list-concepts shows heart" "heart" "$out"
check "list-concepts shows shared doctor" "doctor" "$out"
out=$($SAID --path "$B" list-concepts --prefix lu 2>&1|strip); check "list-concepts prefix filter" "lungs" "$out"
checkn "prefix filter excludes others" "heart" "$out"

echo "════ 4. IMPORT SAFETY (code/brackets must NOT break or coin junk) ════"
$SAID --path "$B" add "Rust: let x = vec[[1,2],[3,4]]; matrix[[i]][[j]]; arr[[42]]." --id code1 >/dev/null 2>&1
$SAID --path "$B" add "Python a[[0]] b[[k]] nested[[x]][[y]] indexing snippet." --id code2 >/dev/null 2>&1
out=$($SAID --path "$B" get code1 2>&1|strip); check "code memory retrievable" "matrix" "$out"
concepts=$($SAID --path "$B" list-concepts 2>&1|strip)
checkn "no junk concept '1,2'" "1,2" "$concepts"
checkn "no junk concept '3,4'" "3,4" "$concepts"
# single-letter i/j from matrix[[i]][[j]] must be absent (only their exact concept lines)
ijlines=$(echo "$concepts" | grep -E '^[[:space:]]+[0-9]+[[:space:]]+[ij]$' || true)
checkn "no single-letter i/j concept" "j" "${ijlines:-none}"
out=$($SAID --path "$B" ask "matrix access code" 2>&1|strip); check "code still searchable" "code1" "$out"

echo "════ 5. PERSISTENCE (save → reopen) ════"
$SAID --path "$B" compact >/dev/null 2>&1
out=$($SAID --path "$B" ask "who do I see about my heart" 2>&1|strip); check "wikilink survives compact+reopen" "heartdoc" "$out"
out=$($SAID --path "$B" list-concepts 2>&1|strip); check "concepts survive compact+reopen" "heart" "$out"
out=$($SAID --path "$B" get wifi 2>&1|strip); check "plain memory survives compact" "sunflower-42" "$out"

echo "════ 6. PORTABILITY (copy file → query copy) ════"
cp "$B" "$W/copy.said"
out=$($SAID --path "$W/copy.said" ask "who do I see about my heart" 2>&1|strip); check "copied file: wikilink works" "heartdoc" "$out"
out=$($SAID --path "$W/copy.said" list-concepts 2>&1|strip); check "copied file: concepts present" "doctor" "$out"

echo "════ 7. EDGE CASES ════"
out=$($SAID --path "$B" ask "xyzzy nonexistent quux topic" 2>&1|strip); check "off-topic returns gracefully" "Ask:" "$out"
$SAID --path "$B" add "" --id empty 2>&1 >/dev/null; ok "empty add doesn't crash"
$SAID --path "$B" add "Unicode: café résumé 日本語 [[café]]" --id uni >/dev/null 2>&1
out=$($SAID --path "$B" get uni 2>&1|strip); check "unicode memory stored" "café" "$out"
out=$($SAID --path "$B" list-concepts --prefix caf 2>&1|strip); check "unicode wikilink concept" "café" "$out"

echo "════ 8. MCP PARITY (same ops via MCP server) ════"
MB="$W/mcp.said"; $SAID create "$MB" >/dev/null 2>&1
mcp_call(){ printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  "$1" | SAID_PATH="$MB" "$MCP" 2>/dev/null; }
out=$(mcp_call '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'|grep -o '"name":"[a-z_]*"'|tr '\n' ' ')
check "MCP advertises ask" '"name":"ask"' "$out"
check "MCP advertises list_concepts" '"name":"list_concepts"' "$out"
# remember with link, then list_concepts + ask, in-session
out=$(printf '%s\n%s\n%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"remember","arguments":{"content":"Dr Sarah cardiologist [[heart]]","id":"m1"}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_concepts","arguments":{}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"ask","arguments":{"query":"who do I see about my heart"}}}' \
  | SAID_PATH="$MB" "$MCP" 2>/dev/null)
check "MCP remember+list_concepts shows heart" "heart" "$(echo "$out"|grep '\"id\":3')"
check "MCP ask finds via wikilink" "cardiologist" "$(echo "$out"|grep '\"id\":4')"

echo
echo "════════════════════════════════════════"
echo "  GLOBAL TEST RESULT:  PASS=$PASS  FAIL=$FAIL"
echo "════════════════════════════════════════"
rm -rf "$W"
[ "$FAIL" -eq 0 ]
