// TokenBucket(capacity, refillPerSec): full at start; continuous fractional refill capped at capacity.
// tryRemove(n, nowMs) -> bool (deduct only if enough); available(nowMs) -> current tokens.
class TokenBucket {
  constructor(capacity, refillPerSec) { throw new Error('not implemented'); }
  tryRemove(n, nowMs) { throw new Error('not implemented'); }
  available(nowMs) { throw new Error('not implemented'); }
}
module.exports = { TokenBucket };
