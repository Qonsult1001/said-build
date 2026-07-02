// Seed one C# fix + one Rust fix into a brain via MCP, for the Phase-4 coding arms.
'use strict';
const { McpClient } = require('./mcp-client');
const fs = require('fs');
const BRAIN = 'G:/development/said-build/hard-eval/mcp-harness/arms.said';
(async () => {
  for (const e of ['','.spill']) { try { fs.unlinkSync(BRAIN+e); } catch {} }
  const c = new McpClient(BRAIN, {}); c.start(); await c.initialize();
  // C# fix: the codebase convention for guarding a constructor arg (Guard.AgainstNullOrEmpty + domain exception)
  await c.call('learn_fix', {
    problem: 'a domain aggregate constructor accepts an invalid argument without guarding it, so bad input fails deep instead of fast at the boundary',
    edits: JSON.stringify([{ file: 'Domain/Models/Account.cs', change: 'At the top of the constructor, validate each argument up front: Guard.AgainstNullOrEmpty(profileId, nameof(profileId)) for strings, and for a numeric identity throw a domain-specific exception (e.g. InvalidAccountStatusException) when it is <= 0. Guards go BEFORE any field assignment so the aggregate never enters an invalid state.' }]),
    learnings: 'C# convention in this codebase: aggregate constructors validate every argument at the top via Guard.AgainstNullOrEmpty for strings and a domain-specific exception for invalid values, before assigning fields.',
  });
  // Rust fix: the crate convention for a fallible constructor (Result + a typed error, no panic)
  await c.call('learn_fix', {
    problem: 'a Rust constructor can receive invalid input but panics or silently accepts it instead of returning a typed error',
    edits: JSON.stringify([{ file: 'src/thing.rs', change: 'Make the constructor fallible: `pub fn new(..) -> Result<Self, String>` (or a typed error enum). Validate inputs at the top and return Err(..) with a clear message on invalid input; never panic/unwrap on caller-provided data. Only construct Self after all checks pass.' }]),
    learnings: 'Rust convention in this crate: constructors that can fail return Result<Self, E> and validate inputs up front, returning a typed Err instead of panicking.',
  });
  const st = await c.call('status', {});
  console.log('seeded:', (st.text.match(/Memories:\s*[\d,]+/)||[])[0]);
  c.stop(); process.exit(0);
})().catch(e=>{console.error(e.message);process.exit(2);});
