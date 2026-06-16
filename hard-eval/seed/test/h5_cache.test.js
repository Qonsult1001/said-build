const assert = require('assert');
const { RecentStore } = require('../src/h5_cache.js');
{ const c=new RecentStore(2); c.store(1,1); c.store(2,2);
  assert.strictEqual(c.fetch(1),1); c.store(3,3);
  assert.strictEqual(c.fetch(2),null); c.store(4,4);
  assert.strictEqual(c.fetch(1),null); assert.strictEqual(c.fetch(3),3); assert.strictEqual(c.fetch(4),4); }
{ const c=new RecentStore(2); c.store(1,1); c.store(2,2); c.store(1,10); c.store(3,3);
  assert.strictEqual(c.fetch(2),null); assert.strictEqual(c.fetch(1),10); }
{ const c=new RecentStore(1); c.store(1,1); assert.strictEqual(c.fetch(1),1);
  c.store(2,2); assert.strictEqual(c.fetch(1),null); assert.strictEqual(c.fetch(2),2); }
{ const c=new RecentStore(2); assert.strictEqual(c.fetch(99),null); }
console.log('ok h5_cache'); process.exit(0);
