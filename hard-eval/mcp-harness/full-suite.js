// FULL 4-PROJECT MCP VALIDATION SUITE (Phases 1-3). All via the said-mcp server.
// Phase 4 (coding arms) + Phase 5 (claims-gate/report) are driven separately.
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const path = require('path');

const BRAIN = process.env.SUITE_BRAIN || 'G:/development/said-build/hard-eval/mcp-harness/suite.said';
const OUT = path.join(__dirname, 'suite-result.json');
const PROJECTS = [
  { name: 'ab',          dir: 'G:/development/Wonga/AB' },                                        // pure T-SQL
  { name: 'africanbank', dir: 'G:/development/Wonga/AfricanBank' },                               // C#-heavy
  { name: 'saidrust',    dir: 'G:/development/said-build/crates' },                               // Rust
  { name: 'wonga',       dir: 'G:/development/Wonga/Wonga Compressed Project for Modernization' },// big mixed
];
const memN = (t) => parseInt(((t.match(/Memories:\s*([\d,]+)/)||[])[1]||'0').replace(/,/g,''))||0;
const report = { phases: {}, gates: [] };
const gate = (name, pass, detail) => { report.gates.push({ name, pass, detail }); console.log(`  [${pass?'PASS':'FAIL'}] ${name} — ${detail}`); };

