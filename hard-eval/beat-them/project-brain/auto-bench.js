#!/usr/bin/env node
// HONEST coverage benchmark on the REAL project brain (4689 memories: 333 commits + 169 docs + code).
// Ground truth is generated DIRECTLY from the real git history -- neither hand-picked nor pre-loaded.
// For each of N real commits we form a natural question from its subject and check whether .said returns
// THAT commit's memory (doc_id = <hash>) in the top-K when asked. This measures real recall-on-the-fly:
// can it find the right real fact among thousands? Reports recall@1/@3/@10 + the exact misses.
const { execFileSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const ROOT = path.join(__dirname, '..', '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const BRAIN = path.join(__dirname, 'project.said');

// Pull N commits straight from git: hash + subject. The question is derived from the subject (the real
// words the developer wrote); the gold doc_id is the commit hash (its memory file was <hash>.txt -> id).
const SAMPLE = Number(process.env.BENCH_N || 40);
function gitCommits(n) {
  const out = execFileSync('git', ['-C', ROOT, 'log', '--format=%H%x09%s', '-n', String(n * 3)], { encoding: 'utf8' });
  return out.trim().split('\n').map(l => { const [h, ...s] = l.split('\t'); return { hash: h, subject: s.join('\t') }; });
}
// Turn a commit subject into a recall question: strip the "area(scope):" prefix + noise, keep the meaning.
function toQuestion(subject) {
  let q = subject.replace(/^[a-z0-9()\-]+:\s*/i, '');         // drop "blueprint(harvest): "
  q = q.replace(/--/g, ' ').replace(/\s+/g, ' ').trim();
  return `which commit: ${q}`;
}
// .said stores dir-ingested files with a doc_id derived from the filename (<hash>). Find the gold rank.
const PILLAR = process.env.BENCH_PILLAR; // e.g. "episodic" to scope to commits (kind-axis fix)
function rankOfCommit(q, hash, k) {
  let out;
  const args = ['--path', BRAIN, 'ask', q, '--json'];
  if (PILLAR) args.push('--pillar', PILLAR);
  try { out = execFileSync(SAID, args, { encoding: 'utf8', maxBuffer: 1 << 26 }); }
  catch (e) { out = e.stdout || ''; }
  let results = [];
  try { results = (JSON.parse(out).results || []); } catch { return { rank: 0, n: 0 }; }
  const short = hash.slice(0, 8);
  const idx = results.findIndex(r => (r.doc_id || '').includes(short) || (r.doc_id || '').includes(hash));
  return { rank: idx >= 0 ? idx + 1 : 0, n: results.length };
}

const K = 10;
const commits = gitCommits(SAMPLE).slice(0, SAMPLE);
const at = { 1: 0, 3: 0, 10: 0, miss: 0 };
const misses = [];
console.log(`=== AUTO coverage benchmark: ${commits.length} real commits, recall on the fly ===\n`);
for (const c of commits) {
  const q = toQuestion(c.subject);
  const { rank } = rankOfCommit(q, c.hash, K);
  if (rank === 1) { at[1]++; at[3]++; at[10]++; }
  else if (rank && rank <= 3) { at[3]++; at[10]++; }
  else if (rank && rank <= 10) at[10]++;
  else { at.miss++; misses.push({ hash: c.hash.slice(0, 8), q, rank }); }
}
const n = commits.length;
const pct = x => `${x}/${n} (${Math.round(100 * x / n)}%)`;
console.log(`  recall@1:  ${pct(at[1])}`);
console.log(`  recall@3:  ${pct(at[3])}`);
console.log(`  recall@10: ${pct(at[10])}`);
console.log(`  missed:    ${pct(at.miss)} (gold commit not in top ${K})`);
if (misses.length) {
  console.log(`\n  --- misses (the real ranking weakness) ---`);
  for (const m of misses.slice(0, 20)) console.log(`   [${m.hash}] ${m.q}`);
}
fs.writeFileSync(path.join(__dirname, 'auto-bench-result.txt'),
  `AUTO coverage benchmark on real project brain (${n} commits)\n\n` +
  `recall@1:  ${pct(at[1])}\nrecall@3:  ${pct(at[3])}\nrecall@10: ${pct(at[10])}\nmissed:    ${pct(at.miss)}\n` +
  (misses.length ? `\nMisses:\n${misses.map(m => `[${m.hash}] ${m.q}`).join('\n')}\n` : ''));
console.log(`\nwrote auto-bench-result.txt`);
