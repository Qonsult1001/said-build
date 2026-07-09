#!/usr/bin/env node
// Answers the real question: at a LARGE brain (~42 blueprints), is the correct shape within top-K (so the
// LLM can pick it), or does it fall past the window? We harvest a big real repo (sca-core + the C# app +
// the bench examples), then for each KNOWN bench shape query, find the RANK of its correct blueprint
// among all returned candidates, and report recall@1 / @3 / @5. This is the recall_fix top-K contract
// tested at scale -- the honest measure, not rank@1 on 3 toys.
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');

// big harvest sources + the 3 bench shapes whose correct answer we KNOW.
const SOURCES = [
  path.join(ROOT, 'crates', 'sca-core', 'src'),
  path.join(ROOT, 'apps', 'OrchestrationFactory'),
  path.join(__dirname, 'examples'),
];
const CASES = [
  { query: 'record audit idempotency insert save response', want: 'create' },
  { query: 'http request message send async parse json',     want: 'lookup' },
  { query: 'parse args validate context exit code',          want: 'run' },
];
const K = 10; // pull a deep list so we can see the TRUE rank, then report @1/@3/@5

const bp = path.join(os.tmpdir(), `scalek_${process.pid}.said`);
for (const f of [bp, bp + '.spill']) { try { fs.unlinkSync(f); } catch {} }
execFileSync(SAID, ['create', bp]);
let total = 0;
for (const s of SOURCES) {
  if (!fs.existsSync(s)) continue;
  const out = execFileSync(SAID, ['--path', bp, 'harvest', s], { encoding: 'utf8' });
  const n = +((out.match(/Harvested (\d+)/) || [, 0])[1]); total += n;
}
console.log(`harvested ~${total} blueprints across ${SOURCES.length} real sources\n`);

const at = { 1: 0, 3: 0, 5: 0, miss: 0 };
for (const c of CASES) {
  const out = execFileSync(SAID, ['--path', bp, 'recall-blueprint', '--shape', c.query, '--min-similarity', '0.0', '--top-k', String(K)], { encoding: 'utf8' });
  // parse "[i] Blueprint (..) .. shape=<verb><Entity> .."
  const ranks = [...out.matchAll(/\[(\d+)\] Blueprint .* shape=(\S+)/g)].map(m => [+m[1], m[2]]);
  const hit = ranks.find(([, shape]) => shape.startsWith(c.want));
  const rank = hit ? hit[0] : 0;
  if (rank === 1) at[1]++, at[3]++, at[5]++;
  else if (rank > 0 && rank <= 3) at[3]++, at[5]++;
  else if (rank > 0 && rank <= 5) at[5]++;
  else at.miss++;
  console.log(`  want=${c.want.padEnd(7)} -> rank ${rank || 'MISS'} of ${ranks.length}  q="${c.query}"`);
}
for (const f of [bp, bp + '.spill']) { try { fs.unlinkSync(f); } catch {} }
const n = CASES.length;
console.log(`\nrecall@1=${at[1]}/${n}  recall@3=${at[3]}/${n}  recall@5=${at[5]}/${n}  miss=${at.miss}`);
console.log(`(@3 is the LLM-picks window. If correct shape is in @3, "let the LLM decide" works at scale.)`);
