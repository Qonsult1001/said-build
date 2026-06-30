#!/usr/bin/env node
// PHASE 1 (v2, doc-29-faithful) — import Claude's REAL corpus into .said as 3 TIERS, richest-first.
//
// Doc 29 (research-grounded: Mem0 two-stage extract+consolidate; Zep/Letta episodic+semantic; A-Mem links;
// MemoryBank decay) prescribes EXACTLY:
//   TIER 1 — distilled facts (memory/*.md) -> memory frames with evidence links. Highest signal, no LLM.
//   TIER 2 — session episode -> ONE distilled Episodic note PER SESSION ("wanted / decided / built /
//            blockers / next"), via an extract+consolidate pass. The agent driving this IS the BYO-LLM
//            (same pattern as the LoCoMo oracle / dream-v3): we EXTRACT the user requests (decisions) +
//            the assistant's substantive conclusions, DEDUP near-duplicates, and write ONE consolidated
//            note per session -- NOT raw slices.
//   TIER 3 — raw transcript -> NOT stored (disposable; the privacy + distil-not-dump win).
// Tags: project:<name>, source:claude, session:<id>. The v1 of this script WRONGLY dumped 545 raw slices
// (tier-3); this v2 distills to ONE note per session per the documented design.
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

// ---- TIER 1: distilled facts (memory/*.md) -> memory frames with evidence links ----
const memDir = path.join(CLAUDE_DIR, 'memory');
let tier1 = 0;
const mdFiles = fs.existsSync(memDir) ? fs.readdirSync(memDir).filter(f => f.endsWith('.md') && f !== 'MEMORY.md') : [];
for (const file of mdFiles) {
  const body = fs.readFileSync(path.join(memDir, file), 'utf8');
  const fm = /^---\n([\s\S]*?)\n---/.exec(body); const meta = {};
  if (fm) for (const line of fm[1].split('\n')) { const m = /^(\w+):\s*(.+)$/.exec(line.trim()); if (m) meta[m[1]] = m[2].replace(/^["']|["']$/g, ''); }
  const name = meta.name || file.replace(/\.md$/, '');
  const desc = (meta.description || name).slice(0, 200);
  const mtype = ['user', 'feedback', 'project', 'reference'].includes(meta.type) ? meta.type : 'project';
  const claim = body.replace(/^---\n[\s\S]*?\n---\n/, '').trim();
  const ev = new Set();
  for (const m of claim.matchAll(/\b([0-9a-f]{7,40})\b/g)) ev.add(m[1]);
  for (const m of claim.matchAll(/\b([a-z_]+\.rs|[A-Za-z_]+\.md|[a-z_]+\.js)\b/g)) ev.add(m[1]);
  for (const m of claim.matchAll(/\[\[([a-z0-9-]+)\]\]/g)) ev.add(m[1]);
  const evArgs = [...ev].slice(0, 12).flatMap(e => ['--evidence', e]);
  const cf = path.join(path.dirname(OUT), `_c${tier1}.txt`); fs.writeFileSync(cf, claim);
  run('save-memory', '--name', name, '--description', desc, '--mtype', mtype, '--claim-file', cf, ...evArgs);
  fs.unlinkSync(cf); tier1++;
}

// ---- TIER 2: ONE distilled, consolidated Episodic note per SESSION (extract + consolidate) ----
// helper: dedup near-identical lines (cheap consolidation — Mem0 stage 2 at the line level)
function consolidate(lines) {
  const seen = new Set(); const out = [];
  for (const l of lines) {
    const key = l.toLowerCase().replace(/[^a-z0-9 ]/g, '').split(/\s+/).slice(0, 12).join(' ');
    if (key.length < 8 || seen.has(key)) continue;
    seen.add(key); out.push(l);
  }
  return out;
}
const transcripts = fs.readdirSync(CLAUDE_DIR).filter(f => f.endsWith('.jsonl'));
let tier2 = 0, droppedRaw = 0, scannedBytes = 0, distilledFromEvents = 0;
const noteDir = path.join(path.dirname(OUT), 'session-notes');
fs.mkdirSync(noteDir, { recursive: true });
for (const t of transcripts) {
  scannedBytes += fs.statSync(path.join(CLAUDE_DIR, t)).size;
  const lines = fs.readFileSync(path.join(CLAUDE_DIR, t), 'utf8').split('\n');
  const wanted = [], decided = [];
  for (const l of lines) {
    if (!l.trim()) continue; let o; try { o = JSON.parse(l); } catch { continue; }
    if (o.type === 'user' && typeof o.message?.content === 'string') {
      const c = o.message.content.trim();
      if (c.length >= 40 && !c.startsWith('<') && !c.startsWith('Caveat:')) wanted.push(c.replace(/\s+/g, ' ').slice(0, 300));
      else droppedRaw++;
      distilledFromEvents++;
    } else if (o.type === 'assistant' && Array.isArray(o.message?.content)) {
      const txt = o.message.content.filter(c => c.type === 'text').map(c => c.text).join(' ').trim();
      // KEEP only assistant lines that read like a DECISION/CONCLUSION (the durable invariant), drop chatter
      if (/\b(decided|root cause|fix|proven|measured|committed|the moat|conclusion|lesson|because)\b/i.test(txt) && txt.length >= 60)
        decided.push(txt.replace(/\s+/g, ' ').slice(0, 300));
      else droppedRaw++;
      distilledFromEvents++;
    } else { droppedRaw++; }
  }
  const w = consolidate(wanted).slice(0, 15);
  const d = consolidate(decided).slice(0, 25);
  if (!w.length && !d.length) continue;                      // empty session -> nothing to import
  const note = [
    `SESSION ${t.slice(0, 8)} (source:claude) -- distilled episode (one consolidated note, raw dropped).`,
    '', 'WANTED (the user requests/decisions):', ...w.map(x => '  - ' + x),
    '', 'DECIDED / BUILT (durable conclusions):', ...d.map(x => '  - ' + x),
  ].join('\n');
  const nf = path.join(noteDir, `session-${t.slice(0, 8)}.txt`); fs.writeFileSync(nf, note);
  // store as ONE Episodic memory-evidence frame (type project), tagged source/session via evidence links
  run('save-memory', '--name', `session-${t.slice(0, 8)}`, '--description',
      `claude session ${t.slice(0, 8)}: ${(d[0] || w[0] || '').slice(0, 120)}`,
      '--mtype', 'project', '--claim-file', nf, '--evidence', `session-${t.slice(0, 8)}`, '--evidence', 'source-claude');
  tier2++;
}

const stats = run('stats');
const mem = (/Memories:\s+(\d+)/.exec(stats) || [])[1] || '?';
const sizeMB = (fs.statSync(OUT).size / 1e6).toFixed(1);
console.log('=== PHASE 1 v2 (doc-29-faithful): Claude corpus -> 3 tiers, distil-not-dump ===\n');
console.log(`  TIER 1 distilled facts (claim+evidence):   ${tier1} frames`);
console.log(`  TIER 2 ONE consolidated note PER SESSION:  ${tier2} frames (from ${distilledFromEvents} events distilled)`);
console.log(`  TIER 3 raw transcript:                     NOT stored (${droppedRaw} raw events dropped, ${(scannedBytes/1e6).toFixed(1)} MB)`);
console.log(`  total .said memories:                      ${mem}  |  brain ${sizeMB} MB (from ${(scannedBytes/1e6).toFixed(1)} MB)`);
const concepts = (run('list-concepts').split('\n').find(l => /distinct/.test(l)) || '').trim();
console.log(`  ${concepts}`);
console.log(`\n  Per doc 29: tier-1 facts + tier-2 ONE distilled session note (extract+consolidate, Mem0 2-stage),`);
console.log(`  tier-3 raw DROPPED. This is distil-not-dump (the v1 of this script wrongly stored 545 raw slices).`);
