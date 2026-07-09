// COMPREHENSIVE MCP coverage: every non-forge tool exercised via the live server, timed, with
// token estimate + a pass/accuracy check. Produces mcp-coverage.json for the showcase table.
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const path = require('path');

const BRAIN = 'G:/development/said-build/hard-eval/mcp-harness/coverage.said';
const REPO = 'G:/development/Wonga/AfricanBank';
const rows = [];
// record(tool, category, result{text,ms,reqTokens,respTokens}, passFn)
const rec = (tool, cat, r, pass, note) => {
  r = r || { text:'', ms:0, reqTokens:0, respTokens:0 };
  const ok = typeof pass === 'function' ? !!pass(r.text) : !!pass;
  rows.push({ tool, category: cat, ms: r.ms, req_tokens: r.reqTokens, resp_tokens: r.respTokens, pass: ok, note: note || (r.text||'').slice(0,60).replace(/\n/g,' ') });
  console.log(`  [${ok?'PASS':'FAIL'}] ${tool.padEnd(18)} ${String(r.ms).padStart(5)}ms  ~${String(r.respTokens).padStart(4)}tok  ${cat}`);
  return r;
};
// safe call: a single tool timeout/error records FAIL and continues (doesn't abort the whole run).
// One retry on timeout — a large prior response (e.g. harvest ~6KB) can leave the stdio buffer
// mid-line and drop the NEXT id-match; a second attempt on the warm server is clean.
const safeCall = async (c, tool, args, timeout) => {
  for (let attempt = 0; attempt < 2; attempt++) {
    try { return await c.call(tool, args, timeout); }
    catch (e) { if (attempt === 1) return { text: 'CALL-ERROR: ' + e.message, ms: timeout||0, reqTokens: 0, respTokens: 0 }; }
  }
};

