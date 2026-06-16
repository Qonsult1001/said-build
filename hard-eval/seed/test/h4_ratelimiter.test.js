const assert = require('assert');
const { RateLimiter } = require('../src/h4_ratelimiter.js');
const close = (a, b, m) => assert.ok(Math.abs(a - b) < 1e-6, `${m}: ${a} vs ${b}`);

// 1. Starts full; can burst up to capacity then is empty.
{
  const r = new RateLimiter(5, 1); // 5 cap, 1 token/sec
  close(r.available(0), 5, 'starts full');
  assert.strictEqual(r.tryRemove(5, 0), true, 'burst of 5 ok');
  close(r.available(0), 0, 'empty after burst');
  assert.strictEqual(r.tryRemove(1, 0), false, 'empty -> reject');
}
// 2. Refill over time (1/sec): after 3s, 3 tokens available (now in ms).
{
  const r = new RateLimiter(10, 1);
  r.tryRemove(10, 0);           // empty
  close(r.available(3000), 3, '3 tokens after 3s');
  assert.strictEqual(r.tryRemove(3, 3000), true);
  close(r.available(3000), 0, 'spent the 3');
}
// 3. Refill is capped at capacity (no overflow).
{
  const r = new RateLimiter(5, 100);
  r.tryRemove(5, 0);
  close(r.available(10000), 5, 'capped at capacity, not 100*10');
}
// 4. Fractional refill: 2 tokens/sec, after 500ms -> 1 token, tryRemove(1) ok at 500ms.
{
  const r = new RateLimiter(4, 2);
  r.tryRemove(4, 0);
  close(r.available(500), 1, '0.5s * 2/s = 1');
  assert.strictEqual(r.tryRemove(1, 500), true);
  assert.strictEqual(r.tryRemove(1, 500), false, 'now empty again');
}
// 5. Partial: cannot remove more than available; state unchanged on reject.
{
  const r = new RateLimiter(10, 1);
  r.tryRemove(7, 0);            // 3 left
  assert.strictEqual(r.tryRemove(5, 0), false, '5 > 3 -> reject');
  close(r.available(0), 3, 'reject does not deduct');
  assert.strictEqual(r.tryRemove(3, 0), true);
}
// 6. available() never exceeds capacity and never goes negative.
{
  const r = new RateLimiter(2, 1);
  close(r.available(99999), 2, 'capped');
  r.tryRemove(2, 0);
  assert.ok(r.available(0) >= 0, 'non-negative');
}
console.log('ok h4_ratelimiter'); process.exit(0);
