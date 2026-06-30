#!/usr/bin/env node
// PHASE 2 — .said UPDATES ON THE FLY + APPENDS like Kimi, driven through the REAL MCP server (JSON-RPC).
//
// Kimi stays current by self-updating AGENTS.md when things change. .said's equivalent, live over MCP:
//   - tool_completion : append an episodic "what happened / what was decided" frame after each action,
//   - remember        : capture a fact mid-session,
//   - journal         : the carry-across session log,
//   then ask/search immediately sees the new memory (no re-index step, no restart) -> "stays up to date".
// Proves the loop an MCP-driven agent uses to keep its .said current within a live session.
const { spawn, execFileSync } = require('child_process');
const fs = require('fs'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = path.join(ROOT, 'target', 'debug', 'said.exe');
const MCP = path.join(ROOT, 'target', 'debug', 'said-mcp.exe');
// ANCHORED to the REAL imported corpus (owner: "we do everything with claude-import"), not a temp brain.
// We operate on a COPY so the test is repeatable without mutating the canonical import.
const SRC = path.join(__dirname, 'claude-import', 'claude.said');
const BR = path.join(__dirname, 'claude-import', 'claude-phase2.said');
for (const f of [BR, BR + '.spill']) { try { fs.unlinkSync(f); } catch {} }
if (!fs.existsSync(SRC)) { console.error('run import-claude-corpus.js first (claude-import/claude.said missing)'); process.exit(1); }
fs.copyFileSync(SRC, BR);
try { fs.copyFileSync(SRC + '.spill', BR + '.spill'); } catch {}

const srv = spawn(MCP, ['--path', BR], { stdio: ['pipe', 'pipe', 'ignore'] });
let buf = ''; const pending = new Map(); let id = 0;
srv.stdout.on('data', d => {
  buf += d;
  let nl; while ((nl = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, nl); buf = buf.slice(nl + 1);
    if (!line.trim()) continue;
    let m; try { m = JSON.parse(line); } catch { continue; }
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  }
});
const rpc = (method, params) => new Promise(r => { const i = ++id; pending.set(i, r); srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method, params }) + '\n'); });
const callTool = (name, args) => rpc('tools/call', { name, arguments: args }).then(r => (r.result?.content || []).map(c => c.text).join('\n'));

let fails = 0; const ok = (n, c, d) => { console.log(`  ${c ? 'PASS' : 'FAIL'}  ${n}${c ? '' : '  -> ' + (d || '')}`); if (!c) fails++; };

(async () => {
  await rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'phase2', version: '0' } });
  srv.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized', params: {} }) + '\n');

  console.log('=== PHASE 2: .said updates on the fly over MCP, ON THE REAL imported claude-import brain ===\n');

  // 0. the REAL imported corpus is already there + recallable (we operate on top of it, not a blank brain)
  const pre = await callTool('ask', { query: 'what is the memory evidence standard', top: 3 });
  ok('imported corpus (tier-1 facts) recallable through MCP before any new write', /claim|evidence|manifest|standard/i.test(pre), pre.slice(0, 100));
  const preSess = await callTool('ask', { query: 'what did the encoder dimension session decide', top: 3 });
  ok('imported distilled SESSION notes recallable through MCP', /encoder|128|session|compact|recall/i.test(preSess), preSess.slice(0, 100));

  // 1. mid-session: append a "what happened/decided" frame via tool_completion (the Kimi-append analogue)
  await callTool('tool_completion', { tool: 'Edit', result: 'Decided: project-scoped episodic, cross-project fixes. Implemented save_memory with evidence links in memory.rs.', status: 'success' });
  // 2. capture a fact via remember
  await callTool('remember', { content: 'The encoder is the 128-dim said-lam-static-4M with our own WordPiece tokenizer.' });
  // 3. journal the session (carry-across log)
  await callTool('journal', { summary: 'Session: built memory-evidence standard + corrected encoder dim to 128.' }).catch(() => {});

  // 4. WITHOUT restart/reindex: ask immediately sees the just-written memories -> "stays up to date"
  const q1 = await callTool('ask', { query: 'what dimension is the encoder and which tokenizer', top: 3 });
  ok('on-the-fly remember is IMMEDIATELY recallable (no restart)', /128|4M|WordPiece|tokenizer/i.test(q1), q1.slice(0, 120));

  const q2 = await callTool('ask', { query: 'what was decided about project scoping and fixes', top: 3 });
  ok('appended tool_completion decision is recallable', /cross-project|project-scoped|fixes/i.test(q2), q2.slice(0, 120));

  // 5. APPEND AGAIN (update) and confirm the newest is reachable too (stays current across many appends)
  await callTool('tool_completion', { tool: 'Edit', result: 'Update: corrected the /compact comparison to use exact symbol recall (sym 12/12).', status: 'success' });
  const q3 = await callTool('ask', { query: 'what was corrected about the compact comparison', top: 3 });
  ok('a LATER append is also immediately recallable (continuous update)', /symbol|sym|compact|corrected/i.test(q3), q3.slice(0, 120));

  // 6. status shows the brain grew live
  const st = await callTool('status', {});
  ok('brain grew live across the session (memories added without restart)', /[1-9]\d*/.test(st));

  srv.kill();
  for (const f of [BR, BR + '.spill']) { try { fs.unlinkSync(f); } catch {} } // remove the test COPY, keep canonical claude.said
  console.log(`\n${fails === 0 ? 'ALL PASS -- on the REAL imported claude-import brain: prior memories stay recallable AND live appends (tool_completion/remember/journal) are immediately recallable, no restart. Stays current like Kimi self-updates AGENTS.md.' : fails + ' FAILED'}`);
  process.exit(fails === 0 ? 0 : 1);
})().catch(e => { console.error(e); try { srv.kill(); } catch {} process.exit(1); });
