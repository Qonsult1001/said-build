#!/usr/bin/env node
// E2E: the doc-31 memory-evidence standard, living INSIDE one .said brain ALONGSIDE blueprints + fixes.
// Proves: (1) memories save with evidence links; (2) the manifest is the LLM-select list (memory-only);
// (3) recall returns claim + evidence-to-verify; (4) OKF traversal reaches shared evidence; (5) blueprint
// + coding-fix still recall in the SAME brain (coexistence, nothing replaced).
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const BR = path.join(os.tmpdir(), `mem_e2e_${process.pid}.said`);
for (const f of [BR, BR + '.spill']) { try { fs.unlinkSync(f); } catch {} }
const env = { ...process.env, SAID_PROJECT: 'said-build' };
const run = (...a) => execFileSync(SAID, ['--path', BR, ...a], { encoding: 'utf8', env, maxBuffer: 1 << 24 });
execFileSync(SAID, ['create', BR]);

let fails = 0; const ok = (n, c, d) => { console.log(`  ${c ? 'PASS' : 'FAIL'}  ${n}${c ? '' : '  -> ' + (d || '')}`); if (!c) fails++; };
console.log('=== memory-evidence standard e2e (one brain: memory + blueprint + fix) ===\n');

// (1) save memories with evidence links + a blueprint + a fix in ONE brain
run('save-memory', '--name', 'kind-axis', '--mtype', 'project', '--description', 'kind-axis lifted recall@10 35->85',
    '--claim', 'pillar scope fix', '--evidence', 'edd8a3c', '--evidence', 'ask.rs');
run('save-memory', '--name', 'okf-default', '--mtype', 'project', '--description', 'OKF default-on',
    '--claim', 'reachability lever', '--evidence', '1e58c9e', '--evidence', 'edd8a3c');
run('save-memory', '--name', 'owner-pref', '--mtype', 'user', '--description', 'honest measurement, no overclaiming',
    '--claim', 'always measure on a real brain');
run('learn-blueprint', '--shape', 'Create<Entity> endpoint', '--sections', '{"sections":["validate","persist","respond"]}');
run('learn-fix', '--problem', 'implement an LRU cache O(1)', '--learnings', 'HashMap + DLL', '--edits', '[{"file":"lru.rs","content":"x"}]');

// (2) manifest = memory-only select list
const man = run('memory-manifest', '--json');
const mj = JSON.parse(man.split('\n').filter(l => l.trim().startsWith('[')).pop() || '[]');
ok('manifest lists exactly the 3 memory frames (blueprint/fix excluded)', mj.length === 3, `got ${mj.length}`);
ok('manifest carries type + description (the LLM-select hooks)',
   mj.some(e => e.type === 'user') && mj.some(e => e.name === 'kind-axis' && e.description.includes('recall@10')));

// (3) recall returns claim + evidence links (the source to verify)
const rec = JSON.parse(run('recall-memory', '--name', 'kind-axis', '--json').split('\n').filter(l => l.includes('"memory"')).pop());
ok('recall returns the claim', (rec.claim || '').includes('pillar scope fix'));
ok('recall returns evidence links (commit + source frame)',
   rec.evidence_links.includes('link:commit-edd8a3c') && rec.evidence_links.includes('link:ask.rs'));

// (4) OKF traversal: both memories share commit-edd8a3c -> reachable
const concepts = run('list-concepts', '--prefix', 'commit', '--json');
const cj = JSON.parse(concepts.split('\n').filter(l => l.trim().startsWith('[')).pop() || '[]');
const shared = cj.find(c => c.concept === 'commit-edd8a3c');
ok('OKF: both memories reachable via shared commit-edd8a3c evidence', shared && shared.memories >= 2,
   `commit-edd8a3c reaches ${shared ? shared.memories : 0}`);

// (5) coexistence: blueprint + fix still recall in the same brain
ok('blueprint still recalls in the same brain',
   run('recall-blueprint', '--shape', 'create a new entity endpoint', '--min-similarity', '0.0').includes('Create<Entity> endpoint'));
ok('coding-fix (global learned lesson) still recalls in the same brain',
   run('recall-fix', '--problem', 'build an LRU cache', '--min-similarity', '0.0').includes('LRU cache'));

for (const f of [BR, BR + '.spill']) { try { fs.unlinkSync(f); } catch {} }
console.log(`\n${fails === 0 ? 'ALL PASS -- memory-evidence frames live INSIDE .said alongside blueprints + fixes; nothing replaced.' : fails + ' FAILED'}`);
process.exit(fails === 0 ? 0 : 1);
