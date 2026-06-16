const assert = require('assert');
const { mergeIntervals } = require('../src/h2_intervals.js');
const eq = (a, b, m) => assert.deepStrictEqual(a, b, m);

// 1. Classic overlap.
eq(mergeIntervals([[1,3],[2,6],[8,10],[15,18]]), [[1,6],[8,10],[15,18]]);
// 2. Touching intervals merge ([1,4]+[4,5] -> [1,5]) — the '<' vs '<=' bug.
eq(mergeIntervals([[1,4],[4,5]]), [[1,5]]);
// 3. UNSORTED input must still merge correctly — the missing-sort bug.
eq(mergeIntervals([[2,6],[1,3],[15,18],[8,10]]), [[1,6],[8,10],[15,18]]);
// 4. Fully nested interval.
eq(mergeIntervals([[1,10],[2,3],[4,5]]), [[1,10]]);
// 5. Single + empty.
eq(mergeIntervals([[5,7]]), [[5,7]]);
eq(mergeIntervals([]), []);
// 6. Adjacent-but-not-overlapping stays separate ([1,2],[3,4]).
eq(mergeIntervals([[1,2],[3,4]]), [[1,2],[3,4]]);
// 7. Does NOT mutate the input arrays.
const input = [[1,3],[2,6]];
mergeIntervals(input);
eq(input, [[1,3],[2,6]], 'input must not be mutated');
console.log('ok h2_intervals'); process.exit(0);
