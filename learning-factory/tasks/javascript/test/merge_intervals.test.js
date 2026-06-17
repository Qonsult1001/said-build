const assert = require('assert');
const { mergeIntervals } = require('../src/merge_intervals.js');
const eq=(a,b)=>assert.strictEqual(JSON.stringify(a),JSON.stringify(b));
eq(mergeIntervals([[1,3],[2,6],[8,10],[15,18]]), [[1,6],[8,10],[15,18]]);
eq(mergeIntervals([[1,4],[4,5]]), [[1,5]]);           // TOUCHING merges (<=)
eq(mergeIntervals([[1,4],[0,4]]), [[0,4]]);            // unsorted input
eq(mergeIntervals([[1,4],[2,3]]), [[1,4]]);            // contained
// must NOT mutate input
const input=[[3,4],[1,2]]; const copy=JSON.parse(JSON.stringify(input));
mergeIntervals(input); eq(input, copy);
console.log('ok merge_intervals'); process.exit(0);
