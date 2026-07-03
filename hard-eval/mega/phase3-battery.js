// PHASE 3: arXiv LongMemEval-style battery (5 categories) on the 46.6k-frame maximal brain, via MCP.
// Categories (from arxiv-research.md): info-extract, multi-session, temporal, knowledge-update, abstention.
// Measures recall@1/@5/@10 per category + abstention accuracy. Gates vs Mem0 (LoCoMo 92.5 / LMEval 94.4).
'use strict';
const { McpClient } = require('../mcp-harness/mcp-client');
const fs = require('fs');
const path = require('path');
const BRAIN = 'G:/development/said-build/hard-eval/mega/mega.said';

// Each: [category, store-content, recall-query, expected-token-in-answer]. Planted as memories, then
// recalled by a DIFFERENT-worded query, into a brain already holding 46.6k frames (realistic noise).
const ITEMS = [
  // INFORMATION EXTRACTION (single-hop)
  ['IE', 'The prod database master password is stored in vault path secret/db/master-cred-7788', 'where is the production database master password kept', 'secret/db/master-cred-7788'],
  ['IE', 'The billing cron job runs every night at 02:15 UTC on host bill-worker-3', 'when does the nightly billing job run and on which host', 'bill-worker-3'],
  ['IE', 'Our Stripe webhook signing secret rotates on the first Monday of each quarter', 'how often does the stripe webhook secret rotate', 'first Monday'],
  // MULTI-SESSION (compose across items)
  ['MS', 'Session A: the payment service owns the ledger. Session B: the ledger uses double-entry posting', 'which service owns the double-entry ledger', 'payment service'],
  ['MS', 'Earlier: Alice leads the risk team. Later: the risk team approved the new fraud model v4', 'who leads the team that approved fraud model v4', 'Alice'],
  // TEMPORAL (track dates/order)
  ['TR', 'On 2026-03-01 we set the retry limit to 3. On 2026-05-10 we raised it to 5. On 2026-06-20 we lowered it to 4', 'what is the CURRENT retry limit after the 2026-06-20 change', '4'],
  ['TR', 'The migration ran in phases: phase 1 on 2026-01-05, phase 2 on 2026-02-05, final phase on 2026-03-05', 'when did the FINAL migration phase run', '2026-03-05'],
  // KNOWLEDGE UPDATE (fact changed -> use latest)
  ['KU', 'The default database is Postgres. UPDATE: as of 2026-06 the default database is now SQLite', 'what is the default database NOW', 'SQLite'],
  ['KU', 'The on-call lead was Bob. UPDATE: the on-call lead is now Carol as of this sprint', 'who is the CURRENT on-call lead', 'Carol'],
  // ABSTENTION (never stored -> must decline, NOT fabricate)
  ['ABS', null, 'what is the CEO home address', '__ABSTAIN__'],
  ['ABS', null, 'what colour is the office break-room sofa', '__ABSTAIN__'],
  ['ABS', null, 'how many llamas does the finance team own', '__ABSTAIN__'],
];

(async () => {
  const c = new McpClient(BRAIN, {}); c.start(); await c.initialize();
  // plant the non-abstention memories
  console.log('planting memories...');
  for (let i=0;i<ITEMS.length;i++) {
    const [cat, store] = ITEMS[i];
    if (store) await c.call('remember', { content: store, id: `lme_${i}`, pillar: 'Semantic' });
  }
  console.log('recalling by category...');
  const perCat = {};
  for (let i=0;i<ITEMS.length;i++) {
    const [cat, store, query, expect] = ITEMS[i];
    (perCat[cat] ||= { n:0, h1:0, h5:0, h10:0, absOK:0 });
    perCat[cat].n++;
    const r = await c.call('ask', { query, top: 10 }, 60000);
    const text = r.text || '';
    if (expect === '__ABSTAIN__') {
      // abstention: correct if NO confident result (ask returns weak/none). Heuristic: no [0.5+] hit
      // AND our planted id isn't there (nothing to find).
      const confident = /\[0\.[5-9]\d?\]|\[1\.00\]/.test(text);
      if (!confident) perCat[cat].absOK++;
      console.log(`  [ABS] "${query.slice(0,30)}" -> ${confident?'FABRICATED (confident hit)':'abstained OK'}`);
    } else {
      // recall@k: does the expected token appear, and at what rank? split ask results by rank markers.
      const blocks = text.split(/\n\s*\d+\.\s/).slice(1); // rough per-result split
      let rank = -1;
      for (let b=0;b<blocks.length;b++) { if (blocks[b].includes(expect)) { rank=b; break; } }
      // fallback: whole-text contains it (rank unknown but present -> count as <=10)
      if (rank < 0 && text.includes(expect)) rank = 9;
      if (rank === 0) perCat[cat].h1++;
      if (rank >= 0 && rank < 5) perCat[cat].h5++;
      if (rank >= 0 && rank < 10) perCat[cat].h10++;
      console.log(`  [${cat}] "${query.slice(0,30)}" -> ${rank<0?'MISS':'rank '+(rank+1)}`);
    }
  }
  c.stop();

  // aggregate
  const report = {};
  for (const [cat, s] of Object.entries(perCat)) {
    if (cat === 'ABS') report[cat] = { n: s.n, abstention_accuracy: +(s.absOK/s.n).toFixed(2) };
    else report[cat] = { n: s.n, recall_at_1: +(s.h1/s.n).toFixed(2), recall_at_5: +(s.h5/s.n).toFixed(2), recall_at_10: +(s.h10/s.n).toFixed(2) };
  }
  fs.writeFileSync(path.join(__dirname,'phase3-battery.json'), JSON.stringify(report, null, 2));
  console.log('\n== LongMemEval-style results (on 46.6k-frame brain) ==');
  for (const [cat, r] of Object.entries(report)) console.log(`  ${cat}: ${JSON.stringify(r)}`);
  console.log('\n  gate: vs Mem0 LoCoMo 92.5 / LongMemEval 94.4 (accuracy). ABS = abstention accuracy.');
  process.exit(0);
})().catch(e => { console.error('PHASE3 ERROR:', e.message); process.exit(2); });
