#!/usr/bin/env node
// PHASE 1 — import Claude's REAL corpus into .said NATIVELY, KEEP/DROP filtered (doc 29).
//
// Source (this machine): C:\Users\Carter\.claude\projects\g--development-said-build
//   - memory/*.md      : 25 curated CLAIM+EVIDENCE records (~120 KB) -> imported IN FULL as memory frames.
//   - *.jsonl          : ~131 MB of transcripts -> KEEP only the decision/learning slices (user requests +
//                        assistant text conclusions); DROP raw tool calls, attachments, snapshots, queue
//                        ops (disposable noise). Each kept slice becomes an Episodic frame.
// Output: one .said brain. Memory records keep their evidence as link: edges (commit/file/concept), so the
// OKF graph (default-on) makes the evidence reachable -- native import, no markdown re-store hack.
const { execFileSync } = require('child_process');
const fs = require('fs'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const CLAUDE_DIR = process.env.CLAUDE_DIR || 'C:\\Users\\Carter\\.claude\\projects\\g--development-said-build';
const OUT = path.join(__dirname, 'claude-import', 'claude.said');
fs.mkdirSync(path.dirname(OUT), { recursive: true });
for (const f of [OUT, OUT + '.spill']) { try { fs.unlinkSync(f); } catch {} }
execFileSync(SAID, ['create', OUT]);
const env = { ...process.env, SAID_PROJECT: 'claude-corpus', SAID_OKF_LINKS: '1' };
const run = (...a) => execFileSync(SAID, ['--path', OUT, ...a], { encoding: 'utf8', env, maxBuffer: 1 << 26 });

// ---- 1. memory records (claim + evidence) -> memory frames, evidence as link: edges ----
const memDir = path.join(CLAUDE_DIR, 'memory');
let memCount = 0;
const mdFiles = fs.existsSync(memDir) ? fs.readdirSync(memDir).filter(f => f.endsWith('.md') && f !== 'MEMORY.md') : [];
for (const file of mdFiles) {
  const body = fs.readFileSync(path.join(memDir, file), 'utf8');
  // parse frontmatter name/description/type
  const fm = /^---\n([\s\S]*?)\n---/.exec(body);
  const meta = {};
  if (fm) for (const line of fm[1].split('\n')) { const m = /^(\w+):\s*(.+)$/.exec(line.trim()); if (m) meta[m[1]] = m[2].replace(/^["']|["']$/g, ''); }
  const name = meta.name || file.replace(/\.md$/, '');
  const desc = (meta.description || name).slice(0, 200);
  const mtype = ['user', 'feedback', 'project', 'reference'].includes(meta.type) ? meta.type : 'project';
  const claim = body.replace(/^---\n[\s\S]*?\n---\n/, '').trim();
  // evidence = commit hashes + file refs + [[wikilinks]] mentioned in the body
  const ev = new Set();
  for (const m of claim.matchAll(/\b([0-9a-f]{7,40})\b/g)) ev.add(m[1]);             // commit hashes
  for (const m of claim.matchAll(/\b([a-z_]+\.rs|[A-Za-z_]+\.md|[a-z_]+\.js)\b/g)) ev.add(m[1]); // file refs
  for (const m of claim.matchAll(/\[\[([a-z0-9-]+)\]\]/g)) ev.add(m[1]);             // wikilinks
  const evArgs = [...ev].slice(0, 12).flatMap(e => ['--evidence', e]);
  const claimFile = path.join(path.dirname(OUT), `_claim_${memCount}.txt`);
  fs.writeFileSync(claimFile, claim);
  run('save-memory', '--name', name, '--description', desc, '--mtype', mtype, '--claim-file', claimFile, ...evArgs);
  fs.unlinkSync(claimFile);
  memCount++;
}

// ---- 2. transcripts -> KEEP only decisions/learnings (drop raw noise) ----
const transcripts = fs.readdirSync(CLAUDE_DIR).filter(f => f.endsWith('.jsonl'));
let keptSlices = 0, droppedEvents = 0, scannedBytes = 0;
const sliceDir = path.join(path.dirname(OUT), 'slices');
fs.mkdirSync(sliceDir, { recursive: true });
for (const t of transcripts) {
  const full = path.join(CLAUDE_DIR, t);
  scannedBytes += fs.statSync(full).size;
  const lines = fs.readFileSync(full, 'utf8').split('\n');
  let buf = [], n = 0;
  for (const l of lines) {
    if (!l.trim()) continue;
    let o; try { o = JSON.parse(l); } catch { continue; }
    // KEEP: user text requests + assistant text conclusions. DROP everything else.
    let text = null, role = null;
    if (o.type === 'user' && typeof o.message?.content === 'string') { text = o.message.content; role = 'USER'; }
    else if (o.type === 'assistant' && Array.isArray(o.message?.content)) {
      const t2 = o.message.content.filter(c => c.type === 'text').map(c => c.text).join('\n');
      if (t2.trim()) { text = t2; role = 'ASSISTANT'; }
    }
    if (!text) { droppedEvents++; continue; }
    // skip pure tool-echo / command noise, keep substantive decisions/learnings
    if (text.length < 40 || text.startsWith('<') || text.startsWith('Caveat:')) { droppedEvents++; continue; }
    buf.push(`${role}: ${text.slice(0, 1200)}`);
    n++;
  }
  if (buf.length) {
    // one frame per ~20 kept slices (a session "chapter") so recall returns a coherent chunk
    for (let i = 0; i < buf.length; i += 20) {
      const chunk = buf.slice(i, i + 20).join('\n\n');
      const sf = path.join(sliceDir, `${t}_${i}.txt`);
      fs.writeFileSync(sf, chunk);
      keptSlices++;
    }
  }
}
// bulk-ingest the kept transcript slices as Episodic (the KEEP-filtered decision/learning record)
if (keptSlices) run('init', sliceDir);

const stats = run('stats');
const mem = (/Memories:\s+(\d+)/.exec(stats) || [])[1] || '?';
console.log('=== PHASE 1: Claude corpus imported into .said (KEEP-filtered) ===\n');
console.log(`  memory records (claim+evidence):  ${memCount} frames`);
console.log(`  transcript bytes scanned:         ${(scannedBytes / 1e6).toFixed(1)} MB`);
console.log(`  transcript events DROPPED (noise): ${droppedEvents}`);
console.log(`  transcript KEEP slices ingested:  ${keptSlices}`);
console.log(`  total .said memories:             ${mem}`);
console.log(`  brain: ${OUT}`);
const concepts = run('list-concepts').split('\n').find(l => /distinct/.test(l)) || '';
console.log(`  ${concepts.trim()}`);
console.log(`\n  KEEP/DROP working: dropped ${droppedEvents} raw events, kept ${keptSlices} decision/learning slices + ${memCount} curated records.`);
