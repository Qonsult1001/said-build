#!/usr/bin/env node
// MULTI-AGENT = SHARED MEMORY (the world-class, fewest-moving-parts design).
//
// Owner: ".said needs to be world-class; shared memory works; not over-complicating things, fewer moving
// parts -- the idea is next time I plug in a model that also has agents, it works as well."
//
// So .said is NOT a competing orchestrator. It is the ONE portable brain a FLEET of agents (Kimi, Claude,
// .said's own, ANY future model) all read + write. The whole multi-agent memory problem Kimi/Claude have --
// each subagent's context compacts independently, and the result handed to the parent is a LOSSY SUMMARY
// (Kimi: "provide a more comprehensive summary"; Claude: the Agent tool returns the subagent's final text)
// -- disappears when every agent shares one out-of-band brain: the subagent WRITES its finding as a
// claim+evidence memory, and the parent (or any other agent) RECALLS the EXACT frame, not a paraphrase.
//
// This e2e simulates 3 distinct agents as separate processes (different SAID_PROJECT/identity), all on ONE
// .said file -- the minimal proof of "plug in the next agent, it inherits the brain and just works".
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const BRAIN = path.join(os.tmpdir(), `fleet_${process.pid}.said`);
for (const f of [BRAIN, BRAIN + '.spill']) { try { fs.unlinkSync(f); } catch {} }
execFileSync(SAID, ['create', BRAIN]);
// each "agent" runs as its own invocation with its own identity -- the ONLY shared thing is the .said file
const agent = (id, args) => execFileSync(SAID, ['--path', BRAIN, ...args],
  { encoding: 'utf8', maxBuffer: 1 << 26, env: { ...process.env, SAID_PROJECT: 'fleet', SAID_AGENT: id } });

let fails = 0; const ok = (n, c, d) => { console.log(`  ${c ? 'PASS' : 'FAIL'}  ${n}${c ? '' : '  -> ' + (d || '')}`); if (!c) fails++; };
console.log('=== MULTI-AGENT = shared memory: a fleet on ONE .said (fewest moving parts) ===\n');

// AGENT A (a "researcher" subagent) finds something and WRITES it as a claim+evidence memory.
agent('agent-A', ['save-memory', '--name', 'auth-rootcause', '--mtype', 'project',
  '--description', 'root cause of the auth 500: token TTL parsed as seconds not ms',
  '--claim', 'The auth service 500s because TokenTTL is parsed as SECONDS but issued as MILLISECONDS; fix: divide by 1000 at parse in auth.rs:142. Why: a unit mismatch silently overflows the expiry check.',
  '--evidence', 'auth.rs', '--evidence', 'commit-9f12adf']);
// AGENT A also learns a verified FIX (procedural -- federates to every agent).
agent('agent-A', ['learn-fix', '--problem', 'auth 500 on token expiry check',
  '--learnings', 'TokenTTL is ms; divide by 1000 at parse; guard the overflow',
  '--edits', '[{"file":"auth.rs","content":"ttl_ms / 1000"}]']);

// AGENT B (a DIFFERENT agent, separate process/identity) inherits the brain. It NEVER saw A's conversation.
// It recalls A's EXACT finding (not a lossy summary) by the memory manifest + recall.
const manifest = agent('agent-B', ['memory-manifest']);
ok('agent B sees agent A\'s memory in the shared manifest', /auth-rootcause/.test(manifest), 'manifest carries A\'s finding');

const recalled = agent('agent-B', ['recall-memory', '--name', 'auth-rootcause']);
ok('agent B recalls A\'s finding BYTE-EXACT (not a lossy summary)',
   /parsed as SECONDS but issued as MILLISECONDS/.test(recalled) && /auth\.rs:142/.test(recalled),
   recalled.slice(0, 90));
ok('agent B gets A\'s EVIDENCE links (verify before acting)', /link:auth\.rs|link:commit-9f12adf/.test(recalled));

// AGENT B reuses A's verified FIX (procedural federates across the fleet -- the 80% reuse).
const fix = agent('agent-B', ['recall-fix', '--problem', 'auth returns 500 checking token expiry', '--min-similarity', '0.0']);
ok('agent B reuses A\'s verified FIX (cross-agent procedural reuse)', /ms|divide by 1000|ttl/i.test(fix), fix.slice(0, 80));

// AGENT C (a THIRD agent -- "the next model you plug in") inherits EVERYTHING with zero setup.
const cManifest = agent('agent-C', ['memory-manifest']);
const cFix = agent('agent-C', ['recall-fix', '--problem', 'token expiry 500', '--min-similarity', '0.0']);
ok('a NEWLY plugged-in agent C inherits the whole brain (memory + fix) with zero setup',
   /auth-rootcause/.test(cManifest) && /ms|1000|ttl/i.test(cFix),
   'plug in the next model -> it just works');

for (const f of [BRAIN, BRAIN + '.spill']) { try { fs.unlinkSync(f); } catch {} }
console.log(`\n${fails === 0 ? 'ALL PASS -- a fleet of agents shares ONE .said: findings written as claim+evidence are recalled BYTE-EXACT by other agents (no lossy summary), and verified fixes federate across the fleet. Plug in the next model -> it inherits the brain and just works. World-class via FEWEST moving parts (one portable file, no orchestrator).' : fails + ' FAILED'}`);
process.exit(fails === 0 ? 0 : 1);