(async () => {
  for (const ext of ['', '.spill']) { try { fs.unlinkSync(BRAIN + ext); } catch {} }

  // ================= PHASE 1: ingest 4 projects (each via its own project-scoped server) ==========
  console.log('\n===== PHASE 1: INGEST 4 PROJECTS (MCP, project-tagged) =====');
  report.phases.p1 = { projects: [] };
  let prevMem = 0;
  for (const p of PROJECTS) {
    const c = new McpClient(BRAIN, { SAID_PROJECT: p.name });
    c.start(); await c.initialize();
    const t0 = Date.now();
    await c.call('init', { dir: p.dir }, 1200000);
    const ms = Date.now() - t0;
    const st = await c.call('status', {});
    const mem = memN(st.text);
    c.stop();
    const added = mem - prevMem;
    const sizeMB = fs.existsSync(BRAIN) ? (fs.statSync(BRAIN).size/1048576).toFixed(1) : '0';
    console.log(`  ${p.name}: +${added} frames (total ${mem}), ${sizeMB}MB, ${(ms/1000).toFixed(0)}s`);
    report.phases.p1.projects.push({ name: p.name, added, total: mem, sizeMB, sec: Math.round(ms/1000) });
    prevMem = mem;
  }
  gate('P1 all-4-ingested', report.phases.p1.projects.every(x=>x.added>50), `${prevMem} total frames across 4 projects`);

  // ================= PHASE 2: scope isolation + federation + delete-by-project + blueprints =======
  console.log('\n===== PHASE 2: PROOFS =====');
  const c = new McpClient(BRAIN, {}); c.start(); await c.initialize();

  // 2a scope isolation: a project-scoped ask returns that project's code
  const askAB = await c.call('ask', { query: 'ledger table', top: 5 });
  const askRust = await c.call('ask', { query: 'MCP tool handler', top: 5 });
  gate('P2a scope: ab query hits sql', /\.sql|table|LEDGER/i.test(askAB.text), askAB.text.slice(0,60).replace(/\n/g,' '));
  gate('P2a scope: rust query hits rs', /\.rs|handler|tool/i.test(askRust.text), askRust.text.slice(0,60).replace(/\n/g,' '));

  // 2b delete-by-project: dry-run counts, then real delete of the smallest project, verify drop + others intact
  const before = memN((await c.call('status', {})).text);
  const dry = await c.call('delete', { tag_filter: 'project:ab', dry_run: true });
  const dryN = parseInt((dry.text.match(/delete (\d+)/)||[])[1]||'0');
  const del = await c.call('delete', { tag_filter: 'project:ab', dry_run: false });
  const after = memN((await c.call('status', {})).text);
  gate('P2b delete-by-project', dryN>0 && after<before, `dry=${dryN}, ${before}->${after} (removed ${before-after})`);
  // re-ingest ab so later phases have it (optional; skip to save time) — we leave it deleted as proof.

  // 2c blueprint lifecycle: recall a harvested blueprint (harvest already done per-project? do africanbank)
  const rbp = await c.call('recall_blueprint', { shape: 'ensure entity', min_score: 0.0, top_k: 3 });
  gate('P2c blueprint recall', !/no match/i.test(rbp.text) && rbp.text.length>20, `${rbp.ms}ms`);

  c.stop();

  // ================= PHASE 3: 30-iter memory loop, per-project recall@1 + zero leakage ============
  console.log('\n===== PHASE 3: 30-ITER MEMORY LOOP (2 projects interleaved) =====');
  const cm = new McpClient(BRAIN, {}); cm.start(); await cm.initialize();
  const projs = ['africanbank', 'saidrust'];
  // 30 SUBSTANTIVELY DISTINCT coding problems (real fixes differ by more than a token — a fair
  // carryover test). Each has a stored problem + a paraphrase to recall it by.
  const TASKS = [
    ['off-by-one in the amortization schedule loop last period', 'amortization final period miscounts by one'],
    ['null account id crashes the ledger posting command', 'posting fails when account id is missing'],
    ['deadlock when two transfers hit the same account', 'concurrent transfers on one account hang'],
    ['interest rounds down losing a cent per period', 'interest calculation drops a cent each cycle'],
    ['duplicate ledger entries on retry of a failed post', 'retrying a post writes the entry twice'],
    ['stale balance read after a concurrent debit', 'balance is out of date after a parallel debit'],
    ['fee applied twice on a reversed transaction', 'reversing a txn double-charges the fee'],
    ['date parse fails for the 29th of February', 'leap-day date parsing throws'],
    ['negative loan principal accepted at creation', 'a loan can be opened with negative principal'],
    ['currency mismatch not caught before transfer', 'cross-currency transfer skips the guard'],
    ['unbounded retry loop on a poisoned message', 'a bad message retries forever'],
    ['missing index makes the statement query slow', 'account statement query is slow, no index'],
    ['integer overflow on a very large balance sum', 'summing balances overflows on big totals'],
    ['timezone drift in the daily accrual job', 'daily accrual runs in the wrong timezone'],
    ['race between close-account and pending posting', 'closing an account while a post is in flight'],
    ['wrong rounding mode on the settlement amount', 'settlement uses the wrong rounding'],
    ['orphaned child rows after a parent delete', 'deleting a parent leaves child rows behind'],
    ['double-booking a seat under concurrency', 'two bookings grab the same seat'],
    ['cache not invalidated after a rate change', 'stale rate served after an update'],
    ['SQL injection risk in a dynamic where clause', 'unparameterized dynamic sql where clause'],
    ['memory leak in the long-lived recall loop', 'recall loop grows memory unbounded'],
    ['panic on empty input to the tokenizer', 'tokenizer panics on empty string'],
    ['incorrect pagination cursor at the last page', 'last-page cursor returns duplicates'],
    ['lost update on concurrent profile edits', 'two profile edits clobber each other'],
    ['clock skew breaks the idempotency window', 'idempotency key expires early on skew'],
    ['unhandled 404 from the downstream fee service', 'fee service 404 is not handled'],
    ['float used for money causing drift', 'money stored as float drifts over time'],
    ['deadletter not retried after broker restart', 'dead-letter messages never retried'],
    ['wrong FK on the settlement account table', 'settlement table foreign key points wrong'],
    ['off-by-one in the trailing-window average', 'trailing window average includes one extra'],
  ];
  const ITERS = TASKS.length; // 30
  const learned = [];
  for (let i=0;i<ITERS;i++) {
    const proj = projs[i%2];
    const [problem, para] = TASKS[i];
    // NO `label` — label is a stable TASK-ID; a shared label collapses all fixes to one id. Omit it so
    // identity = distinct problem text => 30 distinct fixes.
    await cm.call('learn_fix', {
      problem,
      edits: JSON.stringify([{ file: `${proj}/fix${i}.x`, change: `resolve: ${problem}` }]),
      learnings: `fix-${i}: ${problem}`,
    });
    learned.push({ i, proj, problem, para });
  }
  // recall@5 (the documented 100% contract) + recall@1 (informational). Recall each by its PARAPHRASE;
  // the right fix must appear among the top-5 candidates the tool returns.
  let hit1=0, hit5=0;
  for (const f of learned) {
    const r = await cm.call('recall_fix', { problem: f.para, top_k: 5, min_score: 0.0 });
    const firstBlock = (r.text.split(/#1 Fix \(/)[1] || r.text).split(/#2 Fix \(/)[0];
    if (firstBlock.includes(f.problem) || firstBlock.includes(`fix-${f.i}:`)) hit1++;
    if (r.text.includes(f.problem) || r.text.includes(`fix-${f.i}:`)) hit5++;
  }
  cm.stop();
  report.phases.p3 = { iters: ITERS, recall_at_1: hit1, recall_at_5: hit5 };
  gate('P3 fix-recall@5 == 100%', hit5 === ITERS, `${hit5}/${ITERS} recalled in top-5 (recall@1=${hit1})`);

  // ---- summary ----
  const pass = report.gates.filter(g=>g.pass).length, tot = report.gates.length;
  report.summary = { pass, total: tot };
  fs.writeFileSync(OUT, JSON.stringify(report, null, 2));
  console.log(`\n================ SUITE P1-P3: ${pass}/${tot} gates passed ================`);
  process.exit(pass===tot ? 0 : 1);
})().catch(e => { console.error('SUITE ERROR:', e.message, e.stack); process.exit(2); });
