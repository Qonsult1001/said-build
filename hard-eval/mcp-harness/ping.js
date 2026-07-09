// Minimal MCP round-trip test: initialize -> init -> status -> ask. Proves the harness core.
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const BRAIN = 'G:/development/said-build/hard-eval/mcp-harness/ping.said';
(async () => {
  for (const ext of ['', '.spill']) { try { fs.unlinkSync(BRAIN + ext); } catch {} }
  const c = new McpClient(BRAIN, { SAID_PROJECT: 'africanbank' });
  c.start();
  console.log('init handshake...');
  const ir = await c.initialize();
  console.log('  serverInfo:', ir && ir.serverInfo ? ir.serverInfo.name : JSON.stringify(ir).slice(0,80));
  console.log('ingesting (init dir)...');
  const t0 = Date.now();
  const ing = await c.call('init', { dir: 'G:/development/Wonga/AfricanBank' }, 600000);
  console.log(`  init done in ${((Date.now()-t0)/1000).toFixed(1)}s: ${ing.text.slice(0,120).replace(/\n/g,' ')}`);
  const st = await c.call('status', {});
  console.log('  status:', st.text.match(/Memories:\s*[\d,]+/)?.[0] || st.text.slice(0,60));
  const ask = await c.call('ask', { query: 'ledger entry', top: 3 });
  console.log(`  ask (${ask.ms}ms): ${ask.text.slice(0,100).replace(/\n/g,' ')}`);
  c.stop();
  console.log('PING OK');
  process.exit(0);
})().catch(e => { console.error('PING ERROR:', e.message); process.exit(2); });
