const assert = require('assert');
const { ms } = require('../src/ms.js');
// All values come straight from the context7 vercel/ms doc:
assert.strictEqual(ms('5s'), 5000);
assert.strictEqual(ms('10m'), 600000);
assert.strictEqual(ms('2h'), 7200000);
assert.strictEqual(ms('3d'), 259200000);
assert.strictEqual(ms('2w'), 1209600000);
assert.strictEqual(ms('1mo'), 2629800000);      // non-obvious: month constant
assert.strictEqual(ms('1y'), 31557600000);      // non-obvious: year constant
assert.strictEqual(ms('30 seconds'), 30000);
assert.strictEqual(ms('2 hrs'), 7200000);
assert.strictEqual(ms('100 msecs'), 100);
assert.strictEqual(ms('0.5h'), 1800000);
assert.strictEqual(ms('-1.5h'), -5400000);
assert.ok(Number.isNaN(ms('foo')));
console.log('ok ms'); process.exit(0);
