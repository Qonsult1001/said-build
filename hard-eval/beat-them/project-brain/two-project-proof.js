#!/usr/bin/env node
// Proves project scope on the problem it is FOR: cross-project isolation of EPISODIC/code memories,
// while PROCEDURAL fixes/blueprints stay cross-project reusable (the 80% win). One brain, two projects.
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const brain = path.join(os.tmpdir(), `twoproj_${process.pid}.said`);
for (const f of [brain, brain + '.spill']) { try { fs.unlinkSync(f); } catch {} }
const run = (args, env) => execFileSync(SAID, args, { encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 1 << 24 });
run(['create', brain]);

// helper: write a dir with one file, ingest under a project (code-ingest path -> project-tagged)
function ingestProjectFile(project, name, text) {
  const d = path.join(os.tmpdir(), `tp_${project}_${process.pid}`); fs.mkdirSync(d, { recursive: true });
  fs.writeFileSync(path.join(d, name), text);
  run(['--path', brain, 'add', '--dir', d], { SAID_PROJECT: project });
}
// EPISODIC/code memories, one per project, with a DISTINCT marker
ingestProjectFile('said-build', 'commit_build.txt', 'commit ZZZ: said-build wired SessionStart re-ground for compaction survival');
ingestProjectFile('said-echo',  'commit_echo.txt',  'commit ZZZ: said-echo tuned the MinishLab potion-code encoder rerank');

// PROCEDURAL fix learned under said-echo (must remain reusable from said-build)
run(['--path', brain, 'learn-fix', '--problem', 'implement an LRU cache with O(1) eviction',
     '--learnings', 'ECHO FIX: use HashMap + doubly linked list, move-to-front on get',
     '--edits', '[{"file":"lru.rs","content":"struct LruCache"}]'],
    { SAID_PROJECT: 'said-echo' });

let fails = 0; const ok = (n, c, d) => { console.log(`  ${c ? 'PASS' : 'FAIL'}  ${n}${c ? '' : '  -> ' + (d || '')}`); if (!c) fails++; };
const ask = (q, env) => run(['--path', brain, 'ask', q, '--json'], env);
const idsFor = (q, env) => { try { return (JSON.parse(ask(q, env)).results || []).map(r => r.doc_id); } catch { return []; } };

console.log('=== two-project isolation + cross-project fix reuse ===\n');

// 1) UNSCOPED: a said-build query can see both projects' commits (reuse stays possible).
const open = idsFor('which commit wired the compaction survival re-ground', {});
ok('unscoped recall sees said-build commit', open.some(i => i.includes('commit_build')));

// 2) SCOPED to said-build: must NOT return said-echo's commit (episodic isolation).
const scoped = idsFor('commit ZZZ encoder rerank tuning', { SAID_RECALL_PROJECT: 'said-build' });
ok('scoped said-build does NOT leak said-echo commit', !scoped.some(i => i.includes('commit_echo')),
   `got: ${scoped.join(', ')}`);

// 3) PROCEDURAL reuse: even scoped to said-build, the said-echo FIX is still recallable (the 80% win).
const fixScoped = run(['--path', brain, 'recall-fix', '--problem', 'build an LRU cache O(1) eviction', '--min-similarity', '0.0'],
                      { SAID_RECALL_PROJECT: 'said-build' });
ok('scoped said-build STILL reuses said-echo procedural fix', fixScoped.includes('ECHO FIX'),
   'procedural memory must stay cross-project');

for (const f of [brain, brain + '.spill']) { try { fs.unlinkSync(f); } catch {} }
console.log(`\n${fails === 0 ? 'ALL PASS -- episodic isolated per project; procedural (fixes) reusable across projects.' : fails + ' FAILED'}`);
process.exit(fails === 0 ? 0 : 1);
