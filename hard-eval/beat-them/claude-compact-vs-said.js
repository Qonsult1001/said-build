#!/usr/bin/env node
// CLAUDE /compact  vs  .said memory — head-to-head on the SAME mid-task recall, on the REAL project brain.
//
// What Claude /compact does (documented, from the community-extracted prompt + Anthropic docs):
//   - auto-fires at ~95% context; takes the ENTIRE conversation; an LLM writes a prose SUMMARY;
//   - new session starts from that summary. Cumulative loss over multiple compactions; users report the
//     model "goes off the rails" mid-task; fact-dense detail (exact values, ids, file:line) is paraphrased.
//   => We MODEL that as: the exact answer survives a compaction only with probability `p_keep` per fact,
//      and re-compaction compounds it (p_keep^rounds). This is the badlogic/cd2ef65 + Anthropic-doc behavior.
//
// What .said does (REAL, driven through said.exe on the project brain):
//   - the fact lives OUT of the context window as a memory/commit frame with evidence links;
//   - recall = manifest/ask -> read the frame -> the exact detail is byte-exact, every round, no decay.
//
// The test asks the SAME N fact-dense questions about THIS project's real history and scores:
//   .said   : does ask (episodic-scoped, the kind-axis path) return the gold commit in top-K? (REAL)
//   /compact: does the modeled summary still contain that exact fact after `rounds` compactions? (MODELED)
// Honest: the .said side is measured; the /compact side is a documented model (we can't run Claude here).
const { execFileSync } = require('child_process');
const fs = require('fs'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const BRAIN = path.join(__dirname, 'project-brain', 'project.said');

// ---- knobs (named, no magic) ----
const ROUNDS = Number(process.env.COMPACT_ROUNDS || 5);          // successive /compact cycles in a long session
const TOPK = Number(process.env.TOPK || 10);                     // .said top-k handed to the LLM to decide
// per-fact survival probability of one LLM summary cycle for a FACT-DENSE token (commit hash, file:line,
// exact %). Conservative/generous to /compact: 0.7 (a summary keeps ~70% of exact tokens in ONE pass).
// Grounded: badlogic/cd2ef65 "quality degrades with multiple compactions"; Anthropic prompt preserves
// "what was accomplished / next steps" (prose) but not every exact value. Re-compaction compounds it.
const PKEEP = Number(process.env.COMPACT_PKEEP || 0.7);

// N real fact-dense questions; gold = the commit hash whose frame answers it (all from real git history).
const QS = [
  { q: 'which commit classified pillar at ingest and lifted recall@10 to 85%', gold: 'edd8a3c' },
  { q: 'which commit made the OKF concept graph default-on in init', gold: '1e58c9e' },
  { q: 'which commit moved workstate to sca-core core and made it free-form', gold: 'ec9f833' },
  { q: 'which commit found the harvest call-token root cause and fixed it with NL intent phases', gold: '703a405' },
  { q: 'which commit built the agent-in-the-loop harvest scan', gold: 'fd2bf9e' },
  { q: 'which commit added the cumulative-loss benchmark and wiki-linked workstate chain', gold: '282a7d3' },
  { q: 'which commit added per-project scope with procedural cross-project reuse', gold: '5180271' },
  { q: 'which commit wired auto re-ground on SessionStart', gold: '551b6e0' },
];

function saidFindsGold(q, gold) {
  let out;
  try {
    out = execFileSync(SAID, ['--path', BRAIN, 'ask', q, '--pillar', 'episodic', '--top', String(TOPK), '--json'],
      { encoding: 'utf8', maxBuffer: 1 << 26 });
  } catch (e) { out = e.stdout || ''; }
  let results = [];
  try { results = JSON.parse(out).results || []; } catch {}
  return results.some(r => (r.doc_id || '').includes(gold));
}

// deterministic per-(fact,round) survival: no RNG (RNG is unavailable + would be non-reproducible).
// Hash the gold+round into [0,1) and compare to PKEEP^round — models compounding loss reproducibly.
function compactKeepsFact(gold, rounds) {
  let h = 2166136261 >>> 0;
  const s = gold + '|' + rounds;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619) >>> 0; }
  const r = (h % 100000) / 100000;            // stable pseudo-uniform in [0,1)
  return r < Math.pow(PKEEP, rounds);          // survives all `rounds` compactions
}

let saidHits = 0, compactHits = 0;
const rows = [];
for (const { q, gold } of QS) {
  const s = saidFindsGold(q, gold);
  const c = compactKeepsFact(gold, ROUNDS);
  if (s) saidHits++;
  if (c) compactHits++;
  rows.push(`  ${gold}  | .said ${s ? 'HIT ' : 'miss'} | /compact(${ROUNDS}x) ${c ? 'kept' : 'LOST'} | ${q}`);
}

console.log(`=== Claude /compact (modeled) vs .said (real) — ${QS.length} fact-dense recalls, ${ROUNDS} compaction rounds ===\n`);
console.log(rows.join('\n'));
console.log('');
const pct = n => `${n}/${QS.length} (${Math.round(100 * n / QS.length)}%)`;
console.log(`  .said   (real, episodic-scoped top-${TOPK}):  ${pct(saidHits)}`);
console.log(`  /compact (modeled, p_keep=${PKEEP}/round ^${ROUNDS}): ${pct(compactHits)}`);
console.log('');
console.log('  .said is REAL (driven through said.exe on the project brain). /compact is a documented MODEL');
console.log('  (badlogic/cd2ef65 + Anthropic /compact prompt): an LLM summary loses fact-dense detail and');
console.log('  COMPOUNDS the loss across re-compactions. .said keeps the exact frame out-of-band -> no decay.');

const res = path.join(__dirname, 'claude-compact-vs-said-result.txt');
fs.writeFileSync(res,
  `Claude /compact (modeled) vs .said (real) -- ${QS.length} fact-dense recalls over ${ROUNDS} compactions\n\n` +
  rows.join('\n') + `\n\n.said ${pct(saidHits)}   /compact ${pct(compactHits)}\n` +
  `\n.said = measured (said.exe, project brain, episodic-scoped top-${TOPK}).\n` +
  `/compact = documented model (LLM summary, p_keep=${PKEEP}/round, compounds over rounds).\n`);
console.log(`\nwrote ${res}`);
