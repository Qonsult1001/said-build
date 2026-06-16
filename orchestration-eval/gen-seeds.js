// Generates the 12 seed src stubs + test gates. Run: node gen-seeds.js
const fs = require('fs');
const path = require('path');
const ROOT = __dirname;
const REPO = path.join(ROOT, 'repo');

function stub(fn) {
  return `// TODO: implement ${fn} to make the test pass.\nfunction ${fn}() {\n  throw new Error('not implemented');\n}\nmodule.exports = { ${fn} };\n`;
}
function testFile(id, fn, body) {
  return `const assert = require('assert');\nconst { ${fn} } = require('../src/${id}.js');\n${body}\nconsole.log('ok ${id}'); process.exit(0);\n`;
}

const tests = {
  r1: testFile('r1','add', `assert.strictEqual(add(2,3),5); assert.strictEqual(add(-1,1),0); assert.strictEqual(add(0,0),0);`),
  r2: testFile('r2','max', `assert.strictEqual(max([1,5,3]),5); assert.strictEqual(max([-2,-9,-1]),-1); assert.strictEqual(max([42]),42);`),
  r3: testFile('r3','reverse', `assert.strictEqual(reverse('abc'),'cba'); assert.strictEqual(reverse(''),''); assert.strictEqual(reverse('a'),'a');`),
  r4: testFile('r4','isEven', `assert.strictEqual(isEven(4),true); assert.strictEqual(isEven(7),false); assert.strictEqual(isEven(0),true);`),
  r5: testFile('r5','sum', `assert.strictEqual(sum([1,2,3]),6); assert.strictEqual(sum([]),0); assert.strictEqual(sum([5]),5);`),
  r6: testFile('r6','countVowels', `assert.strictEqual(countVowels('hello'),2); assert.strictEqual(countVowels('XYZ'),0); assert.strictEqual(countVowels('AeIoU'),5);`),
  n1: testFile('n1','fib', `assert.strictEqual(fib(0),0); assert.strictEqual(fib(1),1); assert.strictEqual(fib(10),55);`),
  n2: testFile('n2','isPalindrome', `assert.strictEqual(isPalindrome('A man, a plan, a canal: Panama'),true); assert.strictEqual(isPalindrome('race a car'),false); assert.strictEqual(isPalindrome(''),true);`),
  n3: testFile('n3','romanToInt', `assert.strictEqual(romanToInt('III'),3); assert.strictEqual(romanToInt('IV'),4); assert.strictEqual(romanToInt('MCMXCIV'),1994);`),
  n4: testFile('n4','groupBy', `const g=groupBy([1,2,3,4], x=>x%2===0?'even':'odd'); assert.deepStrictEqual(g,{odd:[1,3],even:[2,4]});`),
  n5: testFile('n5','debounceCount', `assert.strictEqual(debounceCount([{t:0},{t:50},{t:100},{t:500}],100),2); assert.strictEqual(debounceCount([],100),0); assert.strictEqual(debounceCount([{t:0}],100),1);`),
  n6: testFile('n6','parseQuery', `assert.deepStrictEqual(parseQuery('a=1&b=two&a=3'),{a:['1','3'],b:'two'}); assert.deepStrictEqual(parseQuery('?x=9'),{x:'9'}); assert.deepStrictEqual(parseQuery(''),{});`),
};

const fns = { r1:'add', r2:'max', r3:'reverse', r4:'isEven', r5:'sum', r6:'countVowels',
  n1:'fib', n2:'isPalindrome', n3:'romanToInt', n4:'groupBy', n5:'debounceCount', n6:'parseQuery' };

for (const id of Object.keys(fns)) {
  fs.writeFileSync(path.join(REPO, 'src', id + '.js'), stub(fns[id]));
  fs.writeFileSync(path.join(REPO, 'test', id + '.test.js'), tests[id]);
}
console.log('wrote 12 stubs + 12 tests');
