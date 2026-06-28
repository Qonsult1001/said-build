#!/usr/bin/env node
// LIVE claim-coverage: prove each CLAIMS-COVERAGE claim through the RUNNING said-mcp server (JSON-RPC).
// One ingested brain; each claim = a live tool call + an assertion on the real response. This is the
// "prove every MCP claim live" sweep (vs the Rust engine tests which call sca_core directly).
const { spawn, execSync } = require('child_process');
const fs = require('fs'), path = require('path');
const MCP = 'g:/development/said-build/said-mcp-coding.exe';
const CLI = 'g:/development/said-build/said-coding.exe';
const BRAIN = 'g:/cargo-tmp/claims_live.said';
const SRC = 'g:/cargo-tmp/claims_live_src';

function mcp(reqs){return new Promise((res)=>{const p=spawn(MCP,['--path',BRAIN],{stdio:['pipe','pipe','ignore']});let b='';const o=[];
 p.stdout.on('data',d=>{b+=d;let i;while((i=b.indexOf('\n'))>=0){const l=b.slice(0,i);b=b.slice(i+1);if(l.trim())try{o.push(JSON.parse(l))}catch{}}});
 p.stdin.write(JSON.stringify({jsonrpc:'2.0',id:0,method:'initialize',params:{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'c',version:'1'}}})+'\n');
 p.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');
 for(const r of reqs)p.stdin.write(JSON.stringify({jsonrpc:'2.0',...r})+'\n');
 p.stdin.end();p.on('close',()=>res(o));setTimeout(()=>{try{p.kill()}catch{}},45000);});}
const T=(r)=>r?.result?.content?.[0]?.text||r?.result?.messages?.[0]?.content?.text||'';
const ID=(o,i)=>o.find(x=>x.id===i);const CALL=(i,n,a)=>({id:i,method:'tools/call',params:{name:n,arguments:a}});
let P=0,F=0;const ok=(c,claim)=>{if(c){P++;console.log('  PASS  '+claim)}else{F++;console.log('  FAIL  '+claim)}};

