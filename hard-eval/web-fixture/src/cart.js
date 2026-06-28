// Shopping cart + checkout logic for the website. (Has known edge bugs to fix.)

// BUG 1 (float money): naive float arithmetic -> 0.1+0.2 rounding errors in totals.
function lineTotal(price, qty) {
  return price * qty;
}

// BUG 2 (discount stacking order): applies coupon BEFORE tax incorrectly, and a 100%+ coupon goes negative.
function applyDiscount(subtotal, percent) {
  return subtotal - subtotal * (percent / 100);
}

// BUG 3 (tax rounding): rounds each line then sums -> off-by-a-cent vs round-at-end.
function withTax(amount, taxRate) {
  return amount + amount * taxRate;
}

// BUG 4 (qty clamp / integer): qty can be negative or fractional; no clamp.
function setQty(item, qty) {
  item.qty = Math.max(0, Math.floor(qty));
  return item;
}

// BUG 5 (debounce/race in coupon apply): re-applying the same coupon stacks it (no idempotency).
function addCoupon(cart, code) {
  if (!cart.coupons.some(c => c.code === code.code)) cart.coupons.push(code);
  return cart;
}

function checkout(cart, taxRate) {
  let subtotal = 0;
  for (const it of cart.items) subtotal += lineTotal(it.price, it.qty);
  let total = subtotal;
  for (const c of cart.coupons) total = Math.max(0, applyDiscount(total, c.percent));
  total = withTax(total, taxRate);
  return Math.round(total * 100) / 100;
}

module.exports = { lineTotal, applyDiscount, withTax, setQty, addCoupon, checkout };
