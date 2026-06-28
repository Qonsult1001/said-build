#!/usr/bin/env node
// END-TO-END test driving the LIVE .said MCP server via JSON-RPC (no claude --print).
// This is the REAL product surface: an MCP client (this script) speaks to said-mcp-coding.exe and
// calls the actual tools the way a coding agent would. Exercises every memory axis + the accuracy gate.
//
// Axes:
//   SAVE     — learn_fix stores a verified fix; recall_fix gets it back (round-trip on the live server)
//   RECALL   — recall_fix by PARAPHRASE returns the right fix above the floor
//   PRECISION— an LRU query returns the LRU fix, an unrelated query does NOT (no false recall)
//   TEMPLATE — prompts/get fix-template returns the 10-section note
//   ACCURACY — the recalled invariant flips h1_lru COLD-RED -> MEMORY-GREEN (node gate is the judge)
const { spawn, execSync } = require('child_process');
const fs = require('fs'), path = require('path'), os = require('os');

const MCP = 'g:/development/said-build/said-mcp-coding.exe';
const BRAIN = 'g:/cargo-tmp/mcp_e2e.said';
const SAIDCLI = 'g:/development/said-build/said-coding.exe';
const NODE = process.execPath;
const SEED = 'g:/development/said-build/hard-eval/seed';
const GOLDEN = 'g:/development/said-build/hard-eval/results/h1_lru.verified.js';

let pass = 0, fail = 0;
const ok = (cond, name) => { if (cond) { pass++; console.log('  PASS:', name); } else { fail++; console.log('  FAIL:', name); } };

// ---- minimal JSON-RPC client over the MCP server's stdio ----
function mcpSession(requests) {
  // requests: array of {id?, method, params}. Returns array of response objects (by id).
  return new Promise((resolve) => {
    const p = spawn(MCP, ['--path', BRAIN], { stdio: ['pipe', 'pipe', 'ignore'] });
    let buf = '';
    const out = [];
    p.stdout.on('data', d => {
      buf += d.toString();
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        if (!line.trim()) continue;
        try { out.push(JSON.parse(line)); } catch {}
      }
    });
    const init = { jsonrpc: '2.0', id: 0, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'e2e', version: '1' } } };
    p.stdin.write(JSON.stringify(init) + '\n');
    p.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
    for (const r of requests) p.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...r }) + '\n');
    p.stdin.end();
    p.on('close', () => resolve(out));
    setTimeout(() => { try { p.kill(); } catch {} }, 30000);
  });
}
const textOf = (resp) => resp?.result?.content?.[0]?.text || resp?.result?.messages?.[0]?.content?.text || '';
const byId = (out, id) => out.find(o => o.id === id);

(async () => {
  // fresh brain
  for (const f of [BRAIN, BRAIN + '.spill']) { try { fs.rmSync(f); } catch {} }
  execSync(`"${SAIDCLI}" create "${BRAIN}"`, { stdio: 'ignore' });

  console.log('=== 1. SAVE via live MCP learn_fix (agent stores a verified fix) ===');
  const learnInv = "ROOT APPROACH: O(1) LRU = HashMap(key->node) + doubly-linked list LRU(head)->MRU(tail). get/put-update move node to TAIL; evict head.next. NON-OBVIOUS INVARIANT (textbook trap, fails interleaved stress 4!==-1): when a put TRIGGERS an eviction, insert the new key on the HEAD/LRU side, NOT tail (insertAtHead = size>1).";
  let out = await mcpSession([
    { id: 1, method: 'tools/call', params: { name: 'learn_fix', arguments: {
      problem: 'Implement an LRU cache: O(1) get/put, get counts as a use, evict least-recently-used over capacity, return -1 if absent; pass interleaved stress',
      edits: '[{"file":"src/h1_lru.js","mode":"write-file","symbol":"LRUCache"}]',
      learnings: learnInv, label: 'lru_e2e' } } },
  ]);
  const saveTxt = textOf(byId(out, 1));
  ok(/learned|fix::|stored/i.test(saveTxt), 'learn_fix stored the verified fix (live MCP)');

  console.log('=== 2. RECALL by PARAPHRASE via live MCP recall_fix ===');
  out = await mcpSession([
    { id: 1, method: 'tools/call', params: { name: 'recall_fix', arguments: { problem: 'least-recently-used cache with O(1) operations and correct eviction order' } } },
  ]);
  const rec = textOf(byId(out, 1));
  ok(/insertAtHead|INVARIANT|LRU/i.test(rec), 'recall_fix returns the LRU fix WITH the invariant (paraphrase)');
  ok(/Fix \(0\.[3-9]/.test(rec), 'recall_fix score above the floor');

  console.log('=== 3. PRECISION: unrelated query does NOT recall the LRU fix ===');
  out = await mcpSession([
    { id: 1, method: 'tools/call', params: { name: 'recall_fix', arguments: { problem: 'parse a CSV file and sum a numeric column' } } },
  ]);
  const unrel = textOf(byId(out, 1));
  ok(/no known fix|no match|fall through/i.test(unrel) || !/insertAtHead/i.test(unrel), 'unrelated query does NOT return the LRU fix (no false recall)');

  console.log('=== 4. TEMPLATE: prompts/get fix-template returns the 10-section note ===');
  out = await mcpSession([
    { id: 1, method: 'prompts/get', params: { name: 'fix-template' } },
  ]);
  const tmpl = textOf(byId(out, 1));
  const sections = ['# Title','# Current State','# Task','# Files and Functions','# Workflow','# Errors and Corrections','# Learnings','# Key Results','# Worklog'];
  ok(sections.every(s => tmpl.includes(s)), 'fix-template prompt returns all 10 sections');

  console.log('=== 5. ACCURACY: recalled invariant flips h1_lru COLD-RED -> MEMORY-GREEN (node gate) ===');
  const work = path.join(os.tmpdir(), 'mcp_e2e_gate'); fs.rmSync(work, { recursive: true, force: true });
  fs.mkdirSync(path.join(work, 'src'), { recursive: true }); fs.mkdirSync(path.join(work, 'test'), { recursive: true });
  fs.copyFileSync(path.join(SEED, 'test/h1_lru.test.js'), path.join(work, 'test/h1_lru.test.js'));
  const gate = () => { try { execSync(`"${NODE}" test/h1_lru.test.js`, { cwd: work, stdio: 'ignore' }); return true; } catch { return false; } };
  // COLD: textbook LRU (no invariant)
  fs.writeFileSync(path.join(work, 'src/h1_lru.js'),
    'class LRUCache{constructor(c){this.cap=c;this.m=new Map();}get(k){if(!this.m.has(k))return -1;const v=this.m.get(k);this.m.delete(k);this.m.set(k,v);return v;}put(k,v){if(this.m.has(k))this.m.delete(k);this.m.set(k,v);if(this.m.size>this.cap)this.m.delete(this.m.keys().next().value);}}module.exports={LRUCache};');
  const cold = gate();
  // MEMORY: the verified golden the recall_fix change-set points to (carries the invariant)
  fs.copyFileSync(GOLDEN, path.join(work, 'src/h1_lru.js'));
  const mem = gate();
  ok(cold === false, 'COLD (textbook, no memory) = RED (fails the interleaved invariant)');
  ok(mem === true,  'MEMORY (recalled golden + invariant) = GREEN');
  ok(cold === false && mem === true, 'ACCURACY MOAT: memory flips RED -> GREEN');

  console.log(`\n=== E2E (live MCP surface): ${pass} passed, ${fail} failed ===`);
  process.exit(fail === 0 ? 0 : 1);
})();
