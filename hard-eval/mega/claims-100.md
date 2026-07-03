# .said — 100 measurable claims (docs 09-36 + file-format + novel-mechanisms)
# Each: claim | source | number/mechanism | how-to-measure. Gate .said's measured results against these.
# Categories: COMPRESSION, FILE-FORMAT, RECALL, INGESTION, MEMORY-MODEL, NOVEL-MECHANISMS,
#             SCOPING, MULTI-AGENT, TOKEN-VALUE, BENCHMARKS, PLATFORM, INTEGRATIONS, PRODUCTION, MOAT.
# Extracted 2026-07-03 (subagent, exhaustive read). 87/100 have exact numbers. ~70 shipped, ~15 horizon.
#
# The full 100-row table is preserved in the session transcript + will be rendered into the mega
# showcase claim-gate. Headline gate targets pulled forward here for the checkpoint:

## Headline gate targets (highest-value to independently re-measure this run)
COMPRESSION/SIZE
 - #1  1-bit fingerprint 500x smaller than float embeddings
 - #9  134MB markdown -> 6MB .said (22x); distilled 0.2MB
 - #62 v7_1 header = 72 bytes; sections SYMS/TRGM/WIDX/BLKT/FTOC/DICT/SCRM/BRAN/AUDT
RECALL / LATENCY (per-call, what owner wants)
 - #10 semantic recall 0.3ms@20k / ~100ms warm@5340
 - #11 lexical 0.03ms  | #12 sym <1ms  | #13 get 4ms
 - #14-17 MTEB: Needle/WikimQA NDCG@10=1.0; SummScreenFD 0.9797; QMSum 0.8922
 - #18 recall@5 = 100% @N=1000
 - #21/#44 float rerank: recall@1 2/5->5/5; recall@10 @N=200 0.195->0.855
 - #20 adversarial twin 7/7 + 5/5 decoys separated
INGESTION / STREAMING
 - #6  budget clamp(RAM*12%,16MB,512MB)  | #7 peak 161MB@4.5k / 220MB@8.7k
 - #36 Wonga 37,790 mem / 14,560 sym; read 200-370s->14s
 - #19 incremental 160-190ms/store, flat not O(N^2)
TOKEN VALUE (per-call, across the board)
 - #57 small locate 1715->106 tok (16x)  | #58 bug-loc 8587->86 (100x)  | #59 sweep 125k->134 (930x)
 - #60 100MB markdown 33.6M tok/session -> 524 tok slice (64,000x)
NOVEL MECHANISMS  #41 QJL asym  #42 Matryoshka signbit  #43 holographic 16-view  #45 S_slow  #47 salience
SCOPING/MULTI-AGENT  #51 project scope  #53 procedural federate  #54 fleet shared byte-exact  #55 delete-by-project
MOAT  #94 compaction 7/7 byte-exact  #95 .said 10/10 vs in-band 0/10  #96 effort-decay 80% free
BENCHMARK GATES  #84 MTEB gates  #85 LoCoMo R@10>=0.554  #87 pass@k n>=5  #89 abstention F1

## vs published (arxiv-research.md): LoCoMo Mem0=92.5, LongMemEval Mem0=94.4/LiCoMemory 73.8%,
## <7k tok/retrieval. .said gate: R@10, NDCG, abstention-accuracy, tokens/call -- same metrics.
