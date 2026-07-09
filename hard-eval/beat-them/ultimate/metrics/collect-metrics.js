#!/usr/bin/env node
// ULTIMATE TEST metrics collector. Reads a project's .said brain (after the agents built into it) and
// reports the full doc-30/28 metric set -- not just "tokens", the complete beats-them-all picture.
//
// Usage: node collect-metrics.js <project.said> <projectName> [tokensUsedJson]
//   tokensUsedJson (optional): {"withSaid": N, "baselineNoSaid": M} captured from the agent run.
const { execFileSync } = require('child_process');
const fs = require('fs'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..', '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const BRAIN = process.argv[2];
const NAME = process.argv[3] || path.basename(BRAIN, '.said');
const tokens = process.argv[4] ? JSON.parse(process.argv[4]) : null;
const run = (...a) => { try { return execFileSync(SAID, ['--path', BRAIN, ...a], { encoding: 'utf8', maxBuffer: 1 << 26 }); } catch (e) { return e.stdout || ''; } };
const J = (...a) => { try { return JSON.parse(run(...a, '--json')); } catch { return null; } };

const tok = chars => Math.round(chars / 4); // doc 28

// --- count each kind in the brain ---
const stats = run('stats', '--verbose');
const memTotal = +(stats.match(/Memories:\s+(\d+)/) || [])[1] || 0;
const symbols = +(stats.match(/Symbol table:\s+(\d+)/) || [])[1] || 0;

// blueprints (auto-created from fn used >1x) -- harvested have "(harvested, Nx)" in their shape
const bpList = run('recall-blueprint', '--shape', 'endpoint service handler create', '--min-similarity', '0.0', '--top-k', '50');
const harvested = [...bpList.matchAll(/harvested,\s*(\d+)x/g)].map(m => +m[1]);
const blueprintsAuto = harvested.length;
const maxReuse = harvested.length ? Math.max(...harvested) : 0;

// learned fixes (tasks) -- count fix:: frames via several recall probes (recall-fix has no --top-k; k=1).
// Probe with the distinct problems an invoicing build would learn, dedup the TASK lines.
let fixes = 0;
{
  const seen = new Set();
  for (const p of ['validate the request fields', 'generate an id for a new entity', 'persist to the repository',
                   'return the response envelope', 'handle a not found id', 'thin controller validation seam']) {
    const out = run('recall-fix', '--problem', p, '--min-similarity', '0.0');
    for (const m of out.matchAll(/TASK:\s*([^\n]+)/g)) seen.add(m[1].trim().slice(0, 60));
  }
  fixes = seen.size;
}

// memories (claim+evidence)
const memRecords = (run('memory-manifest').match(/^\s*\[/gm) || []).length;

const report = {
  project: NAME,
  totalMemories: memTotal,
  codeSymbols: symbols,
  blueprintsAutoCreated: blueprintsAuto,
  maxBlueprintReuse: maxReuse,          // highest "seen Nx" -- a fn/shape reused most
  fixesLearned: fixes,
  memoryRecords: memRecords,
  tokens: tokens || '(not captured -- pass tokensUsedJson)',
  tokenSavingX: tokens && tokens.baselineNoSaid && tokens.withSaid
    ? Math.round(tokens.baselineNoSaid / tokens.withSaid) : null,
};

console.log(`=== ULTIMATE TEST metrics: ${NAME} ===`);
console.log(JSON.stringify(report, null, 2));

const out = path.join(__dirname, `${NAME}-metrics.json`);
fs.writeFileSync(out, JSON.stringify(report, null, 2));
console.log(`\nwrote ${out}`);
