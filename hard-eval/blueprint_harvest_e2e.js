#!/usr/bin/env node
// Live MCP proof for harvest_blueprints: scan a REAL repo through the running said-mcp.exe, then recall a
// harvested blueprint. Separate JSON-RPC process. Encoder build required for semantic recall.
const { spawn, execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..');
const MCP = process.env.SAID_MCP || path.join(ROOT, 'target', 'debug', 'said-mcp.exe');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const SCAN_DIR = process.env.SCAN_DIR || path.join(ROOT, 'crates', 'sca-core', 'src');

const BRAIN = path.join(os.tmpdir(), `bp_harvest_${process.pid}.said`);
for (const f of [BRAIN, BRAIN + '.spill']) { try { fs.unlinkSync(f); } catch {} }
execFileSync(SAID, ['create', BRAIN]);

const srv = spawn(MCP, ['--path', BRAIN], { stdio: ['pipe', 'pipe', 'ignore'] });
let buf = ''; const pending = new Map(); let idc = 0;
srv.stdout.on('data', d => { buf += d.toString(); let nl;
  while ((nl = buf.indexOf('\n')) >= 0) { const line = buf.slice(0, nl); buf = buf.slice(nl + 1);
    if (!line.trim()) continue; let m; try { m = JSON.parse(line); } catch { continue; }
    if (m.id != null && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } } });
const rpc = (method, params) => new Promise(res => { const id = ++idc; pending.set(id, res);
  srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n'); });
const notify = (m, p) => srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: m, params: p }) + '\n');
const text = r => (r.result && r.result.content || []).map(c => c.text).join('\n');
let fails = 0; const ok = (n, c, d) => { console.log(`${c ? 'PASS' : 'FAIL'}  ${n}${c ? '' : '  -> ' + d}`); if (!c) fails++; };

(async () => {
  await rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'h', version: '0' } });
  notify('notifications/initialized', {});

  const list = await rpc('tools/list', {});
  ok('tools/list exposes harvest_blueprints', (list.result.tools || []).some(t => t.name === 'harvest_blueprints'));

  console.log(`\nharvesting ${SCAN_DIR} ...`);
  const h = await rpc('tools/call', { name: 'harvest_blueprints', arguments: { dir: SCAN_DIR } });
  const ht = text(h);
  console.log(ht.split('\n').slice(0, 14).map(l => '  ' + l).join('\n'));
  ok('harvest learned >=1 blueprint', /Harvested [1-9]/.test(ht), ht);

  // a harvested shape should now be recallable. Query a shape we KNOW was harvested ("new<Entity>"),
  // min_score 0 so any match returns -- this proves recall reaches the harvested store, end to end.
  const r = await rpc('tools/call', { name: 'recall_blueprint', arguments: { shape: 'new<Entity>', min_score: 0.0 } });
  ok('a harvested blueprint is recallable', /<Entity>|Sections|sections/.test(text(r)), text(r));

  srv.kill();
  for (const f of [BRAIN, BRAIN + '.spill']) { try { fs.unlinkSync(f); } catch {} }
  console.log(fails === 0 ? '\nALL HARVEST MCP CHECKS PASSED' : `\n${fails} FAILED`);
  process.exit(fails === 0 ? 0 : 1);
})();
