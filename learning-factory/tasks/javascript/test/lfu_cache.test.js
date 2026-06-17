const assert = require('assert');
const { LFUCache } = require('../src/lfu_cache.js');
{ const c = new LFUCache(2); c.put(1,1); c.put(2,2);
  assert.strictEqual(c.get(1),1);          // freq: 1->2, 2->1
  c.put(3,3);                               // evict 2 (lowest freq)
  assert.strictEqual(c.get(2),-1);
  assert.strictEqual(c.get(3),3);           // freq: 3->2
  c.put(4,4);                               // 1 and 3 both freq 2; evict LRU among them = 1
  assert.strictEqual(c.get(1),-1);
  assert.strictEqual(c.get(3),3); assert.strictEqual(c.get(4),4); }
{ const c = new LFUCache(0); c.put(1,1); assert.strictEqual(c.get(1),-1); }  // zero capacity
{ const c = new LFUCache(2); c.put(1,1); c.put(2,2); c.put(1,11); // update counts as a use
  assert.strictEqual(c.get(1),11); c.put(3,3); assert.strictEqual(c.get(2),-1); }
console.log('ok lfu_cache'); process.exit(0);
