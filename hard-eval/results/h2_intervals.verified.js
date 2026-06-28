function mergeIntervals(intervals) {
  const sorted = intervals.map(iv => iv.slice()).sort((a, b) => a[0] - b[0]);
  const out = [];
  for (const iv of sorted) {
    const last = out[out.length - 1];
    if (last && iv[0] <= last[1]) last[1] = Math.max(last[1], iv[1]);
    else out.push(iv.slice());
  }
  return out;
}
module.exports = { mergeIntervals };
