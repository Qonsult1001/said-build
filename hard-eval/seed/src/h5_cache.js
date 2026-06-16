// TASK h5 (LRU variant — DIFFERENT names/file, SAME underlying shape).
// new RecentStore(limit): a cache that keeps the `limit` most-recently-used keys.
//   fetch(key)        -> value, or null if absent. fetch COUNTS as recent use.
//   store(key,value)  -> insert/update; update counts as use; evict least-recent
//                        when over limit.
// O(1) operations expected.
// TODO: implement (stub throws).
class RecentStore {
  constructor(limit) { throw new Error('not implemented'); }
  fetch(key) { throw new Error('not implemented'); }
  store(key, value) { throw new Error('not implemented'); }
}
module.exports = { RecentStore };
