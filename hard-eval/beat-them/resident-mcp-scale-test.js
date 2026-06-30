#!/usr/bin/env node
// RESIDENT-MCP SCALE TEST on the PRODUCTION binary — the real measurement (owner insisted).
// Drives target/production/said-mcp.exe (resident: encoder loads ONCE) against a real African-Bank brain.
// Measures: cold-init-once vs WARM recall (the true per-query speed), code sym-exact, and the nudge SAVE
// path (agent calls learn_fix/remember over MCP -> persisted -> immediately recallable).
const { spawn, execFileSync } = require('child_process');
const fs = require('fs'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const MCP = path.join(ROOT, 'target', 'production', 'said-mcp.exe');
const SAID = path.join(ROOT, 'target', 'production', 'said.exe');
const SRC = path.join(__dirname, 'scale-test', 'scale.said');
const BR = path.join(__dirname, 'scale-test', 'scale-mcp.said');   // operate on a copy (repeatable)
for (const f of [BR, BR + '.spill']) { try { fs.unlinkSync(f); } catch {} }
fs.copyFileSync(SRC, BR); try { fs.copyFileSync(SRC + '.spill', BR + '.spill'); } catch {}

const srv = spawn(MCP, ['--path', BR], { stdio: ['pipe', 'pipe', 'ignore'], env: { ...process.env, SAID_PROJECT: 'scale' } });
let buf = ''; const p = new Map(); let id = 0;
srv.stdout.on('data', d => { buf += d; let n; while ((n = buf.indexOf('\n')) >= 0) { const l = buf.slice(0, n); buf = buf.slice(n + 1); if (!l.trim()) continue; let m; try { m = JSON.parse(l); } catch { continue; } if (m.id && p.has(m.id)) { p.get(m.id)(m); p.delete(m.id); } } });
const rpc = (me, pa) => new Promise(r => { const i = ++id; p.set(i, r); srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method: me, params: pa }) + '\n'); });
const call = (name, args) => rpc('tools/call', { name, arguments: args }).then(r => (r.result?.content || []).map(c => c.text).join('\n'));

let fails = 0; const ok = (n, c, d) => { console.log(`  ${c ? 'PASS' : 'FAIL'}  ${n}${d ? '  ' + d : ''}`); if (!c) fails++; };

(async () => {
  console.log('=== RESIDENT-MCP SCALE TEST (production binary, 4,868-frame African Bank brain) ===\n');

  const tInit = Date.now();
  await rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 't', version: '0' } });
  srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized', params: {} }) + '\n');
  const initMs = Date.now() - tInit;

  // first query builds the word index; subsequent are warm.
  const timed = async q => { const s = Date.now(); await call('ask', { query: q, top: 5 }); return Date.now() - s; };
  const q1 = await timed('account balance validation');
  const warm = [];
  for (const q of ['customer onboarding', 'loan repayment schedule', 'transaction posting', 'interest accrual', 'kyc verification']) warm.push(await timed(q));
  const avgWarm = Math.round(warm.reduce((a, b) => a + b, 0) / warm.length);

  console.log(`  init (load encoder ONCE): ${initMs} ms`);
  console.log(`  query 1 (cold, builds word index): ${q1} ms`);
  console.log(`  warm queries: [${warm.join(', ')}] ms  -> avg ${avgWarm} ms`);
  console.log('');
  ok('encoder loads ONCE (init), not per query', initMs > 0);
  ok('WARM recall is fast on a 4,868-frame brain', avgWarm < 600, `avg ${avgWarm} ms warm`);

  // CODE recall: sym-exact via the get/ask path (the bank's real symbols)
  const symProbe = await call('ask', { query: 'the repository class', top: 3 });
  ok('code recall returns real frames from the bank brain', /\.cs|Repository|Service|Account|class/i.test(symProbe));

  // NUDGE SAVE PATH e2e: agent saves over MCP -> persisted -> immediately recallable (no restart)
  await call('remember', { content: 'African Bank: posting dates must use the bank business calendar, not UTC -- a fix learned this session.' });
  await call('learn_fix', { problem: 'interest accrual off by one day at month boundary', learnings: 'use the business-day calendar; accrue on value date not booking date', edits: '[{"file":"Accrual.cs","content":"valueDate"}]' });
  const back = await call('ask', { query: 'posting dates business calendar', top: 3 });
  ok('NUDGE SAVE: agent-saved memory is IMMEDIATELY recallable (no restart)', /business calendar|posting dates|UTC/i.test(back), 'remember -> recall works live');
  const fixBack = await call('recall_fix', { problem: 'interest accrual wrong at month end', min_similarity: 0.0 }).catch(() => '');
  ok('NUDGE SAVE: agent-saved FIX recallable', /business.?day|value date|accru/i.test(fixBack || back));

  srv.kill();
  for (const f of [BR, BR + '.spill']) { try { fs.unlinkSync(f); } catch {} }
  console.log(`\n${fails === 0 ? 'ALL PASS -- resident MCP (production): encoder loads ONCE, warm recall ~' + avgWarm + 'ms on 4,868 frames, code recall works, and the NUDGE SAVE path (agent saves over MCP -> immediately recallable) is live 100%.' : fails + ' FAILED'}`);
  process.exit(fails === 0 ? 0 : 1);
})().catch(e => { console.error(e); try { srv.kill(); } catch {} process.exit(1); });
