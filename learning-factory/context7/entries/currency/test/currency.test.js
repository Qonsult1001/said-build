const assert=require('assert'); const {add,subtract,multiply,distribute}=require('../src/currency.js');
assert.strictEqual(add(123.50,0.23), 123.73);
assert.strictEqual(subtract(5.00,0.50), 4.50);
assert.strictEqual(multiply(45.25,3), 135.75);
assert.deepStrictEqual(distribute(1.12,5), [0.23,0.23,0.22,0.22,0.22]); // remainder to the front
assert.strictEqual(add(2.51,0.01), 2.52);  // no float error
console.log('ok currency'); process.exit(0);
