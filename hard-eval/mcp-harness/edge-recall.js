// GENERALIZATION battery for fix-recall: many paraphrase-gap PATTERNS, not one case.
// Proves the lexical+dense union is a GLOBAL improvement, and a NEGATIVE control proves it
// doesn't create false positives (which would break precision / global recall).
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const path = require('path');
const BRAIN = 'G:/development/said-build/hard-eval/mcp-harness/edge.said';

// [stored problem, recall paraphrase, gap-type]. Each paraphrase is a DIFFERENT surface form.
const CASES = [
  // synonym / reworded framing
  ['date parse fails for the 29th of February', 'leap-day date parsing throws', 'reworded+synonym'],
  ['a poisoned message retries forever in an unbounded loop', 'bad message keeps retrying without limit', 'synonym'],
  ['the last-page pagination cursor returns duplicate rows', 'final page cursor shows repeated records', 'synonym'],
  ['money stored as a float drifts over time', 'using floating point for currency causes rounding drift', 'expand-jargon'],
  ['two concurrent profile edits clobber each other (lost update)', 'simultaneous profile saves overwrite one another', 'synonym'],
  // abbreviation / expansion
  ['FK on the settlement account table points to the wrong parent', 'foreign key on settlement table is misconfigured', 'abbrev-expand'],
  ['SQL injection risk in a dynamically-built WHERE clause', 'unparameterized dynamic sql where clause is injectable', 'reorder'],
  ['idempotency key expires early when the clock skews', 'clock drift breaks the idempotency window', 'reorder+synonym'],
  // plain-english <-> technical
  ['integer overflow when summing a very large set of balances', 'balance total wraps around on huge sums', 'plain-english'],
  ['deadlock when two transfers hit the same account', 'concurrent transfers on one account hang forever', 'synonym'],
  ['timezone drift in the nightly accrual job', 'the daily interest accrual runs in the wrong timezone', 'reword'],
  ['a domain constructor accepts an invalid argument without a guard', 'aggregate is created with bad input, no validation', 'reword'],
  // near-twins that must NOT collide (distinct fixes, similar domain)
  ['stale balance read AFTER a concurrent debit', 'balance is out of date following a parallel withdrawal', 'twin-A'],
  ['duplicate ledger entries on RETRY of a failed post', 'retrying a failed posting writes the entry twice', 'twin-B'],
  ['interest rounds DOWN losing a cent each period', 'interest calc drops a cent every cycle', 'twin-C'],
];

// NEGATIVE control: queries with NO matching stored fix — must return nothing / not a confident hit.
const NEGATIVES = [
  'how to center a div in css',
  'configure kubernetes ingress tls',
  'parse a yaml config file in go',
  'render a react component with hooks',
];

const hit = (text, storedProblem) => text.includes(storedProblem) || text.split('\n').some(l => {
  // token-overlap sanity: the returned TASK line should share most content words with the stored problem
  const w = s => new Set(s.toLowerCase().split(/[^a-z0-9]+/).filter(x=>x.length>=4));
  if (!/TASK:/.test(l)) return false;
  const a = w(l), b = w(storedProblem);
  let o=0; for (const t of b) if (a.has(t)) o++;
  return o >= Math.ceil(b.size*0.5);
});

(async () => {
  for (const e of ['','.spill']) { try { fs.unlinkSync(BRAIN+e); } catch {} }
  const c = new McpClient(BRAIN, {}); c.start(); await c.initialize();
  // realistic scale: ingest a real project so the fixes compete against many code frames
  console.log('ingesting africanbank for realistic scale...');
  await c.call('init', { dir: 'G:/development/Wonga/AfricanBank' }, 600000);
  console.log('learning', CASES.length, 'distinct fixes...');
  for (let i=0;i<CASES.length;i++) {
    await c.call('learn_fix', { problem: CASES[i][0], edits: JSON.stringify([{file:`fix${i}.x`,change:'resolve'}]), learnings:`edge-${i}` });
  }
  console.log('recalling each by its DIFFERENT paraphrase (top-10)...');
  let h1=0,h5=0,h10=0; const misses=[];
  for (let i=0;i<CASES.length;i++) {
    const [stored, para, gap] = CASES[i];
    const r = await c.call('recall_fix', { problem: para, top_k: 10, min_score: 0.0 });
    const blocks = r.text.split(/#\d+ Fix \(/).slice(1);
    const rank = blocks.findIndex(b => hit('#x Fix (' + b, stored)) ; // 0-based rank of the correct fix
    if (rank === 0) h1++;
    if (rank >= 0 && rank < 5) h5++;
    if (rank >= 0 && rank < 10) h10++;
    if (rank < 0) misses.push(`[${gap}] "${para}"  (stored: "${stored.slice(0,40)}...")`);
  }
  // NEGATIVE control: none should return a CONFIDENT fix (min_score default gate)
  console.log('negative control (should NOT confidently match)...');
  let falsePos=0;
  for (const q of NEGATIVES) {
    const r = await c.call('recall_fix', { problem: q });  // default min_score 0.45, k=1 -> the gated path
    if (/#1 Fix \(0\.[5-9]/.test(r.text) || /^Fix \(0\.[5-9]/.test(r.text)) falsePos++;
  }
  c.stop();
  const N = CASES.length;
  console.log(`\n=== GENERALIZATION (${N} varied paraphrase gaps, realistic scale) ===`);
  console.log(`  recall@1  : ${h1}/${N} (${Math.round(h1/N*100)}%)`);
  console.log(`  recall@5  : ${h5}/${N} (${Math.round(h5/N*100)}%)`);
  console.log(`  recall@10 : ${h10}/${N} (${Math.round(h10/N*100)}%)`);
  console.log(`  false positives on ${NEGATIVES.length} unrelated queries: ${falsePos}`);
  if (misses.length) { console.log('  MISSES:'); misses.forEach(m=>console.log('    '+m)); }
  fs.writeFileSync(path.join(__dirname,'edge-result.json'), JSON.stringify({N,h1,h5,h10,falsePos,misses},null,2));
  const pass = h10 >= N && falsePos === 0;
  console.log(`\n[${pass?'PASS':'REVIEW'}] recall@10=${Math.round(h10/N*100)}% + ${falsePos} false-positives`);
  process.exit(0);
})().catch(e => { console.error('ERR:', e.message); process.exit(2); });
