#!/usr/bin/env bash
# SCALE test for coding-fix recall: does the right learning land in the top-K when
# the store holds ~N records (crowded neighborhood, near-duplicates)? Measures
# recall@1 / @5 / @10 over many query paraphrases. The semantic top-k contract:
# we don't need precision@1 — the right fix in the top-5/10 is a good result.
#
# Usage: N=1000 RUNS=3 bash recall-scale.sh
set -u
ROOT="/g/development/said-build/hard-eval"
SAID="/g/development/said-build/target/release/said.exe"
NODE="/c/nvm4w/nodejs/node"
N="${N:-1000}"          # total stored fixes (decoys + targets)
BRAIN="$ROOT/results/recall_scale_${N}.said"   # per-N path so runs don't clobber
RUNS="${RUNS:-3}"        # repeat the query suite this many times (stochastic-free, but confirms stability)

# ---- Generate N diverse decoy problems (varied domains/verbs/nouns) ----
gen_decoys() {
  "$NODE" -e '
    const N = parseInt(process.argv[1]);
    const verbs = ["Implement","Add","Fix","Optimize","Refactor","Build","Write","Cache","Validate","Parse","Serialize","Stream","Throttle","Batch","Index"];
    const things = ["a binary search tree","a trie","a bloom filter","a connection pool","a retry-with-backoff wrapper","a CSV parser","a JSON schema validator","a rate limiter","a circular buffer","a priority queue","a union-find","a debounce helper","a memo table","a graph BFS","a graph DFS","a topological sort","a sliding-window counter","a token bucket","a leaky bucket","a min-heap","a max-heap","a skip list","a B-tree node split","a hash map resize","a quicksort partition","a merge step","a dijkstra relaxation","a LFU cache","a write-through cache","a read-through cache","a TTL store","a session store","a event emitter","a pub/sub bus","a worker pool","a semaphore","a mutex guard","a deadlock check","a CRDT counter","a vector clock","a consistent hash ring","a load balancer pick","a circuit breaker","a backpressure queue","a pagination cursor","a diff algorithm","a patch apply","a markdown tokenizer","a URL router","a cookie parser"];
    const tails = ["with O(1) operations","handling edge cases","without mutating input","that is thread-safe","with proper error handling","supporting concurrent access","with lazy evaluation","minimizing allocations","that passes the test suite","with bounded memory"];
    let h=2166136261>>>0; const rnd=()=>{h^=h<<13;h^=h>>>17;h^=h<<5;h>>>=0;return h/4294967296;};
    for(let i=0;i<N;i++){
      const v=verbs[(rnd()*verbs.length)|0], t=things[(rnd()*things.length)|0], x=tails[(rnd()*tails.length)|0];
      console.log(`${v} ${t} ${x} [variant ${i}]`);
    }
  ' "$1"
}

echo "=== building scale brain: $N records ==="
rm -f "$BRAIN"
"$SAID" create "$BRAIN" >/dev/null 2>&1

# Plant KNOWN targets first (distinct, recallable shapes).
declare -A TARGETS
TARGETS[lru]="Implement an LRU cache: O(1) get/put, get and put-update count as a use, evict least-recently-used over capacity, get returns -1 if absent"
TARGETS[ratelimiter]="Implement a token-bucket rate limiter: continuous fractional refill capped at capacity, tryRemove deducts only if enough tokens else false"
TARGETS[intervals]="Fix mergeIntervals: sort by start, merge touching intervals at the <= boundary, do not mutate the input arrays"
TARGETS[ttl]="Add TTL expiry to a key-value Store: set(k,v,ttlMs) expires the key ttlMs after set; expired keys are absent for get/has/size; non-TTL keys never expire"

ed='[{"file":"src/x.js","mode":"write-file","content":"// impl\n"}]'
for key in "${!TARGETS[@]}"; do
  "$SAID" --path "$BRAIN" learn-fix --problem "${TARGETS[$key]}" --edits "$ed" \
    --learnings "verified target $key" --label "TARGET:$key" >/dev/null 2>&1
