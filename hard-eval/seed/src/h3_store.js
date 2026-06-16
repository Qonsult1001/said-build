// TASK h3 (feature in existing multi-file code): an in-memory KV store.
// EXISTING API (do not break it): set(k,v), get(k), has(k), del(k), size().
// FEATURE TO ADD: optional TTL. set(k, v, ttlMs) — when ttlMs is given, the key
// EXPIRES ttlMs after it was set. Expired keys behave as absent for get/has/size
// and are cleaned up lazily on access. A `now()` injection point is provided so
// the gate can control time deterministically — use this.now(), not Date.now().
//
// The TTL feature is NOT implemented yet (set ignores ttlMs; nothing expires).
// Implement it WITHOUT breaking the existing non-TTL behaviour.
class Store {
  constructor() {
    this.map = new Map();
    this._clock = () => Date.now();
  }
  // Test injects a fake clock via setClock; production uses Date.now.
  setClock(fn) { this._clock = fn; }
  now() { return this._clock(); }

  set(k, v, ttlMs) {
    // TODO: honor ttlMs (expiry). For now it just stores the value.
    this.map.set(k, v);
  }
  get(k) {
    // TODO: treat expired keys as absent (return undefined) + lazily delete.
    return this.map.get(k);
  }
  has(k) {
    // TODO: expired keys are not present.
    return this.map.has(k);
  }
  del(k) { return this.map.delete(k); }
  size() {
    // TODO: expired keys must not count.
    return this.map.size;
  }
}
module.exports = { Store };
