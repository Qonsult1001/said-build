const C=v=>Math.round(v*100);            // work in integer cents
const D=c=>c/100;
const add=(a,b)=>D(C(a)+C(b));
const subtract=(a,b)=>D(C(a)-C(b));
const multiply=(a,n)=>D(Math.round(C(a)*n));
function distribute(a,n){ let cents=C(a); const base=Math.floor(cents/n); let rem=cents-base*n;
  const out=[]; for(let i=0;i<n;i++){ let c=base; if(rem>0){c++;rem--;} out.push(D(c)); } return out; }
module.exports={add,subtract,multiply,distribute};
