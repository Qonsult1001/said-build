#!/usr/bin/env node
// Proves the PRODUCT behaviour (encoder build): (1) SAID_PROJECT auto-derived by the MCP server from the
// brain filename -> per-project scoping with zero config; (2) SEMANTIC recall (a strong paraphrase that
// shares no words with the shape still hits); (3) cross-project isolation (vivere's blueprint does not
// leak into said-build). Driven live against said-mcp.exe (separate JSON-RPC process).
const { spawn, execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..');
const MCP = process.env.SAID_MCP || path.join(ROOT, 'target', 'debug', 'said-mcp.exe');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');

function session(brainPath) {
  const srv = spawn(MCP, ['--path', brainPath], { stdio: ['pipe', 'pipe', 'ignore'] });
  let buf = ''; const pending = new Map(); let idc = 0;
  srv.stdout.on('data', d => { buf += d.toString(); let nl;
    while ((nl = buf.indexOf('\n')) >= 0) { const line = buf.slice(0, nl); buf = buf.slice(nl + 1);
      if (!line.trim()) continue; let m; try { m = JSON.parse(line); } catch { continue; }
      if (m.id != null && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } } });
  const rpc = (method, params) => new Promise(res => { const id = ++idc; pending.set(id, res);
    srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n'); });
  const notify = (m, p) => srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: m, params: p }) + '\n');
  return { srv, rpc, notify };
}
const text = r => (r.result && r.result.content || []).map(c => c.text).join('\n');
const call = (s, n, a) => s.rpc('tools/call', { name: n, arguments: a });
async function init(s){ await s.rpc('initialize',{protocolVersion:'2024-11-05',capabilities:{},clientInfo:{name:'t',version:'0'}}); s.notify('notifications/initialized',{}); }
function brain(name){ const p = path.join(os.tmpdir(), name); for(const f of [p,p+'.spill']) {try{fs.unlinkSync(f);}catch{}} execFileSync(SAID,['create',p]); return p; }
let fails = 0; const ok=(n,c,d)=>{console.log(`${c?'PASS':'FAIL'}  ${n}${c?'':'  -> '+d}`); if(!c)fails++;};

(async () => {
  const SHAPE = 'Create<Entity> REST endpoint';
  // A paraphrase that shares NO content words with the shape -> only semantic recall can match it.
  const PARAPHRASE = 'scaffold a new resource insertion handler with audit and validation';

  // brains named by PROJECT so the MCP server auto-derives SAID_PROJECT from the filename.
  const vivere = brain(`vivere_${process.pid}.said`);
  const saidbuild = brain(`said-build_${process.pid}.said`);

  // seed vivere with a blueprint (caller does NOT set SAID_PROJECT -> the MCP server must)
  const v = session(vivere); await init(v);
  await call(v, 'learn_blueprint', { shape: SHAPE, sections: '{"sections":["accept-and-audit","idempotency","guards","save","response"]}' });

  // (1) auto SAID_PROJECT + (2) SEMANTIC recall: a no-shared-words paraphrase still hits in vivere.
  const sem = await call(v, 'recall_blueprint', { shape: PARAPHRASE, min_score: 0.0 });
  ok('semantic recall on a no-shared-words paraphrase', /idempotency/.test(text(sem)), text(sem));

  // (3) cross-project isolation: said-build has NO blueprint; the same query must NOT pull vivere's.
  const sb = session(saidbuild); await init(sb);
  const leak = await call(sb, 'recall_blueprint', { shape: PARAPHRASE, min_score: 0.0 });
  ok('cross-project isolation (said-build does not see vivere blueprint)', /No known blueprint/.test(text(leak)), text(leak));

  // and said-build can learn its OWN, independently
  await call(sb, 'learn_blueprint', { shape: SHAPE, sections: '{"sections":["accept","persist"]}' });
  const own = await call(sb, 'recall_blueprint', { shape: PARAPHRASE, min_score: 0.0 });
  ok('said-build recalls its OWN blueprint', /persist/.test(text(own)) && !/idempotency/.test(text(own)), text(own));

  console.log('\n--- the reactions verbatim ---');
  console.log('[vivere semantic recall]\n  ' + text(sem).split('\n').join('\n  '));
  console.log('[said-build isolation]\n  ' + text(leak).split('\n').join('\n  '));
  console.log('[said-build own]\n  ' + text(own).split('\n').join('\n  '));

  v.srv.kill(); sb.srv.kill();
  for (const p of [vivere, saidbuild]) for (const f of [p, p+'.spill']) { try { fs.unlinkSync(f); } catch {} }
  console.log(fails === 0 ? '\nALL PROJECT/SEMANTIC CHECKS PASSED' : `\n${fails} FAILED`);
  process.exit(fails === 0 ? 0 : 1);
})();
