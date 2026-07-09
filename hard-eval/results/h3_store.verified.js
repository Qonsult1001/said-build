class Store {
  constructor() { this.map = new Map(); this._clock = () => Date.now(); }
  setClock(fn) { this._clock = fn; }
  now() { return this._clock(); }
  _live(k) {
    if (!this.map.has(k)) return false;
    const e = this.map.get(k);
    if (e.exp !== null && this.now() >= e.exp) { this.map.delete(k); return false; }
    return true;
  }
  set(k, v, ttlMs) { this.map.set(k, { v, exp: (ttlMs === undefined || ttlMs === null) ? null : this.now() + ttlMs }); }
  get(k) { return this._live(k) ? this.map.get(k).v : undefined; }
  has(k) { return this._live(k); }
  del(k) { return this.map.delete(k); }
  size() { for (const k of [...this.map.keys()]) this._live(k); return this.map.size; }
}
module.exports = { Store };
