const assert=require('assert'); const {uuidValidate,uuidVersion,isV4}=require('../src/uuid.js');
assert.strictEqual(uuidValidate('9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d'), true);
assert.strictEqual(uuidValidate('not-a-uuid'), false);
assert.strictEqual(uuidValidate('9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6dZZ'), false);
assert.strictEqual(uuidVersion('9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d'), 4);
assert.strictEqual(uuidVersion('c232ab00-9414-11ec-b3c8-9e6bdeced846'), 1);
assert.strictEqual(isV4('9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d'), true);
assert.strictEqual(isV4('c232ab00-9414-11ec-b3c8-9e6bdeced846'), false);
console.log('ok uuid'); process.exit(0);
