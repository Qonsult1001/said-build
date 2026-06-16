const assert = require('assert');
const { LRUCache } = require('../src/h1_lru.js');

// 1. Basic get/put + eviction order (the canonical LeetCode trace).
{
  const c = new LRUCache(2);
  c.put(1, 1); c.put(2, 2);
  assert.strictEqual(c.get(1), 1);          // 1 is now MRU
  c.put(3, 3);                               // evicts 2 (LRU)
  assert.strictEqual(c.get(2), -1);
  c.put(4, 4);                               // evicts 1 (LRU after 3,1 -> 1 oldest)
  assert.strictEqual(c.get(1), -1);
  assert.strictEqual(c.get(3), 3);
  assert.strictEqual(c.get(4), 4);
}
// 2. Update existing key counts as a use (must not evict it next).
{
  const c = new LRUCache(2);
  c.put(1, 1); c.put(2, 2);
  c.put(1, 10);            // update 1 -> MRU
  c.put(3, 3);             // evicts 2, not 1
  assert.strictEqual(c.get(2), -1);
  assert.strictEqual(c.get(1), 10);
  assert.strictEqual(c.get(3), 3);
}
// 3. Capacity 1.
{
  const c = new LRUCache(1);
  c.put(1, 1); assert.strictEqual(c.get(1), 1);
  c.put(2, 2); assert.strictEqual(c.get(1), -1); assert.strictEqual(c.get(2), 2);
}
// 4. get on missing key doesn't crash / doesn't insert.
{
  const c = new LRUCache(2);
  assert.strictEqual(c.get(99), -1);
  c.put(1,1); c.put(2,2); assert.strictEqual(c.get(99), -1);
  c.put(3,3); // still evicts 1 (99 never inserted)
  assert.strictEqual(c.get(1), -1);
  assert.strictEqual(c.get(2), 2);
}
// 5. Stress: interleaved access keeps correct LRU order over many ops.
{
  const c = new LRUCache(3);
  c.put(1,1); c.put(2,2); c.put(3,3);
  c.get(1);                 // order LRU->MRU: 2,3,1
  c.put(4,4);               // evict 2
  assert.strictEqual(c.get(2), -1);
  c.get(3);                 // order: 4,1,3
  c.put(5,5);               // evict 4
  assert.strictEqual(c.get(4), -1);
  assert.strictEqual(c.get(1), 1);
  assert.strictEqual(c.get(3), 3);
  assert.strictEqual(c.get(5), 5);
}
console.log('ok h1_lru'); process.exit(0);
