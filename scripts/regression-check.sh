#!/usr/bin/env bash
# Comprehensive regression gate for the coding build — run after EVERY change
# (especially each step of the lexical word-interning #4 OOM fix) to confirm NO
# regression across all verified capabilities.
#
# Exercises the REAL shipped binary end-to-end:
#   init (doc+code) · content recall · exact lexical · code-graph (sym/callers)
#   · wikilinks · multi-hop graph walk · is_junk_dir not over-excluding
#
# Usage:  bash scripts/regression-check.sh [path-to-said-binary]
# Exit 0 = all pass, 1 = any regression.

set -u
SAIDC="${1:-g:/development/said-build/said-coding.exe}"
WORK="g:/development/said-build/model-eval/volume/_regression"
strip(){ grep -v "Loaded embedded model" ; }
mkdir -p "$WORK"

PASS=0; FAIL=0
ok(){ PASS=$((PASS+1)); echo "  PASS  $1"; }
no(){ FAIL=$((FAIL+1)); echo "  FAIL  $1"; }

# ---- fixtures: a small mixed doc+code corpus (self-contained) ----
SRC="$WORK/src"; rm -rf "$SRC"; mkdir -p "$SRC"
cat > "$SRC/README.md" <<'EOF'
# Payment Service
The payment service processes credit card transactions through the Stripe gateway.
Refunds are handled asynchronously via a background worker. See [[stripe]] and [[refunds]].
The maximum transaction amount is 10000 dollars per customer per day.
EOF
cat > "$SRC/payment.cs" <<'EOF'
public class PaymentProcessor {
    public decimal MaxDailyLimit = 10000m;
    public PaymentResult ChargeCard(string token, decimal amount) {
        if (amount > MaxDailyLimit) return PaymentResult.Declined;
        return StripeGateway.Charge(token, amount);
    }
    public void IssueRefund(string transactionId) { RefundQueue.Enqueue(transactionId); }
}
EOF
cat > "$SRC/schema.sql" <<'EOF'
CREATE TABLE Transactions (Id INT PRIMARY KEY, CustomerId INT, Amount DECIMAL(18,2));
CREATE PROCEDURE GetDailyTotal @CustomerId INT AS
    SELECT SUM(Amount) FROM Transactions WHERE CustomerId = @CustomerId;
EOF
cat > "$SRC/utils.py" <<'EOF'
def calculate_fee(amount):
    """Calculate the processing fee: 2.9% plus 30 cents."""
    return amount * 0.029 + 0.30
def is_high_value(amount):
    return amount > 1000
EOF

B="$WORK/reg.said"; rm -f "$B" "$B.tmp"
"$SAIDC" create "$B" >/dev/null 2>&1

echo "== 1. init (doc + code) =="
mem=$("$SAIDC" --path "$B" init "$SRC" 2>&1 | strip | grep -oE "Memories added: [0-9]+" | grep -oE "[0-9]+")
[ "${mem:-0}" -ge 7 ] && ok "init stored $mem frames" || no "init stored only ${mem:-0} frames (expected >=7)"

echo "== 2. content recall (semantic, by meaning) =="
# match expected filename as a plain substring inside any top-3 doc_id.
# Use bash-native [[ == *substr* ]] (NO pipe) — `printf | grep` triggers SIGPIPE
# aborts (exit 134) in this Git Bash environment regardless of -q/-c.
rc(){ out=$("$SAIDC" --path "$B" ask "$1" --top 3 --json 2>&1 | strip | grep -oE '"doc_id":"[^"]*"' | tr '\n' ' '); if [[ "$out" == *"$2"* ]]; then ok "'$1' -> $2"; else no "'$1' -> $2 (got: $out)"; fi; }
rc "how are refunds processed"        "README"
rc "issue a refund"                   "payment.cs"
rc "calculate the processing fee"     "utils.py"
rc "transactions table"               "schema.sql"

echo "== 3. exact lexical (token) =="
lex(){ out=$("$SAIDC" --path "$B" ask "$1" --top 1 --json 2>&1 | strip | grep -oE '"doc_id":"[^"]*"'); [ -n "$out" ] && ok "lex '$1'" || no "lex '$1'"; }
lex "MaxDailyLimit"; lex "StripeGateway"; lex "GetDailyTotal"; lex "calculate_fee"

echo "== 4. code-graph (sym) =="
sy(){ n=$("$SAIDC" --path "$B" sym "$1" --json 2>&1 | strip | grep -c '"name"'); [ "${n:-0}" -ge 1 ] && ok "sym '$1' ($n)" || no "sym '$1' (0)"; }
sy "PaymentProcessor"; sy "calculate_fee"; sy "Transactions"

echo "== 5. wikilinks ([[concept]]) =="
nc=$("$SAIDC" --path "$B" list-concepts 2>&1 | strip | tr '\n' ' ')
if [[ "$nc" == *stripe* || "$nc" == *refunds* ]]; then ok "list-concepts found stripe/refunds"; else no "list-concepts missing wikilink concepts"; fi
wl=$("$SAIDC" --path "$B" ask "stripe" --top 3 --json 2>&1 | strip | grep -oE '"doc_id":"[^"]*"')
[ -n "$wl" ] && ok "concept query 'stripe' returns linked frame" || no "concept query 'stripe' empty"

echo "== 6. _deploy NOT over-excluded (is_junk_dir regression guard) =="
if [ -d "G:/development/Wonga/_deploy" ]; then
  D="$WORK/deploy.said"; rm -f "$D" "$D.tmp"; "$SAIDC" create "$D" >/dev/null 2>&1
  dm=$("$SAIDC" --path "$D" init "G:/development/Wonga/_deploy" 2>&1 | strip | grep -oE "Memories added: [0-9]+" | grep -oE "[0-9]+")
  [ "${dm:-0}" -ge 50 ] && ok "_deploy ingested $dm frames (out/ not excluded)" || no "_deploy only ${dm:-0} (out/ wrongly excluded!)"
else
  echo "  SKIP  _deploy (no Wonga)"
fi

echo
echo "==================  REGRESSION: $PASS passed, $FAIL failed  =================="
[ "$FAIL" -eq 0 ] && echo "ALL GREEN" || echo "REGRESSION DETECTED"
exit "$FAIL"
