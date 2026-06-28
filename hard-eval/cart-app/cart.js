// Cart + checkout logic for the Said Shop. Pure functions (browser + node testable).
// Built by a Claude agent using .said as working memory; fixes recorded via learn_fix as concluded.

const round2 = (n) => Math.round(n * 100) / 100; // money: round to cents

function lineTotal(price, qty) {
  return price * qty; // raw; rounding happens once at the end (learned: per-line rounding drifts)
}

function applyDiscount(amount, percent) {
  // discount clamp: a >100% coupon must floor at 0, never negative (learned)
  return Math.max(0, amount - amount * (percent / 100));
}

function setQty(qty) {
  // qty clamp: non-negative integer (learned: negative -> 0, fractional -> floor)
  return Math.max(0, Math.floor(qty));
}

function addCoupon(coupons, code) {
  // coupon idempotency: same code counts once (learned)
  if (!coupons.some((c) => c.code === code.code)) coupons.push(code);
  return coupons;
}

function checkout(cart, taxRate) {
  let subtotal = 0;
  for (const it of cart.items) subtotal += lineTotal(it.price, it.qty);
  let afterDiscount = subtotal;
  for (const c of cart.coupons) afterDiscount = applyDiscount(afterDiscount, c.percent);
  const discount = subtotal - afterDiscount;
  const tax = afterDiscount * taxRate;
  const total = round2(afterDiscount + tax); // round ONCE at the end (learned: the money trap)
  return { subtotal: round2(subtotal), discount: round2(discount), tax: round2(tax), total };
}

if (typeof module !== 'undefined') module.exports = { lineTotal, applyDiscount, setQty, addCoupon, checkout, round2 };
