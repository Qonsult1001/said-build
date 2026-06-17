const assert = require('assert');
const { debounce } = require('../src/debounce.js');
(async () => {
  let calls=[]; const f=debounce((x)=>calls.push(x), 50);
  f(1); f(2); f(3);                          // only last (3) should fire, once
  await new Promise(r=>setTimeout(r,90));
  assert.deepStrictEqual(calls,[3]);
  f('a'); await new Promise(r=>setTimeout(r,30)); f('b'); // reset before 50ms
  await new Promise(r=>setTimeout(r,90));
  assert.deepStrictEqual(calls,[3,'b']);
  console.log('ok debounce'); process.exit(0);
})();
