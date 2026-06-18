function qsParse(q){
  const out={};
  for(const pair of q.split('&')){
    if(!pair)continue;
    const eq=pair.indexOf('='); const rawk=eq<0?pair:pair.slice(0,eq); const val=eq<0?'':decodeURIComponent(pair.slice(eq+1));
    const key=decodeURIComponent(rawk.replace(/\[.*$/,''));
    const parts=[...rawk.matchAll(/\[([^\]]*)\]/g)].map(m=>m[1]);
    let node=out, k=key;
    for(let i=0;i<parts.length;i++){ const nextEmpty=parts[i]===''; 
      if(node[k]===undefined) node[k]= nextEmpty||/^\d+$/.test(parts[i])? []:{};
      // descend
      const childKey = parts[i]===''? (Array.isArray(node[k])?node[k].length:0) : parts[i];
      if(i===parts.length-1){ if(parts[i]===''){node[k].push(val);} else {node[k][parts[i]]=val;} }
      else { if(node[k][childKey]===undefined) node[k][childKey]= /^\d+$/.test(parts[i+1])||parts[i+1]===''?[]:{}; node=node[k]; k=childKey; }
    }
    if(parts.length===0) node[k]=val;
  }
  // compact + sort indexed arrays
  const fix=(o)=>{ if(Array.isArray(o))return o.filter(x=>x!==undefined).map(fix);
    if(o&&typeof o==='object'){ const ks=Object.keys(o); if(ks.length&&ks.every(x=>/^\d+$/.test(x))){ return ks.map(Number).sort((a,b)=>a-b).map(i=>fix(o[i])); } const r={}; for(const kk of ks)r[kk]=fix(o[kk]); return r;} return o; };
  return fix(out);
}
module.exports={qsParse};
