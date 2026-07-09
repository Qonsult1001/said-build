#!/usr/bin/env node
// BEFORE/AFTER blueprint test, driven live against said-mcp.exe (separate JSON-RPC process).
//
//  SCENARIO A — NEW project (empty brain):
//    recall_blueprint -> MISS (no blueprint -> agent derives it) -> learn_blueprint (first one) ->
//    recall again -> HIT (next entity REUSES it, doesn't recreate).
//
//  SCENARIO B — EXISTING project (brain already holds a blueprint):
//    recall_blueprint -> HIT immediately (OLD blueprint reused) -> user prefers a different way ->
//    learn_blueprint again = KEEP-FIRST no-op (OLD stands) -> learn_blueprint promote=true ->
//    recall -> NEW blueprint supersedes. Shows old-vs-new + the exact MCP reactions.
const { spawn, execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');

const ROOT = path.join(__dirname, '..');
const MCP = process.env.SAID_MCP || path.join(ROOT, 'target', 'debug', 'said-mcp.exe');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');

function mcpSession(brainPath) {
  const srv = spawn(MCP, ['--path', brainPath], { stdio: ['pipe', 'pipe', 'inherit'] });
  let buf = ''; const pending = new Map(); let idc = 0;
  srv.stdout.on('data', d => {
    buf += d.toString(); let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl); buf = buf.slice(nl + 1);
      if (!line.trim()) continue;
      let m; try { m = JSON.parse(line); } catch { continue; }
      if (m.id != null && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
    }
  });
  const rpc = (method, params) => new Promise(res => {
    const id = ++idc; pending.set(id, res);
    srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
  const notify = (method, params) => srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
  return { srv, rpc, notify };
}
const text = r => (r.result && r.result.content || []).map(c => c.text).join('\n');
const call = (s, name, args) => s.rpc('tools/call', { name, arguments: args });

const SECTIONS_OLD = '{"sections":["accept-and-audit","guards","save","response"]}';
const SECTIONS_NEW = '{"sections":["accept-and-audit","idempotency","guards","save","response","emit-event"]}';

async function init(s) {
  await s.rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'bp-test', version: '0' } });
  s.notify('notifications/initialized', {});
}
function freshBrain(name) {
  const p = path.join(os.tmpdir(), `bp_${name}_${process.pid}.said`);
  for (const f of [p, p + '.spill']) { try { fs.unlinkSync(f); } catch {} }
  execFileSync(SAID, ['create', p]);
  return p;
}
function banner(t) { console.log('\n' + '='.repeat(78) + '\n' + t + '\n' + '='.repeat(78)); }
function react(label, r) { console.log(`\n[${label}] MCP reaction:\n  ` + text(r).split('\n').join('\n  ')); }

(async () => {
  const SHAPE = 'Create<Entity> REST endpoint';
  const QUERY = 'create endpoint for an entity';

  // ---------------- SCENARIO A: NEW project ----------------
  banner('SCENARIO A — NEW project (empty brain): no blueprint yet');
  const aBrain = freshBrain('new');
  const a = mcpSession(aBrain); await init(a);

  console.log('\nAgent is about to build the FIRST Create endpoint. It asks .said first:');
  react('A1 recall (empty)', await call(a, 'recall_blueprint', { shape: QUERY, min_score: 0.0 }));
  console.log('  -> agent gets NO blueprint, so it derives the structure itself, builds + verifies, then saves:');
  react('A2 learn (first)', await call(a, 'learn_blueprint', { shape: SHAPE, sections: SECTIONS_OLD }));
  console.log('\nLater, a SECOND entity of the same shape. Agent asks again BEFORE building:');
  react('A3 recall (now populated)', await call(a, 'recall_blueprint', { shape: QUERY, min_score: 0.0 }));
  console.log('  -> HIT: agent RENDERS these sections in the active language, writes only the 20%. No recreation.');
  a.srv.kill();

  // ---------------- SCENARIO B: EXISTING project ----------------
  banner('SCENARIO B — EXISTING project: brain ALREADY holds the (old) blueprint');
  const bBrain = freshBrain('existing');
  // seed the existing brain via CLI (simulating prior sessions)
  execFileSync(SAID, ['--path', bBrain, 'learn-blueprint', '--shape', SHAPE, '--sections', SECTIONS_OLD]);
  const b = mcpSession(bBrain); await init(b);

  console.log('\nAgent starts work on this established project and asks .said:');
  react('B1 recall (OLD reused)', await call(b, 'recall_blueprint', { shape: QUERY, min_score: 0.0 }));
  console.log('  -> the OLD blueprint comes straight back (reused across sessions).');

  console.log('\nThe user does not like how it works and wants a DIFFERENT structure (adds idempotency + emit-event).');
  console.log('First the agent just tries to learn the new way (no promote):');
  react('B2 learn again = KEEP-FIRST', await call(b, 'learn_blueprint', { shape: SHAPE, sections: SECTIONS_NEW }));
  react('B2b recall (still OLD)', await call(b, 'recall_blueprint', { shape: QUERY, min_score: 0.0 }));
  console.log('  -> keep-first: the OLD blueprint still stands (a throwaway edit cannot silently become the standard).');

  console.log('\nThe user confirms: make this the NEW STANDARD. Agent promotes:');
  react('B3 promote', await call(b, 'learn_blueprint', { shape: SHAPE, sections: SECTIONS_NEW, verified: true }));
  react('B3b recall (NEW supersedes)', await call(b, 'recall_blueprint', { shape: QUERY, min_score: 0.0 }));
  console.log('  -> NEW blueprint now reused for every future entity. OLD vs NEW resolved by an explicit promote.');
  b.srv.kill();

  for (const p of [aBrain, bBrain]) for (const f of [p, p + '.spill']) { try { fs.unlinkSync(f); } catch {} }
  banner('DONE — compare A (none->created->reused) vs B (old reused -> keep-first -> promote -> new)');
  process.exit(0);
})();
