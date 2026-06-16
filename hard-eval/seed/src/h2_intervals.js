// TASK h2 (bug fix): mergeIntervals(intervals) merges all overlapping intervals.
// intervals: array of [start, end] (end >= start). Returns merged, sorted by start.
// Two intervals overlap if they touch or cross (e.g. [1,4] and [4,5] -> [1,5]).
//
// This implementation HAS BUGS — the test suite below fails. Fix it so the gate
// passes. (Do not rewrite the whole file unless needed; find and fix the bugs.)
function mergeIntervals(intervals) {
  const out = [];
  // BUG: does not sort first, so unordered input merges incorrectly.
  for (const iv of intervals) {
    if (out.length === 0) {
      out.push(iv.slice());
      continue;
    }
    const last = out[out.length - 1];
    // BUG: strict '<' misses touching intervals like [1,4],[4,5].
    if (iv[0] < last[1]) {
      last[1] = Math.max(last[1], iv[1]);
    } else {
      out.push(iv.slice());
    }
  }
  return out;
}
module.exports = { mergeIntervals };
