#!/usr/bin/env node
// Live MCP proof for the blueprint tools, driven as a SEPARATE PROCESS over JSON-RPC stdio.
// initialize -> tools/list (assert learn_blueprint+recall_blueprint present) -> learn -> recall ->
// keep-first no-op -> promote -> recall (superseded). Mirrors mcp_e2e.js.
const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const EXE = process.env.SAID_MCP || path.join(__dirname, '..', 'target', 'debug', 'said-mcp.exe');
const BRAIN = path.join(os.tmpdir(), `bp_mcp_${process.pid}.said`);
for (const f of [BRAIN, BRAIN + '.spill']) { try { fs.unlinkSync(f); } catch {} }

// create the brain via the CLI sibling (MCP opens an existing file)
const SAID = process.env.SAID_CLI || path.join(__dirname, '..', 'target', 'debug', 'said.exe');
require('child_process').execFileSync(SAID, ['create', BRAIN]);

const srv = spawn(EXE, ['--path', BRAIN], { stdio: ['pipe', 'pipe', 'inherit'] });
let buf = '';
const pending = new Map();
let idc = 0;
srv.stdout.on('data', d => {
  buf += d.toString();
  let nl;
  while ((nl = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, nl); buf = buf.slice(nl + 1);
    if (!line.trim()) continue;
    let msg; try { msg = JSON.parse(line); } catch { continue; }
    if (msg.id != null && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
  }
});
function rpc(method, params) {
  const id = ++idc;
  return new Promise(res => {
    pending.set(id, res);
    srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
}
function notify(method, params) {
  srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
}
const text = r => (r.result && r.result.content || []).map(c => c.text).join('\n');
let failures = 0;
function check(name, cond, detail) {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${cond ? '' : '  -> ' + detail}`);
  if (!cond) failures++;
}

(async () => {
  await rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'bp-e2e', version: '0' } });
  notify('notifications/initialized', {});

  const list = await rpc('tools/list', {});
  const names = (list.result.tools || []).map(t => t.name);
  check('tools/list exposes learn_blueprint', names.includes('learn_blueprint'), names.join(','));
  check('tools/list exposes recall_blueprint', names.includes('recall_blueprint'), names.join(','));

  const shape = 'Create<Entity> REST endpoint';
  const learn = await rpc('tools/call', { name: 'learn_blueprint', arguments: {
    shape, sections: '{"sections":["accept-and-audit","idempotency","guards","save","response"]}' } });
  check('learn_blueprint stores', /Blueprint shape::.* learned/.test(text(learn)), text(learn));

  const recall = await rpc('tools/call', { name: 'recall_blueprint', arguments: { shape: 'create endpoint for an entity', min_score: 0.0 } });
  check('recall_blueprint returns it', /idempotency/.test(text(recall)), text(recall));

  const keep = await rpc('tools/call', { name: 'learn_blueprint', arguments: {
    shape, sections: '{"sections":["DIFFERENT"]}' } });
  check('keep-first no-op on same shape', /kept-first/.test(text(keep)), text(keep));

  const recall2 = await rpc('tools/call', { name: 'recall_blueprint', arguments: { shape: 'create endpoint for an entity', min_score: 0.0 } });
  check('original survived keep-first', /idempotency/.test(text(recall2)) && !/DIFFERENT/.test(text(recall2)), text(recall2));

  const promote = await rpc('tools/call', { name: 'learn_blueprint', arguments: {
    shape, sections: '{"sections":["my-preferred-way"]}', promote: true } });
  check('promote supersedes', /promoted/.test(text(promote)), text(promote));

  const recall3 = await rpc('tools/call', { name: 'recall_blueprint', arguments: { shape: 'create endpoint for an entity', min_score: 0.0 } });
  check('recall shows promoted sections', /my-preferred-way/.test(text(recall3)), text(recall3));

  srv.kill();
  for (const f of [BRAIN, BRAIN + '.spill']) { try { fs.unlinkSync(f); } catch {} }
  console.log(failures === 0 ? '\nALL BLUEPRINT MCP CHECKS PASSED' : `\n${failures} CHECK(S) FAILED`);
  process.exit(failures === 0 ? 0 : 1);
})();
