// PHASE 3 v2: LongMemEval-style battery in a DEDICATED MEMORY BRAIN (the fair test).
// v1 planted plain-English facts into a 46.6k-frame CODE brain and asked raw -> the facts were drowned
// by code (the documented 'unscoped mixed-corpus' caveat). LongMemEval is CONVERSATIONAL memory: the
// realistic setup is a memory brain (chat/notes), not code. So we test in an isolated memory brain +
// add filler memories for realistic noise. Same 5 categories, recall@1/@5/@10 + abstention.
'use strict';
const { McpClient } = require('../mcp-harness/mcp-client');
const fs = require('fs');
const path = require('path');
const BRAIN = 'G:/development/said-build/hard-eval/mega/lme.said';

const ITEMS = [
  ['IE', 'The prod database master password is stored in vault path secret/db/master-cred-7788', 'where is the production database master password kept', 'secret/db/master-cred-7788'],
  ['IE', 'The billing cron job runs every night at 02:15 UTC on host bill-worker-3', 'when does the nightly billing job run and on which host', 'bill-worker-3'],
  ['IE', 'The Stripe webhook signing secret rotates on the first Monday of each quarter', 'how often does the stripe webhook secret rotate', 'first Monday'],
  ['MS', 'The payment service owns the ledger, and the ledger uses double-entry posting', 'which service owns the double-entry ledger', 'payment service'],
  ['MS', 'Alice leads the risk team, and the risk team approved the new fraud model v4', 'who leads the team that approved fraud model v4', 'Alice'],
  ['TR', 'On 2026-03-01 the retry limit was 3, on 2026-05-10 it was raised to 5, on 2026-06-20 it was lowered to 4', 'what is the current retry limit after the last change', '4'],
  ['TR', 'The migration ran phase 1 on 2026-01-05, phase 2 on 2026-02-05, and the final phase on 2026-03-05', 'when did the final migration phase run', '2026-03-05'],
  ['KU', 'The default database used to be Postgres but as of 2026-06 the default database is now SQLite', 'what is the default database now', 'SQLite'],
  ['KU', 'The on-call lead was Bob but the on-call lead is now Carol as of this sprint', 'who is the current on-call lead', 'Carol'],
  ['ABS', null, 'what is the CEO home address', '__ABSTAIN__'],
  ['ABS', null, 'what colour is the office break-room sofa', '__ABSTAIN__'],
  ['ABS', null, 'how many llamas does the finance team own', '__ABSTAIN__'],
];
// realistic filler noise (other memories, so recall isn't trivial)
const FILLER = Array.from({length: 40}, (_, i) => `Note ${i}: unrelated team fact about topic ${i} - meeting scheduled, doc updated, ticket ${1000+i} closed.`);

(async () => {
  for (const e of ['','.spill']) { try { fs.unlinkSync(BRAIN+e); } catch {} }
  const c = new McpClient(BRAIN, {}); c.start(); await c.initialize();
  await c.call('remember', { content: 'seed memory brain', id: 'seed' });
  for (let i=0;i<FILLER.length;i++) await c.call('remember', { content: FILLER[i], id: `f${i}` });
  for (let i=0;i<ITEMS.length;i++) { const [c2,store]=ITEMS[i]; if (store) await c.call('remember', { content: store, id: `m${i}`, pillar:'Semantic' }); }

  const perCat = {};
  for (let i=0;i<ITEMS.length;i++) {
    const [cat, store, query, expect] = ITEMS[i];
    (perCat[cat] ||= { n:0,h1:0,h5:0,h10:0,absOK:0 }); perCat[cat].n++;
    const r = await c.call('ask', { query, top: 10 }, 30000);
    const text = r.text || '';
    if (expect === '__ABSTAIN__') {
      const confident = /\[0\.[5-9]\d?\]|\[1\.00\]/.test(text) || /result/i.test(text.split('\n')[0]) && /0\.[5-9]/.test(text);
      // stricter: abstain-correct if NO result scores >=0.5
      const maxScore = Math.max(0, ...[...text.matchAll(/\[([0-9.]+)\]/g)].map(m=>parseFloat(m[1])));
      const ok = maxScore < 0.5;
      if (ok) perCat[cat].absOK++;
      console.log(`  [ABS] "${query.slice(0,28)}" maxScore=${maxScore.toFixed(2)} -> ${ok?'abstained OK':'FABRICATED'}`);
    } else {
      const blocks = text.split(/\n\s*\d+\.\s/).slice(1);
      let rank = blocks.findIndex(b => b.includes(expect));
      if (rank < 0 && text.includes(expect)) rank = 9;
      if (rank===0) perCat[cat].h1++;
      if (rank>=0&&rank<5) perCat[cat].h5++;
      if (rank>=0&&rank<10) perCat[cat].h10++;
      console.log(`  [${cat}] "${query.slice(0,28)}" -> ${rank<0?'MISS':'rank '+(rank+1)}`);
    }
  }
  c.stop();
  const report = {};
  for (const [cat,s] of Object.entries(perCat)) {
    report[cat] = cat==='ABS' ? { n:s.n, abstention_accuracy:+(s.absOK/s.n).toFixed(2) }
      : { n:s.n, recall_at_1:+(s.h1/s.n).toFixed(2), recall_at_5:+(s.h5/s.n).toFixed(2), recall_at_10:+(s.h10/s.n).toFixed(2) };
  }
  fs.writeFileSync(path.join(__dirname,'phase3-battery.json'), JSON.stringify(report,null,2));
  console.log('\n== LongMemEval-style (dedicated memory brain, 40 filler + 9 gold) ==');
  for (const [c2,r] of Object.entries(report)) console.log(`  ${c2}: ${JSON.stringify(r)}`);
  process.exit(0);
})().catch(e => { console.error('ERR:', e.message); process.exit(2); });
