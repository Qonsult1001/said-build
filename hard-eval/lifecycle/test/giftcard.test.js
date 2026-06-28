const assert=require('assert');const {redeemTotal}=require('../src/giftcard.js');
// 3 cards of 0.333 x1, fee 10% -> subtotal 0.999, *1.1 = 1.0989 -> must be 1.10 (round at END)
const t = redeemTotal([{amount:0.333,count:1},{amount:0.333,count:1},{amount:0.333,count:1}], 0.10);
assert.strictEqual(t, 1.10, `gift-card money precision (round at END): got ${t}`);
console.log('ok giftcard'); process.exit(0);
