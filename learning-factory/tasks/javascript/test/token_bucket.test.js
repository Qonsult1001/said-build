const assert = require('assert');
const { TokenBucket } = require('../src/token_bucket.js');
const b = new TokenBucket(10, 5);             // cap 10, 5/sec
assert.strictEqual(b.tryRemove(8, 0), true);  // 10 -> 2
assert.strictEqual(b.tryRemove(5, 0), false); // only 2 left, no deduction
assert.strictEqual(Math.round(b.available(0)), 2);
assert.strictEqual(Math.round(b.available(1000)), 7);  // +5 after 1s -> 7
assert.strictEqual(b.tryRemove(7, 1000), true);        // 7 -> 0
assert.ok(b.available(100000) <= 10 + 1e-9);           // capped at capacity
console.log('ok token_bucket'); process.exit(0);
