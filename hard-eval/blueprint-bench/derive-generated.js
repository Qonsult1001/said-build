#!/usr/bin/env node
// DERIVE the GENERATED/YOURS split from the recalled BLUEPRINT, not by hand. This is the principle:
//   a rendered section is GENERATED  iff its name matches a section in the recalled blueprint (the
//                                    indexed standard -- the reused 80%);
//   a rendered section is YOURS       iff it is NOT in the blueprint (entity-specific -- you added it).
// So the 80/20 ratio = MEASURED blueprint coverage, not a label I chose. Run after harvesting a brain:
//   node derive-generated.js <brain.said> <renderedFile> <shapeQuery>
const { execFileSync } = require('child_process');
const fs = require('fs'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');

const [brain, file, ...q] = process.argv.slice(2);
if (!brain || !file || !q.length) { console.error('usage: derive-generated.js <brain.said> <file> <shape query...>'); process.exit(2); }
const query = q.join(' ');

// 1. recall the blueprint for this shape (top-1) and extract its section names (the standard).
const out = execFileSync(SAID, ['--path', brain, 'recall-blueprint', '--shape', query, '--min-similarity', '0.0', '--top-k', '1'], { encoding: 'utf8' });
const secMatch = out.match(/sections:\s*(\{.*\})/);
let blueprintSecs = new Set();
if (secMatch) {
  try { (JSON.parse(secMatch[1]).sections || []).forEach(s => blueprintSecs.add(norm(s))); } catch {}
}
function norm(s){ return s.toLowerCase().replace(/[^a-z0-9]+/g,''); } // compare names loosely (accept-and-audit ~ acceptandaudit)

// 2. parse the rendered file's sections.
const lines = fs.readFileSync(file, 'utf8').split('\n');
const rendered = [];
for (const l of lines) {
  const m = l.match(/\[S(\d+)\]\s+(.+?)\s+(GENERATED|YOURS)\s*$/);
  if (m) rendered.push({ n: +m[1], name: m[2].trim(), hand: m[3] });
}

// 3. DERIVE: GENERATED iff the section name is covered by the blueprint.
let gen = 0, yours = 0, mismatch = 0;
console.log(`blueprint "${query}" covers ${blueprintSecs.size} sections; rendered file has ${rendered.length}.\n`);
console.log('  S | section          | hand-label | DERIVED   | agree?');
for (const r of rendered) {
  const derived = blueprintSecs.has(norm(r.name)) ? 'GENERATED' : 'YOURS';
  if (derived === 'GENERATED') gen++; else yours++;
  const agree = derived === r.hand;
  if (!agree) mismatch++;
  console.log(`  S${r.n} | ${r.name.padEnd(16)} | ${r.hand.padEnd(10)} | ${derived.padEnd(9)} | ${agree ? 'yes' : 'NO <-'}`);
}
const total = gen + yours;
console.log(`\nDERIVED from the blueprint: ${gen}/${total} GENERATED = ${total ? Math.round(100*gen/total) : 0}% reused (the real 80% number).`);
console.log(`hand-labels disagreed on ${mismatch}/${total} sections (shows hand-labeling was guesswork).`);
