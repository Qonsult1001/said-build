#!/usr/bin/env node
// Prototype of two canon features:
//  (1) WIKI GENERATOR — scan all canon files, (re)build said.index.md with library drill-down + said.log.md.
//  (2) DRIFT GUARD — detect a user-edited GENERATED section (blake3-style hash mismatch) and decide:
//      bypass set -> overwrite silently (Claude permission standard); else -> PROMPT keep/overwrite.
const fs=require('fs'), path=require('path'), crypto=require('crypto');
const DIR=__dirname;
const hash=s=>crypto.createHash('sha256').update(s).digest('hex').slice(0,12); // stand-in for blake3

// --- parse a canon file into sections: [{n, name, klass, body}] ---
function parse(file){
  const src=fs.readFileSync(path.join(DIR,file),'utf8'), lines=src.split('\n');
  const secs=[]; let cur=null;
  for(const l of lines){
    let m=l.match(/\[S(\d+)\]\s+(.+?)\s+(GENERATED|YOURS)\s*$/);
    if(m){cur={n:+m[1],name:m[2].trim(),klass:m[3],body:[]};continue;}
    if(/\[\/S\d+\]/.test(l)){ if(cur){cur.hash=hash(cur.body.join('\n'));secs.push(cur);cur=null;} continue;}
    if(cur) cur.body.push(l);
  }
  return secs;
}
const files=fs.readdirSync(DIR).filter(f=>/\.(cs|rs|py|ts|java)$/.test(f) && parse(f).length);

// (1) WIKI GENERATOR — library drill-down: project -> file -> section
function genWiki(){
  let idx=`---\ntype: said.canon.index\nspec: okf/1.0\nproject: said-demo\n---\n\n# Said Canon Index (the wiki)\n\n`;
  idx+=`Library drill-down: project -> file -> section. Click a file to open the code. Each section tag\n`;
  idx+=`says: **can I edit it?** GENERATED = .said rewrites it (don't edit). YOURS = .said keeps it (edit freely).\n`;
  idx+=`How to change a GENERATED section, eject, review: see "Controls" at the bottom -- stated ONCE.\n\n`;
  idx+=`## Files (${files.length})\n\n`;
  let totG=0,totY=0;
  for(const f of files){
    const s=parse(f); const g=s.filter(x=>x.klass==='GENERATED').length, y=s.length-g; totG+=g; totY+=y;
    idx+=`### [${f}](${f}) -- ${s.length} sections (${y} YOURS / ${g} GENERATED)\n`;
    idx+=`| S | Section | Edit? |\n|---|---|---|\n`;
    for(const x of s) idx+=`| S${x.n} | ${x.name} | ${x.klass==='YOURS'?'YOURS -- yes':'GENERATED -- no'} |\n`;
    idx+=`\n`;
  }
  idx+=`## Totals\nAcross ${files.length} files: **you own ${totY}** sections, **.said generates ${totG}** (re-emitted free on every entity).\n\n`;
  idx+=`## If you edit a GENERATED section (stated once)\n`;
  idx+=`GENERATED sections are written by .said from a saved template and get rebuilt. If you change one,\n`;
  idx+=`.said asks before rebuilding over it:\n`;
  idx+=`- **Keep my change** -- stop auto-generating just that one section.\n`;
  idx+=`- **Discard my change** -- restore the generated version.\n`;
  idx+=`- **Take ownership** -- that section becomes yours forever; .said never regenerates it.\n`;
  idx+=`To change a GENERATED section everywhere at once, change the saved template it comes from.\n\n`;
  idx+=`History: [said.log.md](said.log.md)\n`;
  fs.writeFileSync(path.join(DIR,'said.index.md'),idx);
  // log
  let log=`---\ntype: said.canon.log\nspec: okf/1.0\n---\n\n# Said Canon Log\n\n| File | S | Section | Class | Hash |\n|---|---|---|---|---|\n`;
  for(const f of files) for(const x of parse(f)) log+=`| ${f} | S${x.n} | ${x.name} | ${x.klass} | ${x.hash} |\n`;
  fs.writeFileSync(path.join(DIR,'said.log.md'),log);
  console.log(`WIKI: regenerated said.index.md + said.log.md across ${files.length} files (${totY} YOURS / ${totG} GENERATED).`);
}

// (2) DRIFT WARNING — NOT a merge UI. Decision (14.15): don't reinvent keep/reject; lean on the host's
// diff + git like Cursor/Claude. .said's job is only to WARN that a rebuild rewrites GENERATED sections
// (YOURS are left untouched) and let the editor/git show the diff. `said eject <Sn>` is the explicit
// "make this mine permanently" lever; there is no per-rebuild interactive menu.
function driftCheck(bypass){
  const logPath=path.join(DIR,'said.log.md');
  const prior={}; if(fs.existsSync(logPath)) for(const l of fs.readFileSync(logPath,'utf8').split('\n')){const m=l.match(/\| (\S+) \| S(\d+) .* \| (GENERATED|YOURS) \| (\w+) \|/);if(m)prior[`${m[1]}#S${m[2]}`]=m[4];}
  let conflicts=[];
  for(const f of files) for(const x of parse(f)){
    if(x.klass!=='GENERATED')continue;             // only GENERATED can conflict (YOURS is preserved anyway)
    const key=`${f}#S${x.n}`, was=prior[key];
    if(was && was!==x.hash) conflicts.push(key);
  }
  if(!conflicts.length){console.log('DRIFT: no edited GENERATED sections -- safe to rebuild (YOURS sections are always kept).');return;}
  if(bypass){ console.log(`Rebuilding over edited GENERATED section(s): ${conflicts.join(', ')} (bypass on -- Claude permission standard). Review with git diff.`); return; }
  console.log(`Heads up: rebuild will regenerate these GENERATED section(s) you edited: ${conflicts.join(', ')}.`);
  console.log(`Your YOURS sections are untouched. Review the change as a normal diff (your editor / git diff) and keep or revert there.`);
  console.log(`To make one of these yours permanently: said eject <file> <Sn>.`);
}

const cmd=process.argv[2];
if(cmd==='wiki') genWiki();
else if(cmd==='drift') driftCheck(process.argv.includes('--bypass'));
else console.log('usage: said-canon-tool.js wiki | drift [--bypass]');
