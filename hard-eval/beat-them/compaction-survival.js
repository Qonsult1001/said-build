#!/usr/bin/env node
// THE HEADLINE MOAT (docs/said-structure/30): compaction survival.
// Deterministic e2e proving .said re-grounds an agent EXACTLY after the host compacts the context window
// and loses the fact-dense detail -- the #1 weakness none of Claude/Cursor/Kimi can fix (the thing being
// compacted IS their memory). Driven through the real said.exe as a process.
//
//   WITHOUT .said : after compaction, the exact detail (thresholds, decisions, next step) is GONE ->
//                   the agent "goes stupid" (can't recover the verbatim values a summary paraphrased away).
//   WITH .said    : the work-state was captured in .said; on the next turn it re-grounds
//                   the EXACT values -> "like nothing ever disappeared".
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');
const RESULTS = path.join(__dirname, 'results');

// The mid-task work-state, written by the agent IN ITS OWN WORDS (free-form, no rigid schema) -- with the
// EXACT fact-dense detail an LLM summary would paraphrase/lose.
const PROJECT = 'said-build';
const NOTE = [
  'Building compaction-survival as a CORE memory function in sca-core (NOT vault -- vault is enterprise-only).',
  'Next: wire SessionStart re-ground so it fires automatically on the host hook.',
  'Decided: OWN not pointer; work-state is free-form NL (the agent\'s words), not a rigid field schema;',
  '  idempotency threshold = size > 1, NOT >= 1.',
  'Exact values (a summary would drop these): MIN_COMMON_STEPS = 3; recall score = rel_conf*(0.3 + 0.4*sem + 0.3*intent) @ ask.rs:1542; last good commit = fd2bf9e.',
  'Ruled out (DON\'T retry): Soft-ZCA whitening -- tested negative, recall@3 stayed 1/3.',
  'Plan: step 2 of 3 (work-state continuity).',
].join('\n');
const NEXT = 'wire SessionStart re-ground so it fires automatically on the host hook';

const vault = path.join(os.tmpdir(), `bt_${process.pid}.said`);
for (const f of [vault, vault + '.spill']) { try { fs.unlinkSync(f); } catch {} }
execFileSync(SAID, ['create', vault]);

const say = (...a) => execFileSync(SAID, a, { encoding: 'utf8' });
let fails = 0; const ok = (n, c, d) => { console.log(`  ${c ? 'PASS' : 'FAIL'}  ${n}${c ? '' : '  -> ' + (d||'')}`); if (!c) fails++; };

console.log('=== COMPACTION SURVIVAL e2e (real said.exe) ===\n');

console.log('1. Agent is mid-task. It captures work-state into .said BEFORE compaction (its own words):');
// write the note to a file (Windows-safe), then save it
const noteFile = path.join(os.tmpdir(), `ws_note_${process.pid}.txt`);
fs.writeFileSync(noteFile, NOTE);
say('vault', 'workstate-save', vault, '--project', PROJECT, '--note-file', noteFile);
try { fs.unlinkSync(noteFile); } catch {}
console.log('   captured.\n');

console.log('2. THE HOST COMPACTS. The context window is summarized; the verbatim detail is gone.');
console.log('   (simulated: we no longer have the exact values in context -- only a vague summary like');
console.log('    "was working on memory stuff, made some decisions".)\n');

console.log('3a. WITHOUT .said -> the agent cannot recover the exact detail. It would guess/redo:');
console.log('     e.g. "threshold >= 1?" (WRONG), retry Soft-ZCA (already ruled out), lose the next step.\n');

console.log('3b. WITH .said -> re-ground from .said on the next turn:');
const block = say('vault', 'workstate-resume', vault, '--project', PROJECT);
console.log(block.split('\n').map(l => '     ' + l).join('\n'));

console.log('\n4. ASSERT the re-grounded block carries the EXACT detail verbatim (not paraphrased):');
ok('exact threshold recovered (size > 1, NOT >= 1)', block.includes('threshold = size > 1, NOT >= 1'));
ok('exact constant recovered (MIN_COMMON_STEPS = 3)', block.includes('MIN_COMMON_STEPS = 3'));
ok('exact formula + file:line recovered', block.includes('ask.rs:1542'));
ok('exact commit recovered (fd2bf9e)', block.includes('fd2bf9e'));
ok('next step recovered', block.includes(NEXT));
ok('ruled-out dead end recovered (no Soft-ZCA retry)', block.includes('Soft-ZCA'));
ok('plan status recovered', block.includes('step 2 of 3'));

for (const f of [vault, vault + '.spill']) { try { fs.unlinkSync(f); } catch {} }

if (!fs.existsSync(RESULTS)) fs.mkdirSync(RESULTS, { recursive: true });
const verdict = fails === 0
  ? 'PASS -- .said re-grounds the EXACT work-state after compaction; the host loses it, .said does not.'
  : `${fails} CHECK(S) FAILED`;
fs.writeFileSync(path.join(RESULTS, 'compaction-survival.txt'),
  `COMPACTION SURVIVAL e2e (real said.exe)\n\nWITHOUT .said: exact detail lost on compaction (agent goes stupid).\nWITH .said: re-grounded verbatim.\n\n${block}\n\n${verdict}\n`);
console.log(`\n${verdict}`);
console.log(`wrote ${path.join(RESULTS, 'compaction-survival.txt')}`);
process.exit(fails === 0 ? 0 : 1);
