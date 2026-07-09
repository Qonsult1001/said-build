#!/usr/bin/env node
// Recall-ranking probe: the FAILING case isolated so whitening's effect is MEASURED, not guessed.
// Harvest the 3 bench examples, then for each shape query, check whether the #1 recalled blueprint is
// the CORRECT one. Run this BEFORE and AFTER Soft-ZCA whitening (env SAID_WHITEN=1) to A/B the fix.
//   node recall-ranking-probe.js              # baseline
//   SAID_WHITEN=1 node recall-ranking-probe.js   # with whitening (once implemented)
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const EXAMPLES = path.join(__dirname, 'examples');

// each query + the shape-verb whose blueprint SHOULD rank #1 for it.
const CASES = [
  { query: 'record audit idempotency insert save response', want: 'create' },
  { query: 'http request message send async parse json',     want: 'lookup' },
  { query: 'parse args validate context exit code',          want: 'run' },
];

const bp = path.join(os.tmpdir(), `probe_${process.pid}.said`);
for (const f of [bp, bp + '.spill']) { try { fs.unlinkSync(f); } catch {} }
execFileSync(SAID, ['create', bp]);
execFileSync(SAID, ['--path', bp, 'harvest', EXAMPLES]);

let pass = 0;
console.log(`whitening: ${process.env.SAID_WHITEN ? 'ON' : 'off'}`);
for (const c of CASES) {
  const out = execFileSync(SAID, ['--path', bp, 'recall-blueprint', '--shape', c.query, '--min-similarity', '0.0'], { encoding: 'utf8' });
  const shape = (out.match(/shape=(\S+)/) || [, '?'])[1];
  const ok = shape.startsWith(c.want);
  if (ok) pass++;
  console.log(`  ${ok ? 'PASS' : 'FAIL'}  want=${c.want.padEnd(7)} got=${shape.padEnd(10)}  q="${c.query}"`);
}
for (const f of [bp, bp + '.spill']) { try { fs.unlinkSync(f); } catch {} }
console.log(`\n${pass}/${CASES.length} queries recalled the correct shape at rank #1`);
process.exit(pass === CASES.length ? 0 : 1);
