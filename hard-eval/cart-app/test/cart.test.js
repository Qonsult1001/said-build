const assert=require('assert');const C=require('../cart.js');
// money precision
{const r=C.checkout({items:[{price:0.10,qty:1},{price:0.20,qty:1}],coupons:[]},0);assert.strictEqual(r.total,0.30,`money ${r.total}`);}
// qty clamp
{assert.strictEqual(C.setQty(-3),0);assert.strictEqual(C.setQty(2.7),2);}
// discount >100% clamp
{const r=C.checkout({items:[{price:10,qty:1}],coupons:[{code:'X',percent:150}]},0);assert.strictEqual(r.total,0,`overclamp ${r.total}`);}
// coupon idempotency
{let cp=[];C.addCoupon(cp,{code:'SAVE10',percent:10});C.addCoupon(cp,{code:'SAVE10',percent:10});const r=C.checkout({items:[{price:100,qty:1}],coupons:cp},0);assert.strictEqual(r.total,90,`dupe ${r.total}`);}
// tax round-at-end
{const r=C.checkout({items:[{price:0.333,qty:3}],coupons:[]},0.10);assert.strictEqual(r.total,1.10,`tax ${r.total}`);}
console.log('ok cart-app — interactive cart logic verified');process.exit(0);