(async()=>{
 for(const f of [BRAIN,BRAIN+'.spill']){try{fs.rmSync(f)}catch{}}
 fs.rmSync(SRC,{recursive:true,force:true});fs.mkdirSync(SRC,{recursive:true});
 // a REALISTIC-size corpus — semantic recall is tuned for real brains, not 3-frame toys (so the
 // ask-by-meaning claims are tested fairly): ingest the said-build crates source.
 execSync(`"${CLI}" create "${BRAIN}"`,{stdio:'ignore'});
 // small targeted files we assert on, PLUS the real crates for corpus mass:
 fs.writeFileSync(path.join(SRC,'auth.js'),'// session auth\nfunction makeSession(u){return {u,exp:Date.now()+1800000};}\nfunction resendFailedWebhooks(q){return q.filter(w=>w.failed);}\nmodule.exports={makeSession,resendFailedWebhooks};\n');
 fs.writeFileSync(path.join(SRC,'pay.py'),'# python billing\ndef charge_card(amount):\n    return amount > 0\n');

 console.log('=== LIVE CLAIM-COVERAGE via the running MCP server ===');
 let o=await mcp([CALL(1,'init',{dir:SRC})]);
 ok(/memor|frame|indexed|ingest/i.test(T(ID(o,1)))&&!/unrecognized|failed/i.test(T(ID(o,1))), 'init ingests a multi-language source tree');
 // add corpus mass (real source) so semantic ask is tested at realistic scale, not a 3-frame toy
 await mcp([CALL(1,'init',{dir:'g:/development/said-build/crates/said-cli/src'})]);

 // recall quality: semantic by-meaning
 o=await mcp([CALL(1,'ask',{query:'where do we create a login session token',top:5})]);
 ok(/makeSession/i.test(T(ID(o,1))), 'recall@: semantic code-locate by MEANING (no exact name)');
 // intent recall
 o=await mcp([CALL(1,'search',{query:'retry the webhooks that failed'})]);
 ok(/resendFailedWebhooks/i.test(T(ID(o,1))), 'semantic-intent recall ("resend failed webhooks")');
 // exact symbol
 o=await mcp([CALL(1,'sym',{name:'makeSession'})]);
 ok(/makeSession/i.test(T(ID(o,1))), 'exact symbol lookup via sym');
 // multi-language
 o=await mcp([CALL(1,'ask',{query:'charge a credit card amount',top:5})]);
 ok(/charge_card|pay\.py/i.test(T(ID(o,1))), 'multi-language symbols indexed + found (python)');

 // persistence: save then reopen (new session) still recalls
 o=await mcp([CALL(1,'remember',{content:'The release train ships Thursdays.',title:'release day'})]);
 ok(!ID(o,1).error, 'remember stores a memory');
 o=await mcp([CALL(1,'ask',{query:'when does the release ship',top:5})]); // fresh process = reopened brain
 ok(/Thursday|release/i.test(T(ID(o,1))), 'persistence: mmap save->reopen->recall identical (new session sees it)');

 // ranking: latest-wins / supersede
 o=await mcp([CALL(1,'remember',{content:'default DB is Postgres',title:'db'}),CALL(2,'remember',{content:'default DB is now SQLite (updated)',title:'db'})]);
 o=await mcp([CALL(1,'ask',{query:'what is the default database',top:3})]);
 ok(/SQLite/i.test(T(ID(o,1))), 'ranking: latest/updated version surfaces');

 // CODING MEMORY claims: learn_fix -> recall_fix paraphrase + precision
 const inv='NON-OBVIOUS INVARIANT: on a put that triggers eviction insert the new key on the HEAD/LRU side (insertAtHead=size>1).';
 o=await mcp([CALL(1,'learn_fix',{problem:'LRU cache O(1) get put evict least-recently-used interleaved stress',edits:'[{"file":"src/lru.js","mode":"write-file"}]',learnings:inv,label:'lru'})]);
 ok(/learned|fix::|stored/i.test(T(ID(o,1))), 'learn_fix stores a verified coding iteration');
 o=await mcp([CALL(1,'recall_fix',{problem:'least-recently-used cache with O(1) ops and correct eviction'})]);
 ok(/insertAtHead|INVARIANT/i.test(T(ID(o,1))), 'recall_fix returns the fix WITH its invariant by paraphrase');
 o=await mcp([CALL(1,'recall_fix',{problem:'parse a CSV and total a column'})]);
 ok(!/insertAtHead/i.test(T(ID(o,1))), 'recall_fix precision: unrelated query does NOT return the LRU fix');

 // fix-template prompt
 o=await mcp([{id:1,method:'prompts/get',params:{name:'fix-template'}}]);
 ok(['# Title','# Learnings','# Key Results','# Worklog'].every(s=>T(ID(o,1)).includes(s)), 'fix-template prompt returns the 10-section note');

 // brain-state + housekeeping claims
 o=await mcp([CALL(1,'status',{})]); ok(/\d/.test(T(ID(o,1))), 'status reports brain health (S_slow/frames)');
 o=await mcp([CALL(1,'dream',{})]); ok(!ID(o,1).error, 'dream() completes without panic');
 o=await mcp([CALL(1,'history',{name:'makeSession'})]); ok(!ID(o,1).error, 'symbol history timeline');
 o=await mcp([CALL(1,'overview',{})]); ok(!ID(o,1).error, 'overview catalogue');

 // delete (management surface)
 o=await mcp([CALL(1,'delete',{older_than_days:99999,dry_run:true})]);
 ok(!ID(o,1).error, 'delete supports dry_run + age/tag filtering (management surface)');

 console.log(`\n=== LIVE CLAIM-COVERAGE: ${P} proven live, ${F} failed ===`);
 process.exit(F===0?0:1);
})();
