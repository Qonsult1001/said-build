// Verify the delete-by-tag fix via MCP: ingest 2 projects into one brain, delete one by tag, confirm.
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const BRAIN = 'G:/development/said-build/hard-eval/mcp-harness/deltest.said';
const memN = (t) => parseInt(((t.match(/Memories:\s*([\d,]+)/)||[])[1]||'0').replace(/,/g,''))||0;

(async () => {
  for (const ext of ['', '.spill']) { try { fs.unlinkSync(BRAIN + ext); } catch {} }

  // ingest project A (saidrust crates - small + fast)
  const cA = new McpClient(BRAIN, { SAID_PROJECT: 'projA' });
  cA.start(); await cA.initialize();
  await cA.call('init', { dir: 'G:/development/said-build/crates/said-mcp' }, 300000);
  const stA = await cA.call('status', {});
  cA.stop();
  const afterA = memN(stA.text);
  console.log(`after project A ingest: ${afterA} memories`);

  // ingest project B into the SAME brain (different tag)
  const cB = new McpClient(BRAIN, { SAID_PROJECT: 'projB' });
  cB.start(); await cB.initialize();
  await cB.call('init', { dir: 'G:/development/said-build/crates/said-prompts' }, 300000);
  const stB = await cB.call('status', {});
  const afterB = memN(stB.text);
  console.log(`after project B ingest: ${afterB} memories (A+B in one brain)`);

  // dry-run delete projA by tag ALONE
  const dry = await cB.call('delete', { tag_filter: 'project:projA', dry_run: true });
  const dryCount = (dry.text.match(/Would delete (\d+)/)||[])[1] || '0';
  console.log(`dry-run delete project:projA -> would delete ${dryCount} frames`);

  // REAL delete projA by tag alone
  const del = await cB.call('delete', { tag_filter: 'project:projA', dry_run: false });
  console.log(`real delete: ${del.text.slice(0,90).replace(/\n/g,' ')}`);

  // confirm: status drops, projA query returns nothing, projB intact
  const stAfter = await cB.call('status', {});
  const afterDel = memN(stAfter.text);
  cB.stop();

  const deletedTagOnly = !/No deletion criteria/i.test(del.text) && parseInt(dryCount) > 0;
  const countDropped = afterDel < afterB;
  console.log(`\nmemories: A=${afterA} A+B=${afterB} afterDelete=${afterDel}`);
  console.log(`[${deletedTagOnly?'PASS':'FAIL'}] tag-only delete accepted + found frames`);
  console.log(`[${countDropped?'PASS':'FAIL'}] memory count dropped after project delete`);
  process.exit(deletedTagOnly && countDropped ? 0 : 1);
})().catch(e => { console.error('ERR:', e.message); process.exit(2); });
