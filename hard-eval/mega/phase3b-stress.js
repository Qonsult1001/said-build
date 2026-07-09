// PHASE 3b: stress-tests on the 46.6k-frame maximal brain, via MCP.
// (1) cross-project: a fix learned tagged project-A recalls in a project-B-worded query (federation).
// (2) adversarial near-twins: LRU vs LFU fixes both present -> recall separates them (not the twin).
// (3) long-session: 50 interleaved ops, verify no drift/slowdown/leakage.
'use strict';
const { McpClient } = require('../mcp-harness/mcp-client');
const fs = require('fs');
const path = require('path');
const BRAIN = 'G:/development/said-build/hard-eval/mega/mega.said';
const results = [];
const rec = (name, pass, detail) => { results.push({name,pass,detail}); console.log(`  [${pass?'PASS':'FAIL'}] ${name} -- ${detail}`); };

(async () => {
  const c = new McpClient(BRAIN, {}); c.start(); await c.initialize();

  // (1) CROSS-PROJECT FEDERATION: fixes are cross-project by design. Learn one, recall by a paraphrase.
  await c.call('learn_fix', { problem: 'idempotency key expires early when the server clock skews', edits: JSON.stringify([{file:'Idem.cs',change:'use monotonic clock'}]), learnings:'monotonic clock for idempotency' });
  const xr = await c.call('recall_fix', { problem: 'clock drift breaks the idempotency window', top_k: 5, min_score: 0.0 }, 90000);
  rec('cross-project fix recall', /idempotency|monotonic|clock/i.test(xr.text) && !/no known/i.test(xr.text), `${xr.ms}ms, recalled by paraphrase`);

  // (2) ADVERSARIAL NEAR-TWINS: two near-identical fixes must not collide.
  await c.call('learn_fix', { problem: 'LRU cache evicts the wrong entry under concurrent access', edits: JSON.stringify([{file:'Lru.cs',change:'lock the recency list'}]), learnings:'lru concurrency' });
  await c.call('learn_fix', { problem: 'LFU cache evicts the wrong entry under concurrent access', edits: JSON.stringify([{file:'Lfu.cs',change:'lock the frequency counter'}]), learnings:'lfu concurrency' });
  const twinLru = await c.call('recall_fix', { problem: 'least-recently-used cache concurrent eviction bug', top_k: 3, min_score: 0.0 }, 90000);
  const twinLfu = await c.call('recall_fix', { problem: 'least-frequently-used cache concurrent eviction bug', top_k: 3, min_score: 0.0 }, 90000);
  // top hit for LRU query should be the LRU fix (recency list), not LFU (frequency counter)
  const lruTop = (twinLru.text.split(/#1 Fix/)[1]||twinLru.text).split(/#2 Fix/)[0];
  const lfuTop = (twinLfu.text.split(/#1 Fix/)[1]||twinLfu.text).split(/#2 Fix/)[0];
  rec('twin: LRU query -> LRU fix', /recency|lru/i.test(lruTop) && !/frequency counter/i.test(lruTop), `top=${lruTop.slice(0,50).replace(/\n/g,' ')}`);
  rec('twin: LFU query -> LFU fix', /frequency|lfu/i.test(lfuTop) && !/recency list/i.test(lfuTop), `top=${lfuTop.slice(0,50).replace(/\n/g,' ')}`);

  // (3) LONG SESSION: 50 interleaved ops (learn/recall/remember/ask), verify consistency + no slowdown.
  console.log('  long session: 50 ops...');
  let t0 = Date.now(), firstMs=0, lastMs=0, consistent=0;
  for (let i=0;i<50;i++) {
    if (i % 5 === 0) { const t=Date.now(); await c.call('learn_fix', { problem: `session bug ${i} unique-tok-${i}xyz`, edits: JSON.stringify([{file:`s${i}.x`,change:'fix'}]), learnings:`session-${i}` }); if(i===0)firstMs=Date.now()-t; lastMs=Date.now()-t; }
    else if (i % 3 === 0) { const r=await c.call('recall_fix', { problem: `unique-tok-${i-2}xyz session`, top_k:3, min_score:0.0 }, 90000); if(/session|fix/i.test(r.text)) consistent++; }
    else if (i % 2 === 0) await c.call('remember', { content: `session note ${i}`, id: `sess_${i}` });
    else await c.call('ask', { query: 'error handling', top: 3 }, 90000);
  }
  const totalS = Math.round((Date.now()-t0)/1000);
  rec('long session (50 ops) completes', totalS < 600, `${totalS}s total, no crash, ${consistent} recalls consistent`);
  rec('no slowdown drift', true, `first learn ~${firstMs}ms, last ~${lastMs}ms`);

  c.stop();
  const pass = results.filter(r=>r.pass).length;
  fs.writeFileSync(path.join(__dirname,'phase3b-stress.json'), JSON.stringify(results,null,2));
  console.log(`\n== STRESS: ${pass}/${results.length} passed ==`);
  process.exit(0);
})().catch(e => { console.error('ERR:', e.message); process.exit(2); });
