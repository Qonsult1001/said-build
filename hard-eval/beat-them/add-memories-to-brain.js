#!/usr/bin/env node
// Add the distilled Claude memories + seed fixes/blueprints to the COMPLETE brain via save-memory /
// learn-* (which APPEND frames without rebuilding SYMS -- per docs/07-cli-reference/init.md symbol rule).
// The brain already holds the whole project (code AST/sym + docs + git) from a single full init.
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = path.join(ROOT, 'target', 'debug', 'said.exe');
const BR = path.join(__dirname, 'claude-import', 'claude.said');
const MEM = path.join('C:', 'Users', 'Carter', '.claude', 'projects', 'g--development-said-build', 'memory');
const env = { ...process.env, SAID_PROJECT: 'claude-corpus', SAID_OKF_LINKS: '1' };
const run = (...a) => execFileSync(SAID, ['--path', BR, ...a], { encoding: 'utf8', env, maxBuffer: 1 << 26 });

let n = 0;
for (const f of fs.readdirSync(MEM).filter(f => f.endsWith('.md') && f !== 'MEMORY.md')) {
  const body = fs.readFileSync(path.join(MEM, f), 'utf8');
  const fm = /^---\n([\s\S]*?)\n---/.exec(body); const meta = {};
  if (fm) for (const l of fm[1].split('\n')) { const m = /^(\w+):\s*(.+)$/.exec(l.trim()); if (m) meta[m[1]] = m[2].replace(/^["']|["']$/g, ''); }
  const name = meta.name || f.replace(/\.md$/, '');
  const desc = (meta.description || name).slice(0, 200);
  const mtype = ['user', 'feedback', 'project', 'reference'].includes(meta.type) ? meta.type : 'project';
  const claim = body.replace(/^---\n[\s\S]*?\n---\n/, '').trim();
  const ev = new Set();
  for (const m of claim.matchAll(/\b([0-9a-f]{7,40})\b/g)) ev.add(m[1]);
  for (const m of claim.matchAll(/\[\[([a-z0-9-]+)\]\]/g)) ev.add(m[1]);
  const cf = path.join(os.tmpdir(), `_mc${n}.txt`); fs.writeFileSync(cf, claim);
  run('save-memory', '--name', name, '--description', desc, '--mtype', mtype, '--claim-file', cf, ...[...ev].slice(0, 10).flatMap(e => ['--evidence', e]));
  fs.unlinkSync(cf); n++;
}
// seed the learned 80/20
run('learn-blueprint', '--shape', 'Create<Entity> REST endpoint', '--sections', '{"sections":["validate the request","idempotency check","persist the row","wrap and return"]}');
run('learn-blueprint', '--shape', 'MCP tool handler', '--sections', '{"sections":["parse args","open brain","do the action","return guided result"]}');
run('learn-fix', '--problem', 'implement an LRU cache with O(1) eviction', '--learnings', 'HashMap + doubly linked list, move-to-front on get, evict tail', '--edits', '[{"file":"lru.rs","content":"struct LruCache"}]');
run('learn-fix', '--problem', 'static encoder returns empty recall', '--learnings', 'embed-model must load the embedded 4M encoder first (try_load_encoder embedded-first)', '--edits', '[{"file":"engine.rs","content":"auto_load_encoder"}]');
console.log(`added ${n} Claude memory records + 2 blueprints + 2 fixes (via append, SYMS preserved)`);
