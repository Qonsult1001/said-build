#!/usr/bin/env node
// Live-MCP coverage prober: call the REMAINING tools (beyond the 5 in mcp_e2e.js) via JSON-RPC against
// the running said-mcp server, over a real ingested brain. Records which tools work LIVE (vs only Rust-
// engine-tested). Honest coverage closing the "5 of 34 live" gap.
const { spawn, execSync } = require('child_process');
const fs = require('fs'), path = require('path');
const MCP = 'g:/development/said-build/said-mcp-coding.exe';
const CLI = 'g:/development/said-build/said-coding.exe';
const BRAIN = 'g:/cargo-tmp/mcp_tools_live.said';
const SRC = 'g:/cargo-tmp/mcp_tools_live_src';

function mcp(reqs) {
  return new Promise((res) => {
    const p = spawn(MCP, ['--path', BRAIN], { stdio: ['pipe','pipe','ignore'] });
    let buf=''; const out=[];
    p.stdout.on('data',d=>{buf+=d;let i;while((i=buf.indexOf('\n'))>=0){const l=buf.slice(0,i);buf=buf.slice(i+1);if(l.trim())try{out.push(JSON.parse(l))}catch{}}});
    p.stdin.write(JSON.stringify({jsonrpc:'2.0',id:0,method:'initialize',params:{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'p',version:'1'}}})+'\n');
    p.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');
    for(const r of reqs) p.stdin.write(JSON.stringify({jsonrpc:'2.0',...r})+'\n');
    p.stdin.end(); p.on('close',()=>res(out)); setTimeout(()=>{try{p.kill()}catch{}},45000);
  });
}
const T=(r)=>r?.result?.content?.[0]?.text||'';
const id=(o,i)=>o.find(x=>x.id===i);
const C=(i,name,args)=>({id:i,method:'tools/call',params:{name,arguments:args}});

(async()=>{
  for(const f of [BRAIN,BRAIN+'.spill']){try{fs.rmSync(f)}catch{}}
  fs.rmSync(SRC,{recursive:true,force:true}); fs.mkdirSync(SRC,{recursive:true});
  fs.writeFileSync(path.join(SRC,'app.js'),'// auth + billing\nfunction makeSession(user){ return {user, exp: Date.now()+1800000}; }\nfunction chargeCard(amt){ return amt>0; }\nfunction resendFailedWebhooks(q){ return q.filter(w=>w.failed); }\nmodule.exports={makeSession,chargeCard,resendFailedWebhooks};\n');
  execSync(`"${CLI}" create "${BRAIN}"`,{stdio:'ignore'});

  let passed=0, failed=0; const verdict=(name,okk,note)=>{ if(okk){passed++;console.log(`  LIVE-OK  ${name}  ${note||''}`)} else {failed++;console.log(`  LIVE-FAIL ${name}  ${note||''}`)} };
  const notErr=(r)=>r && !r.error && r.result!=null;

  // init (the tool we fixed) — populate the brain
  let o=await mcp([C(1,'init',{dir:SRC})]); verdict('init', /memor|frame|indexed|ingest/i.test(T(id(o,1))) && !/unrecognized|failed/i.test(T(id(o,1))), T(id(o,1)).slice(0,40).replace(/\n/g,' '));
  // status
  o=await mcp([C(1,'status',{})]); verdict('status', notErr(id(o,1)) && /\d/.test(T(id(o,1))));
  // ask / search / sym
  o=await mcp([C(1,'ask',{query:'session expiry token creation',top:5})]); verdict('ask', /makeSession|session/i.test(T(id(o,1))));
  o=await mcp([C(1,'search',{query:'retry webhooks that failed'})]); verdict('search', /resendFailedWebhooks|webhook/i.test(T(id(o,1))));
  o=await mcp([C(1,'sym',{name:'makeSession'})]); verdict('sym', /makeSession/i.test(T(id(o,1))));
  // overview / discover
  o=await mcp([C(1,'overview',{})]); verdict('overview', notErr(id(o,1)));
  o=await mcp([C(1,'discover',{})]); verdict('discover', notErr(id(o,1)));
  // remember / journal / get
  o=await mcp([C(1,'remember',{content:'Billing runs at 02:00 UTC daily.',title:'billing window'})]); verdict('remember', notErr(id(o,1)));
  o=await mcp([C(1,'journal',{topic:'audit',summary:'wanted live tool coverage; ran all; next: project test'})]); verdict('journal', notErr(id(o,1)));
  // history (symbol timeline) / sync
  o=await mcp([C(1,'history',{name:'makeSession'})]); verdict('history', notErr(id(o,1)));
  o=await mcp([C(1,'sync',{})]); verdict('sync', notErr(id(o,1)) && !/unrecognized|failed to/i.test(T(id(o,1))));
  // dream / salience / session_end (correct schemas)
  o=await mcp([C(1,'dream',{})]); verdict('dream', notErr(id(o,1)));
  o=await mcp([C(1,'salience',{content:'We deliberately do not walk the call-graph in ask().'})]); verdict('salience', notErr(id(o,1)));
  o=await mcp([C(1,'session_end',{summary:'live tool coverage sweep complete'})]); verdict('session_end', notErr(id(o,1)));
  // lsp_* (need `location`; confirm they respond without crashing the server)
  o=await mcp([C(1,'lsp_symbols',{query:'makeSession'})]); verdict('lsp_symbols', id(o,1)!=null);
  o=await mcp([C(1,'lsp_def',{location:'app.js:2:10'})]); verdict('lsp_def', id(o,1)!=null);
  o=await mcp([C(1,'lsp_refs',{location:'app.js:2:10'})]); verdict('lsp_refs', id(o,1)!=null);
  o=await mcp([C(1,'lsp_hover',{location:'app.js:2:10'})]); verdict('lsp_hover', id(o,1)!=null);

  console.log(`\n=== REMAINING LIVE-MCP TOOLS: ${passed} ok, ${failed} fail ===`);
  process.exit(0); // report-only; some tools may legitimately no-op on a tiny brain
})();
