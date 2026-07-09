// Diagnose: does MCP learn_fix -> recall_fix work in ONE session? (persistence + reload)
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const BRAIN = 'G:/development/said-build/hard-eval/mcp-harness/fixcheck.said';
(async () => {
  for (const ext of ['','.spill']) { try { fs.unlinkSync(BRAIN+ext); } catch {} }
  const c = new McpClient(BRAIN, { SAID_PROJECT: 'test' }); c.start(); await c.initialize();
  // tiny ingest so the brain isn't empty
  await c.call('remember', { text: 'seed note for the fix brain', });
  console.log('learning 3 distinct fixes...');
  const fixes = [
    'off-by-one in the amortization schedule loop last period',
    'deadlock when two transfers hit the same account',
    'null account id crashes the ledger posting command',
  ];
  for (let i=0;i<fixes.length;i++) {
    const r = await c.call('learn_fix', {
      problem: fixes[i], edits: JSON.stringify([{file:`f${i}.x`, change:`resolve ${fixes[i]}`}]),
      learnings:`fix-${i}`, label:'project:test',
    });
    console.log(`  learn[${i}]: ${r.text.slice(0,70).replace(/\n/g,' ')}`);
  }
  console.log('recalling each by paraphrase (same session)...');
  const paras = ['amortization final period miscounts by one','concurrent transfers on one account hang','posting fails when account id is missing'];
  for (let i=0;i<paras.length;i++) {
    const r = await c.call('recall_fix', { problem: paras[i], min_score: 0.0 }, 30000);
    const ok = r.text.includes(fixes[i]) || r.text.includes(`fix-${i}`);
    console.log(`  recall[${i}] (${r.ms}ms) ${ok?'HIT':'miss'}: ${r.text.slice(0,70).replace(/\n/g,' ')}`);
  }
  c.stop();
  process.exit(0);
})().catch(e=>{console.error('ERR:',e.message);process.exit(2);});