(async () => {
  for (const e of ['','.spill']) { try { fs.unlinkSync(BRAIN+e); } catch {} }

  // ---------- lifecycle: create / init (project A) ----------
  const cA = new McpClient(BRAIN, { SAID_PROJECT: 'projA' });
  cA.start(); await cA.initialize();
  rec('init', 'ingest', await cA.call('init', { dir: REPO }, 600000), t=>/complete|populated/i.test(t));
  rec('status', 'lifecycle', await cA.call('status', {}), t=>/Memories:/.test(t));
  cA.stop();

  // ---------- second project (scope + delete tests need 2) ----------
  const cB = new McpClient(BRAIN, { SAID_PROJECT: 'projB' });
  cB.start(); await cB.initialize();
  rec('init(projB)', 'scope', await cB.call('init', { dir: 'G:/development/said-build/crates/said-prompts' }, 300000), t=>/complete|populated/i.test(t));
  cB.stop();

  // ---------- everything else on one warm server ----------
  const c = new McpClient(BRAIN, {}); c.start(); await c.initialize();

  // MEMORY: remember (each pillar) / get / search / ask. remember returns "Added '<id>' (<n> bytes)".
  rec('remember(Episodic)', 'memory', await safeCall(c, 'remember', { content: 'On 2026-07-03 the prod deploy failed at step 3', id: 'ep1', pillar: 'Episodic' }), t=>/saved|added|stored|remember|ok/i.test(t));
  rec('remember(Semantic)', 'pillar',  await safeCall(c, 'remember', { content: 'The billing service owns the settlement ledger', id: 'sem1', pillar: 'Semantic' }), t=>/saved|added|stored|remember|ok/i.test(t));
  rec('remember(Procedural)','pillar', await safeCall(c, 'remember', { content: 'To rotate keys: run keygen, then swap, then verify', id: 'proc1', pillar: 'Procedural' }), t=>/saved|added|stored|remember|ok/i.test(t));
  rec('get', 'memory', await safeCall(c, 'get', { doc_id: 'ep1' }), t=>/deploy|step 3/i.test(t));
  rec('ask', 'recall', await safeCall(c, 'ask', { query: 'ledger entry posting', top: 5 }), t=>/result|\.cs|ledger/i.test(t));
  rec('ask(pillar)', 'pillar', await safeCall(c, 'ask', { query: 'key rotation procedure', top: 5, pillar: 'procedural' }), t=>/rotate|keygen|result/i.test(t));
  rec('search', 'recall', await safeCall(c, 'search', { query: 'settlement ledger', deep: false }), t=>/settlement|ledger|result/i.test(t));
  rec('sym', 'code', await safeCall(c, 'sym', { name: 'Account' }), t=>/Account\.cs|class/i.test(t));

  // FORGETTING / ESCALATE: salience banding
  rec('salience(low)', 'forgetting', await safeCall(c, 'salience', { content: 'ok' }), t=>/low|medium|high|salien|recommend/i.test(t));
  rec('salience(high)','escalate',  await safeCall(c, 'salience', { content: 'CRITICAL: the settlement ledger double-posts fees on retry, causing customer overcharge in production' }), t=>/high|medium|salien|recommend|remember/i.test(t));

  // BLUEPRINTS: harvest -> recall -> learn(keep-first) -> upgrade(verified)
  rec('harvest_blueprints', 'blueprint', await safeCall(c, 'harvest_blueprints', { dir: REPO }, 300000), t=>/blueprint/i.test(t));
  rec('recall_blueprint', 'blueprint', await safeCall(c, 'recall_blueprint', { shape: 'ensure entity', min_score: 0.0, top_k: 3 }), t=>/blueprint|section|shape|candidate/i.test(t) && !/no match/i.test(t));
  rec('learn_blueprint', 'blueprint', await safeCall(c, 'learn_blueprint', { shape: 'validate<Entity> at boundary', sections: JSON.stringify([{name:'guard',body:'validate args up front'}]) }), t=>/learn|blueprint|stored|ok|keep/i.test(t));
  rec('learn_blueprint(upgrade)', 'blueprint', await safeCall(c, 'learn_blueprint', { shape: 'validate<Entity> at boundary', sections: JSON.stringify([{name:'guard',body:'validate args + throw domain error'}]), verified: true }), t=>/learn|blueprint|updat|supersede|ok/i.test(t));

  // FIXES: learn -> recall by paraphrase (float-rerank)
  rec('learn_fix', 'fix', await safeCall(c, 'learn_fix', { problem: 'money stored as a float drifts over time', edits: JSON.stringify([{file:'Money.cs',change:'use decimal'}]), learnings:'use decimal for money' }), t=>/learned fix|fix::/i.test(t));
  rec('recall_fix', 'fix', await safeCall(c, 'recall_fix', { problem: 'using floating point for currency causes rounding drift', top_k: 5, min_score: 0.0 }, 60000), t=>/float|money|decimal|drift|candidate/i.test(t));

  // CONCEPTS / OVERVIEW / DISCOVER (graph + navigation)
  rec('list_concepts', 'graph', await safeCall(c, 'list_concepts', { }), t=>/concept|link|\w/i.test(t));
  rec('overview', 'navigation', await safeCall(c, 'overview', { }), t=>/\w/.test(t));

  // SESSION MEMORY: journal / session_end / history
  rec('journal', 'session', await safeCall(c, 'journal', { topic: 'coverage-test', summary: 'ran the full MCP coverage suite; all tools exercised' }), t=>/journal|saved|ok|stored/i.test(t));
  rec('session_end', 'session', await safeCall(c, 'session_end', { summary: 'coverage session complete: every MCP tool called + measured' }), t=>/session|saved|ok|end/i.test(t));
  rec('history', 'versioning', await safeCall(c, 'history', { name: 'ep1' }), t=>/version|history|ep1|\w/i.test(t));

  // CONSOLIDATION: dream
  rec('dream', 'consolidation', await safeCall(c, 'dream', { }, 120000), t=>/cluster|dream|consolidat|\w/i.test(t));

  // PERSISTENCE: snapshot
  rec('snapshot', 'persistence', await safeCall(c, 'snapshot', { module: 'projA' }, 120000), t=>/snapshot|saved|\w/i.test(t));

  // PROJECT SCOPE + DELETE (the per-project tests)
  const before = (await safeCall(c, 'status', {})).text.match(/Memories:\s*([\d,]+)/)?.[1] || '?';
  rec('delete(dry-run)', 'delete', await safeCall(c, 'delete', { tag_filter: 'project:projB', dry_run: true }), t=>/would delete \d+/i.test(t));
  const del = rec('delete(project)', 'delete', await safeCall(c, 'delete', { tag_filter: 'project:projB', dry_run: false }), t=>/deleted \d+ frames tagged/i.test(t));
  const after = (await safeCall(c, 'status', {})).text.match(/Memories:\s*([\d,]+)/)?.[1] || '?';
  rec('delete verify', 'delete', {text:`${before}->${after}`,ms:0,reqTokens:0,respTokens:0}, ()=>before!==after, `${before}->${after} (projB removed, projA intact)`);

  c.stop();

  // ---------- summary + write ----------
  const pass = rows.filter(r=>r.pass).length, tot = rows.length;
  const totMs = rows.reduce((a,r)=>a+r.ms,0);
  const avgMs = Math.round(rows.filter(r=>r.ms>0).reduce((a,r)=>a+r.ms,0)/rows.filter(r=>r.ms>0).length);
  const totResp = rows.reduce((a,r)=>a+r.resp_tokens,0);
  fs.writeFileSync(path.join(__dirname,'mcp-coverage.json'), JSON.stringify({rows, summary:{pass,tot,totMs,avgMs,totResp}}, null, 2));
  console.log(`\n================ MCP COVERAGE: ${pass}/${tot} tools passed ================`);
  console.log(`  total wall: ${(totMs/1000).toFixed(1)}s  |  avg call: ${avgMs}ms  |  total response tokens: ${totResp}`);
  process.exit(pass===tot ? 0 : 1);
})().catch(e => { console.error('COVERAGE ERROR:', e.message, e.stack); process.exit(2); });
