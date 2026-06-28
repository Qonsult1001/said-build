const assert = require('assert');
const C = require('../src/cart.js');

// BUG 1 — money precision: totals must be exact cents (no float drift).
{
  const cart = { items:[{price:0.10,qty:1},{price:0.20,qty:1}], coupons:[] };
  const t = C.checkout(cart, 0);
  assert.strictEqual(t, 0.30, `money precision: got ${t}`);
}
// BUG 4 — qty clamp: negative -> 0, fractional -> floor; total uses clamped qty.
{
  const it = C.setQty({price:5,qty:1}, -3);
  assert.strictEqual(it.qty, 0, `qty negative clamp: ${it.qty}`);
  const it2 = C.setQty({price:5,qty:1}, 2.7);
  assert.strictEqual(it2.qty, 2, `qty fractional floor: ${it2.qty}`);
}
// BUG 2 — discount clamp: a >100% coupon must floor the line at 0, never negative.
{
  const cart = { items:[{price:10,qty:1}], coupons:[{percent:150}] };
  const t = C.checkout(cart, 0);
  assert.strictEqual(t, 0, `over-100% coupon must clamp to 0: ${t}`);
}
// BUG 5 — coupon idempotency: applying the same code twice counts once.
{
  let cart = { items:[{price:100,qty:1}], coupons:[] };
  cart = C.addCoupon(cart, {code:'SAVE10',percent:10});
  cart = C.addCoupon(cart, {code:'SAVE10',percent:10}); // duplicate
  const t = C.checkout(cart, 0);
  assert.strictEqual(t, 90, `duplicate coupon must apply once -> 90, got ${t}`);
}
// BUG 3 — tax round-at-end: total rounded to cents once, not per line.
{
  const cart = { items:[{price:0.333,qty:3}], coupons:[] }; // subtotal 0.999
  const t = C.checkout(cart, 0.1); // *1.1 = 1.0989 -> 1.10
  assert.strictEqual(t, 1.10, `tax round-at-end: got ${t}`);
}
console.log('ok cart — all 5 edge bugs fixed'); process.exit(0);
