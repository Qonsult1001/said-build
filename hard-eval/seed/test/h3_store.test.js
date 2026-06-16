const assert = require('assert');
const { Store } = require('../src/h3_store.js');
const { makeSession } = require('../src/h3_index.js');

// Deterministic clock.
let T = 1000;
function freshStore() { const s = new Store(); s.setClock(() => T); return s; }

// 1. Existing non-TTL behaviour must still work.
{
  T = 1000;
  const s = freshStore();
  s.set('a', 1); s.set('b', 2);
  assert.strictEqual(s.get('a'), 1);
  assert.strictEqual(s.has('b'), true);
  assert.strictEqual(s.size(), 2);
  assert.strictEqual(s.del('a'), true);
  assert.strictEqual(s.has('a'), false);
  assert.strictEqual(s.size(), 1);
}
// 2. TTL: key present before expiry, absent after.
{
  T = 1000;
  const s = freshStore();
  s.set('k', 'v', 100);          // expires at 1100
  T = 1099; assert.strictEqual(s.get('k'), 'v', 'alive just before expiry');
  assert.strictEqual(s.has('k'), true);
  T = 1100; assert.strictEqual(s.get('k'), undefined, 'expired AT ttl boundary');
  assert.strictEqual(s.has('k'), false);
}
// 3. Expired keys do not count toward size; non-TTL keys persist.
{
  T = 1000;
  const s = freshStore();
  s.set('perm', 1);              // no ttl -> never expires
  s.set('temp', 2, 50);         // expires at 1050
  assert.strictEqual(s.size(), 2);
  T = 1051;
  assert.strictEqual(s.size(), 1, 'expired key drops from size');
  assert.strictEqual(s.get('perm'), 1, 'permanent key survives');
}
// 4. Re-set with a new ttl refreshes expiry.
{
  T = 1000;
  const s = freshStore();
  s.set('k', 'a', 100);          // expires 1100
  T = 1050; s.set('k', 'b', 100); // refresh -> expires 1150
  T = 1120; assert.strictEqual(s.get('k'), 'b', 'refreshed ttl keeps it alive');
  T = 1150; assert.strictEqual(s.get('k'), undefined);
}
// 5. Integration via the existing consumer (multi-file).
{
  T = 1000;
  const sess = makeSession();
  sess._store.setClock(() => T);
  sess.login('alice'); assert.strictEqual(sess.current(), 'alice');
  sess.setToken('xyz', 30);     // token expires at 1030
  T = 1029; assert.strictEqual(sess.token(), 'xyz');
  T = 1030; assert.strictEqual(sess.token(), undefined, 'token expired');
  assert.strictEqual(sess.current(), 'alice', 'login (no ttl) still valid');
}
console.log('ok h3_store'); process.exit(0);