done

# Bulk-add N decoys.
i=0
gen_decoys "$N" | while IFS= read -r prob; do
  "$SAID" --path "$BRAIN" learn-fix --problem "$prob" --edits "$ed" --label "decoy" >/dev/null 2>&1
  i=$((i+1)); if [ $((i % 200)) -eq 0 ]; then echo "  ...stored $i decoys"; fi
done
echo "stored. $("$SAID" --path "$BRAIN" stats 2>&1 | grep -iE 'Active frames|SCA docs' | grep -vE 'profile|No such' | tr '\n' ' ')"
echo ""

# ---- Query suite: paraphrases of each target (NOT the stored text verbatim) ----
declare -A QUERIES
QUERIES[lru]="build a cache that evicts the least recently used entry when full"
QUERIES[ratelimiter]="rate limit requests using a token bucket that refills over time"
QUERIES[intervals]="merge overlapping intervals after sorting without mutating inputs"
QUERIES[ttl]="key value store where entries expire after a time to live"

# recall_at: print rank of TARGET:<key> in top-10 via recall-fix top-k debug.
# We use SAID_FIX_SCORE_DEBUG to see the ranked list, find the target's position.
rank_of() { # $1=key  $2=query
  local key="$1" q="$2"
  SAID_FIX_SCORE_DEBUG=1 "$SAID" --path "$BRAIN" recall-fix --problem "$q" --min-similarity 0.0 2>&1 \
    | grep "fix-score" | grep -vE "profile|No such" \
    | sort -t'>' -k2 -rn \
    | "$NODE" -e '
        const key=process.argv[1];
        let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{
          const lines=s.trim().split(/\r?\n/).filter(Boolean);
          // we cannot see provenance here; print the doc_ids in rank order
          for(let i=0;i<lines.length;i++){const m=lines[i].match(/fix::[0-9a-f]+/);if(m)console.log((i+1)+" "+m[0]);}
        });
      ' "$key"
}

echo "ticket,run,target_doc,rank_in_topk" > "$ROOT/results/recall-scale.csv"
for run in $(seq 1 "$RUNS"); do
  echo "=== RUN $run ==="
  for key in lru ratelimiter intervals ttl; do
    # find the target doc_id (by provenance tag)
    tdoc=$("$SAID" --path "$BRAIN" --json recall-fix --problem "${TARGETS[$key]}" --min-similarity 0.0 2>/dev/null | grep -oE 'fix::[0-9a-f]+' | head -1)
    # rank of that doc for the PARAPHRASE query
    rank=$(rank_of "$key" "${QUERIES[$key]}" | grep -F "$tdoc" | head -1 | cut -d' ' -f1)
    [ -z "$rank" ] && rank=">10"
    printf "  %-12s target=%s  rank=%s\n" "$key" "$tdoc" "$rank"
    echo "$key,$run,$tdoc,$rank" >> "$ROOT/results/recall-scale.csv"
  done
done

echo ""
echo "=== SUMMARY (recall@k over all runs) ==="
"$NODE" -e '
  const fs=require("fs");
  const rows=fs.readFileSync(process.argv[1],"utf8").trim().split(/\r?\n/).slice(1).map(l=>l.split(","));
  const total=rows.length; let at1=0,at5=0,at10=0;
  for(const r of rows){const rk=r[3]===">10"?999:parseInt(r[3]); if(rk<=1)at1++; if(rk<=5)at5++; if(rk<=10)at10++;}
  const pct=x=>(100*x/total).toFixed(0)+"%";
  console.log(`  queries: ${total}`);
  console.log(`  recall@1:  ${at1}/${total}  ${pct(at1)}`);
  console.log(`  recall@5:  ${at5}/${total}  ${pct(at5)}`);
  console.log(`  recall@10: ${at10}/${total}  ${pct(at10)}`);
' "$ROOT/results/recall-scale.csv"
