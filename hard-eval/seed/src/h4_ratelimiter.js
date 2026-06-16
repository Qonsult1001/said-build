// TASK h4 (from-scratch, complex): a token-bucket rate limiter.
// new RateLimiter(capacity, refillPerSec): bucket starts FULL (capacity tokens).
//   - tokens refill continuously at refillPerSec (fractional allowed), capped at
//     capacity.
//   - tryRemove(n=1, now) -> true and deducts n if >= n tokens are available at
//     time `now` (ms); else false and deducts nothing.
//   - `now` is passed in (ms) so behaviour is deterministic/testable.
//   - available(now) -> current token count (number) at time `now`.
// Refill is based on elapsed time since the last successful state update.
//
// TODO: implement (stub throws so the gate is red).
class RateLimiter {
  constructor(capacity, refillPerSec) {
    throw new Error('not implemented');
  }
  tryRemove(n, now) { throw new Error('not implemented'); }
  available(now) { throw new Error('not implemented'); }
}
module.exports = { RateLimiter };
