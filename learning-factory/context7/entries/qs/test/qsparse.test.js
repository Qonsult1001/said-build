const assert=require('assert'); const {qsParse}=require('../src/qsparse.js');
assert.deepStrictEqual(qsParse('foo[bar][baz]=foobarbaz'), {foo:{bar:{baz:'foobarbaz'}}});
assert.deepStrictEqual(qsParse('a[]=b&a[]=c'), {a:['b','c']});
assert.deepStrictEqual(qsParse('a[1]=c&a[0]=b'), {a:['b','c']});   // sorted by index
assert.deepStrictEqual(qsParse('a[][b]=c'), {a:[{b:'c'}]});
assert.deepStrictEqual(qsParse('x=1&y=2'), {x:'1',y:'2'});
console.log('ok qs'); process.exit(0);
