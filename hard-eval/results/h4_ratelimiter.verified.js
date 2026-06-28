class RateLimiter {
  constructor(capacity, refillPerSec) {
    this.cap = capacity; this.rate = refillPerSec; this.tokens = capacity; this.last = null;
  }
  _refill(now) {
    if (this.last === null) { this.last = now; return; }
    if (now > this.last) {
      this.tokens = Math.min(this.cap, this.tokens + (now - this.last) / 1000 * this.rate);
      this.last = now;
    }
  }
  tryRemove(n = 1, now) { this._refill(now); if (this.tokens >= n) { this.tokens -= n; return true; } return false; }
  available(now) { this._refill(now); return this.tokens; }
}
module.exports = { RateLimiter };
