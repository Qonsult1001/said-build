#!/usr/bin/env bash
# Aggregate the 3 research-correct axes from results.psv.
# CRITICAL: INVALID rows (failed runs) are EXCLUDED — they affect neither pass@k denominators nor turns.
# pass@k uses the unbiased estimator [arXiv:2107.03374]: pass@k = 1 - C(n-c,k)/C(n,k); pass@1 = c/n.
# Abstention is scored GENERICALLY over every row whose task had gold __ABSTAIN__ (abstain_gold==1),
# NOT a hardcoded task id (FIX #11) — supports N probes and is what abstention-F1 over a SET needs.
# Columns: arm|task|sample|turns|cost|correct|abstained|valid|abstain_gold
set -u
PSV="${1:-/tmp/bench_research_out/results.psv}"
[ -s "$PSV" ] || { echo "no results: $PSV"; exit 1; }

awk -F'|' '
function comb(n,k,  r,i){ if(k<0||k>n)return 0; if(k==0||k==n)return 1; r=1; for(i=0;i<k;i++) r=r*(n-i)/(i+1); return r }
function passk(n,c,k){ if(n<k) return (c>0?1:0); if(c==0) return 0; return 1 - comb(n-c,k)/comb(n,k) }
function sd(sum,sq,m,  v){ if(m<2)return 0; v=(sq-sum*sum/m)/(m-1); return v>0?sqrt(v):0 }
NR==1{ next }
{
  arm=$1; task=$2; turns=$4; cost=$5; correct=$6; abst=$7; valid=$8; ag=$9
  if(valid!="VALID"){ inv[arm"|"task]++; invtot++; next }   # EXCLUDE failed runs
  n[arm"|"task]++; ntot[arm]++
  # Cost-of-Pass accumulators (docs/23 axis-2): amortize over ALL valid samples (failures included),
  # then divide by R=pass@1 in END. Two denominators: dollars (total_cost_usd) and turns. Dollars go
  # n/a if ANY valid sample lacks a real cost (never fabricate a $ figure — docs/23 anti-hallucination).
  turnsum[arm"|"task]+=turns; vcnt[arm"|"task]++
  if(cost=="NA" || cost==""){ costna[arm"|"task]=1 } else { costsum[arm"|"task]+=cost }
  # across-sample correctness distribution per arm (docs/23: report mean +/- spread, not single runs;
  # there is NO RNG seed loop — .said is deterministic, the external LLM variance shows up across samples)
  cc1=(correct=="1")?1:0; csum[arm]+=cc1; csq[arm]+=cc1*cc1; cm[arm]++
  if(correct=="1"){ c[arm"|"task]++; ctot[arm]++; tsum[arm"|"task]+=turns; tcnt[arm"|"task]++ }
  if(abst=="1"){ a[arm"|"task]++ }
  # ABSTENTION generic (FIX #11) + F1 confusion (FIX #8), per arm, over abstain_gold rows:
  #   TP = abstained on an abstain-gold task (correctly gave up)
  #   FN = answered    on an abstain-gold task (should have abstained)
  #   FP = abstained on an answerable task    (wrongly gave up)
  if(ag=="1"){ absN[arm]++; if(abst=="1"){tp[arm]++} else {fn[arm]++} }
  else        { if(abst=="1"){fp[arm]++} }
  if(ag=="1" && correct=="1"){ absok[arm]++ }
  keys[arm"|"task]=1; arms[arm]=1; tasks[task]=1
}
END{
  printf "=== per task: pass@1 / pass@5 (VALID samples only; INVALID excluded) ===\n"
  printf "%-8s %-10s %4s %4s %7s %7s %8s\n","arm","task","n","c","pass@1","pass@5","inv"
  for(key in keys){ split(key,p,"|"); arm=p[1]; t=p[2]
    nn=n[key]+0; cc=c[key]+0; ii=inv[key]+0
    p1=passk(nn,cc,1); p5=passk(nn,cc,5)
    printf "%-8s %-10s %4d %4d %7.2f %7.2f %8d\n",arm,t,nn,cc,p1,p5,ii
  }
  printf "\n=== across-sample correctness distribution (mean +/- SD; no seed loop — see header) ===\n"
  for(arm in arms){ m=cm[arm]+0; mean=(m>0?csum[arm]/m:0)
    printf "  %-8s mean-correct=%.2f  SD=%.2f  (n=%d samples)\n",arm,mean,sd(csum[arm],csq[arm],m),m }
  printf "\n=== co-solved tasks: avg turns-to-success (both arms have >=1 correct) ===\n"
  for(t in tasks){
    bk="baseline|"t; mk="memory|"t
    if((c[bk]+0)>0 && (c[mk]+0)>0){
      bt=(tcnt[bk]>0?tsum[bk]/tcnt[bk]:0); mt=(tcnt[mk]>0?tsum[mk]/tcnt[mk]:0)
      d=(bt>0?100*(mt-bt)/bt:0)
      printf "  %-10s baseline=%.1ft  memory=%.1ft  delta=%+.0f%%\n",t,bt,mt,d
    }
  }
  printf "\n=== co-solved Cost-of-Pass = (mean cost over all valid samples)/R  [2504.13359] ===\n"
  printf "    (R = pass@1; amortized over failures; $ = n/a if any valid sample lacked a cost)\n"
  for(t in tasks){
    bk="baseline|"t; mk="memory|"t
    if((c[bk]+0)>0 && (c[mk]+0)>0){
      # R per arm on this task = pass@1 = c/n
      bR=(n[bk]>0?c[bk]/n[bk]:0); mR=(n[mk]>0?c[mk]/n[mk]:0)
      # turns Cost-of-Pass: mean turns over all valid samples / R (always available)
      btC=(bR>0 && vcnt[bk]>0 ? (turnsum[bk]/vcnt[bk])/bR : 0)
      mtC=(mR>0 && vcnt[mk]>0 ? (turnsum[mk]/vcnt[mk])/mR : 0)
      dt=(btC>0?100*(mtC-btC)/btC:0)
      # dollar Cost-of-Pass: n/a if either arm had any NA-cost valid sample
      if(costna[bk] || costna[mk]){
        printf "  %-10s CoP$: baseline=n/a  memory=n/a   CoP-turns: baseline=%.1f  memory=%.1f  delta=%+.0f%%\n",t,btC,mtC,dt
      } else {
        bC=(bR>0 && vcnt[bk]>0 ? (costsum[bk]/vcnt[bk])/bR : 0)
        mC=(mR>0 && vcnt[mk]>0 ? (costsum[mk]/vcnt[mk])/mR : 0)
        dc=(bC>0?100*(mC-bC)/bC:0)
        printf "  %-10s CoP$: baseline=$%.4f  memory=$%.4f  delta=%+.0f%%   CoP-turns: baseline=%.1f  memory=%.1f  delta=%+.0f%%\n",t,bC,mC,dc,btC,mtC,dt
      }
    }
  }
  printf "\n=== abstention (generic over abstain_gold rows: clean give-up = CORRECT) ===\n"
  for(arm in arms){
    aN=absN[arm]+0; aok=absok[arm]+0
    TP=tp[arm]+0; FP=fp[arm]+0; FN=fn[arm]+0
    prec=(TP+FP>0)?TP/(TP+FP):0; rec=(TP+FN>0)?TP/(TP+FN):0
    f1=(prec+rec>0)?2*prec*rec/(prec+rec):0
    printf "  %-8s abstain-correct=%d/%d  F1=%.2f  (TP=%d FP=%d FN=%d)\n",arm,aok,aN,f1,TP,FP,FN
  }
  printf "  (AURC / risk-coverage: DEFERRED — needs a calibrated per-sample confidence; --print gives none. docs/23)\n"
  printf "\nINVALID (excluded) runs total: %d\n",invtot+0
}' "$PSV"
