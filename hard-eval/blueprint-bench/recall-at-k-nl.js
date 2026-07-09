#!/usr/bin/env node
// Does naming blueprints with NL INTENT phases (the new scan->agent-names->learn flow) lift recall@k at
// scale vs the old RAW CALL-TOKEN harvest (baseline recall@3=1/3 from recall-at-k-scale.js)?
//
// Fair A/B at scale:
//   crowd  = harvest a big real repo (sca-core + C# app) the OLD raw way -> ~42 competing blueprints.
//   probes = the 3 bench shapes onboarded TWO ways into two brains:
//     RAW : learn_blueprint with the raw call-token sections (what old harvest stored)
//     NL  : learn_blueprint with the agent-named NL intent phases (the new flow)
//   Then query each by an NL INTENT query and report the rank of the correct shape among the crowd.
const { execFileSync } = require('child_process');
const fs = require('fs'); const os = require('os'); const path = require('path');
const ROOT = path.join(__dirname, '..', '..');
const SAID = process.env.SAID_CLI || path.join(ROOT, 'target', 'debug', 'said.exe');

const CROWD_SRC = [ path.join(ROOT, 'crates', 'sca-core', 'src'), path.join(ROOT, 'apps', 'OrchestrationFactory') ];

// the 3 probes: NL intent query + raw-token sections (old) + NL-phase sections (new, agent-named).
const PROBES = [
  { want: 'create', query: 'create a new entity endpoint with validation audit and idempotency',
    raw:  '{"sections":["NewGuid","Record","Seen","Conflict","Mark","BadRequest","Insert","Complete"]}',
    nl:   '{"sections":["accept the request and write an audit row","idempotency check reject duplicates","validate required fields","persist the row","wrap response and return"]}' },
  { want: 'lookup', query: 'call an external rest api over http and parse the json response',
    raw:  '{"sections":["CreateClient","FromSeconds","HttpRequestMessage","SendAsync","ConfigureAwait","ProviderError","Parse","GetProperty"]}',
    nl:   '{"sections":["build the http client","build the request url and headers","send the request","guard the response status","parse the json body","map the result"]}' },
  { want: 'run', query: 'handle a cli command parse arguments validate and return an exit code',
    raw:  '{"sections":["Parse","Has","WriteLine","Load","execute","Get"]}',
    nl:   '{"sections":["parse the command line arguments","validate required flags","load the app context","execute the command action","print the result","return the exit code"]}' },
];
const K = 10;

function makeBrain(tag, mode) {
  const bp = path.join(os.tmpdir(), `nlk_${mode}_${process.pid}.said`);
  for (const f of [bp, bp + '.spill']) { try { fs.unlinkSync(f); } catch {} }
  execFileSync(SAID, ['create', bp]);
  // the crowd: raw harvest of the big repo (competing blueprints, both arms identical)
  for (const s of CROWD_SRC) { if (fs.existsSync(s)) try { execFileSync(SAID, ['--path', bp, 'harvest', s]); } catch {} }
  // the probes: learn each shape with raw OR nl sections
  for (const p of PROBES) {
    execFileSync(SAID, ['--path', bp, 'learn-blueprint', '--shape', `${p.want}<Entity> endpoint`,
      '--sections', mode === 'nl' ? p.nl : p.raw, '--verified']); // verified=overwrite any harvested same-shape
  }
  return bp;
}
function rankOf(bp, p) {
  const out = execFileSync(SAID, ['--path', bp, 'recall-blueprint', '--shape', p.query, '--min-similarity', '0.0', '--top-k', String(K)], { encoding: 'utf8' });
  const ranks = [...out.matchAll(/\[(\d+)\] Blueprint .* shape=(\S+)/g)].map(m => [+m[1], m[2]]);
  const hit = ranks.find(([, s]) => s.toLowerCase().startsWith(p.want));
  return { rank: hit ? hit[0] : 0, n: ranks.length };
}
function score(mode) {
  const bp = makeBrain(mode, mode);
  const at = { 1: 0, 3: 0, 5: 0, miss: 0 }; const rows = [];
  for (const p of PROBES) {
    const { rank, n } = rankOf(bp, p);
    if (rank === 1) { at[1]++; at[3]++; at[5]++; } else if (rank && rank <= 3) { at[3]++; at[5]++; } else if (rank && rank <= 5) at[5]++; else at.miss++;
    rows.push(`    ${p.want.padEnd(7)} rank ${rank || 'MISS'} of ${n}`);
  }
  for (const f of [bp, bp + '.spill']) { try { fs.unlinkSync(f); } catch {} }
  return { at, rows };
}

console.log('A/B at scale (probes compete against ~42 raw-harvested blueprints):\n');
for (const mode of ['raw', 'nl']) {
  const { at, rows } = score(mode);
  console.log(`  ${mode === 'raw' ? 'RAW call-token sections (old harvest)' : 'NL intent phases (new scan->name->learn)'}:`);
  console.log(rows.join('\n'));
  console.log(`    => recall@1=${at[1]}/3  recall@3=${at[3]}/3  recall@5=${at[5]}/3\n`);
}
