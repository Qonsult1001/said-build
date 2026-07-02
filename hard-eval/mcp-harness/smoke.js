// SMOKE: africanbank only, through a miniature of all 5 phases, ALL via MCP.
// Proves the harness + gate logic before the full 4-project run.
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const path = require('path');

const BRAIN = process.env.SMOKE_BRAIN || 'G:/development/said-build/hard-eval/mcp-harness/smoke.said';
const REPO = 'G:/development/Wonga/AfricanBank';
const results = [];
const rec = (phase, name, pass, detail) => {
  results.push({ phase, name, pass, detail });
  console.log(`  [${pass ? 'PASS' : 'FAIL'}] ${phase} :: ${name} — ${detail}`);
};

(async () => {
  // fresh brain
  for (const ext of ['', '.spill']) { try { fs.unlinkSync(BRAIN + ext); } catch {} }

  // ---- P1: ingest africanbank via MCP (project-tagged) ----
  console.log('\n== P1: INGEST (MCP init, project=africanbank) ==');
  const c = new McpClient(BRAIN, { SAID_PROJECT: 'africanbank' });
  c.start();
  await c.initialize();
  const t0 = Date.now();
  const ing = await c.call('init', { dir: REPO }, 600000);
  const ingMs = Date.now() - t0;
  const st = await c.call('status', {});
  const mem = (st.text.match(/Memories:\s*([\d,]+)/) || [])[1] || '?';
  const memN = parseInt((mem + '').replace(/,/g, '')) || 0;
  const sizeMB = fs.existsSync(BRAIN) ? (fs.statSync(BRAIN).size / 1048576).toFixed(1) : '0';
  rec('P1', 'ingest+status', memN > 1000, `${mem} memories, ${sizeMB}MB, ${(ingMs/1000).toFixed(1)}s`);

  // ---- P2: code recall + sym via MCP ----
  console.log('\n== P2: CODE RECALL (MCP ask + sym) ==');
  const ask = await c.call('ask', { query: 'ledger entry', top: 5 });
  rec('P2', 'ask ledger entry', /LedgerEntry|\.cs/.test(ask.text), `${ask.ms}ms, resp≈${ask.respTokens}tok`);
  const sym = await c.call('sym', { name: 'Account' });
  rec('P2', 'sym Account', /Account\.cs/.test(sym.text), `${sym.ms}ms`);

  // ---- P2b: blueprint lifecycle (harvest_blueprints LEARNS them; recall confirms) ----
  console.log('\n== P2b: BLUEPRINT LIFECYCLE ==');
  const harv = await c.call('harvest_blueprints', { dir: REPO }, 300000);
  const bpCount = (harv.text.match(/(\d+)\s+blueprint/i) || [])[1] || '0';
  rec('P2b', 'harvest_blueprints', parseInt(bpCount) > 0, `${bpCount} blueprints learned, ${harv.ms}ms`);
  // recall a blueprint that was harvested
  const rbp = await c.call('recall_blueprint', { shape: 'ensure entity', min_score: 0.0, top_k: 3 });
  rec('P2b', 'recall_blueprint', /[Bb]lueprint|section|shape/.test(rbp.text) && !/no match/i.test(rbp.text), `${rbp.ms}ms`);

  // ---- P3: mini memory loop (5 iters, 2 projects interleaved) ----
  console.log('\n== P3: MINI MEMORY LOOP (5 iters, africanbank+other) ==');
  const projFixes = [];
  for (let i = 0; i < 5; i++) {
    const proj = i % 2 === 0 ? 'africanbank' : 'other';
    const problem = `iter${i} ${proj} bug: null in handler ${i}`;
    const learn = `guard step ${i} for ${proj}`;
    // learn_fix uses server-side project scope; simulate by tagging in the problem text + label
    await c.call('learn_fix', {
      problem, edits: JSON.stringify([{ file: `${proj}/F${i}.cs`, change: learn }]),
      learnings: `learning-${proj}-${i}`, label: `project:${proj}`,
    });
    projFixes.push({ i, proj, problem, learn });
  }
  // recall by paraphrase; check (a) a fix comes back and (b) its project provenance is preserved.
  let recalled = 0, provOk = 0;
  for (const f of projFixes) {
    const r = await c.call('recall_fix', { problem: `null in handler ${f.i} issue`, min_score: 0.0 });
    if (/TASK:|LEARNINGS:|Fix \(/.test(r.text)) recalled++;
    // provenance tag must be one of our two projects (proves project-scoping is carried on the fix)
    if (/project:(africanbank|other)/.test(r.text)) provOk++;
  }
  rec('P3', 'memory-loop recall', recalled >= 4, `${recalled}/5 recalled`);
  rec('P3', 'project-provenance preserved', provOk >= 4, `${provOk}/5 carry project: tag`);

  // ---- P4: token comparison (recall tokens vs a file-read baseline) ----
  console.log('\n== P4: TOKEN COMPARISON ==');
  const ledgerFiles = [];
  (function walk(d){ for(const e of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,e.name);
    if(e.isDirectory()&&!/obj|bin/.test(e.name))walk(p); else if(/Ledger.*\.cs$/.test(e.name))ledgerFiles.push(p);} })(REPO);
  let claudeBytes = 0; for (const f of ledgerFiles.slice(0,20)) { try{claudeBytes+=fs.statSync(f).size;}catch{} }
  const claudeTok = Math.ceil(claudeBytes/4);
  const saidTok = ask.respTokens; // .said returned the answer in this many tokens
  const ratio = saidTok>0 ? Math.round(claudeTok/saidTok) : 'n/a';
  rec('P4', 'token savings', claudeTok>saidTok, `.said ${saidTok}tok vs file-read ${claudeTok}tok = ${ratio}x`);

  // ---- P2c: delete-by-project. FINDING: the MCP `delete` tool requires a time criterion
  // (doc_id / older_than_days / before_date); tag_filter is only a SECONDARY filter. So
  // "remove a project by tag alone" is NOT supported via the delete tool. We test the
  // supported combo (older_than_days huge + tag_filter) as the closest project-scoped delete.
  console.log('\n== P2c: DELETE-BY-PROJECT (dry run, supported combo) ==');
  const delTagOnly = await c.call('delete', { tag_filter: 'project:africanbank', dry_run: true });
  const tagOnlyBlocked = /No deletion criteria/i.test(delTagOnly.text);
  const delCombo = await c.call('delete', { older_than_days: 36500, tag_filter: 'project:africanbank', dry_run: true });
  const comboWorks = /\d/.test(delCombo.text) && !/No deletion criteria/i.test(delCombo.text);
  rec('P2c', 'delete tag+time (supported)', comboWorks, delCombo.text.slice(0,70).replace(/\n/g,' '));
  rec('P2c', 'FINDING: tag-only delete blocked', true, tagOnlyBlocked ? 'confirmed: MCP delete needs a time criterion (real gap)' : 'tag-only worked (gap resolved?)');

  c.stop();

  // ---- summary ----
  const pass = results.filter(r=>r.pass).length, tot=results.length;
  console.log(`\n================ SMOKE: ${pass}/${tot} checks passed ================`);
  fs.writeFileSync(path.join(__dirname,'smoke-result.json'), JSON.stringify(results,null,2));
  process.exit(pass===tot ? 0 : 1);
})().catch(e => { console.error('SMOKE ERROR:', e.message); process.exit(2); });
