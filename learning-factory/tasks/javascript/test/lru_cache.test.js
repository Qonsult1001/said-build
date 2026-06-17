const assert = require('assert');
const { LRUCache } = require('../src/lru_cache.js');
{ const c = new LRUCache(2); c.put(1,1); c.put(2,2);
  assert.strictEqual(c.get(1),1); c.put(3,3); assert.strictEqual(c.get(2),-1);
  c.put(4,4); assert.strictEqual(c.get(1),-1); assert.strictEqual(c.get(3),3); assert.strictEqual(c.get(4),4); }
{ const c = new LRUCache(2); c.put(1,1); c.put(2,2); c.put(1,10); c.put(3,3);
  assert.strictEqual(c.get(2),-1); assert.strictEqual(c.get(1),10); assert.strictEqual(c.get(3),3); }
{ const c = new LRUCache(1); c.put(1,1); assert.strictEqual(c.get(1),1);
  c.put(2,2); assert.strictEqual(c.get(1),-1); assert.strictEqual(c.get(2),2); }
{ const c = new LRUCache(2); assert.strictEqual(c.get(99),-1); c.put(1,1); c.put(2,2);
  c.put(3,3); assert.strictEqual(c.get(1),-1); assert.strictEqual(c.get(2),2); }
// Interleaved stress — forces correct recency tracking across get+put.
{ const c = new LRUCache(3); c.put(1,1); c.put(2,2); c.put(3,3); c.get(1);
  c.put(4,4); assert.strictEqual(c.get(2),-1); c.get(3); c.put(5,5);
  assert.strictEqual(c.get(4),-1); assert.strictEqual(c.get(1),1);
  assert.strictEqual(c.get(3),3); assert.strictEqual(c.get(5),5); }
console.log('ok lru_cache'); process.exit(0);
