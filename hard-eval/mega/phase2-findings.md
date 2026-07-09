# Phase 2 — per-call metrics on the 46,656-frame maximal mega-brain (12 projects)

## Latency per call (measured 3 ways; honest)
| Tool | MCP per-call | serve (true warm) | doc claim | verdict |
|---|---|---|---|---|
| sym (symbol-exact) | 0-1 ms | — | #12 <1ms | GREEN (holds at 46.6k) |
| list_concepts | 13 ms | — | #100 graph | GREEN |
| recall_fix (float rerank) | 2,128 ms | — | — | AMBER (2s; re-encodes candidates) |
| ask (semantic) | ~3,100 ms | ~400-460 ms | #10 0.3ms/100ms warm | RED vs claim: true warm ~450ms (NOT 0.3ms; that's the raw fp-scan microbench), MCP adds ~2.5s per-call setup |
| search (lexical) | ~2,700 ms | ~370 ms | #11 0.03ms | RED vs claim: warm ~370ms (0.03ms is the trigram-lookup microbench, not a full query) |

## HONEST CLAIM GAP (report as amber/red, not green)
- Doc claims #10 (0.3ms semantic / 100ms warm) and #11 (0.03ms lexical) are MICROBENCHMARKS of the
  inner scan on SMALL brains (5k-20k). At 46,656 frames the TRUE warm per-query latency is ~370-460ms
  (serve mode), and via the MCP server ~2.7-3.1s (per-call cache/lock/setup overhead the hot serve
  loop skips). So: sym/symbol-exact IS sub-ms at scale (claim holds); semantic ask/search are
  sub-second warm but ~1000x the microbench claim -- the claim is real for the SCAN, not the full call.
- ACTION: the showcase reports BOTH the microbench claim AND the measured full-call latency, labelled.
  Also a real optimization target: the MCP handler's per-call overhead (2.5s) vs serve (0.45s) is a gap
  worth closing (warm the corpus cache once, hold it).

## Compression (measured)
- brain = 163 MB (post-metrics; grew from 132 as recall built caches + learn_fix) for 46,656 frames
  = ~3,666 bytes/frame INCLUDING full source text + fingerprint + symbol + trigram.
- 1-bit fingerprint vs 128-dim float32: 512B -> ~24B = ~21x smaller per vector (claim #1 says 500x vs
  raw float embeddings at 768-dim; at our 128-dim it's ~21x -- honest: the 500x is 768-dim float vs
  64-bit fp; ours is 128-dim).

## Scale headline (the "how far" answer)
12 projects (Vivere/dt-storefront/Agents/Advisory/MCP/SDK-DT/said-build/dt-gateway/Law/Wonga/AlphaGo/
NeoroCore) -> 46,656 frames in ONE 132-163MB portable brain, peak RAM <=1016MB steady. WALL = SAID-ECHO
(37k files) hit 2696MB + 20min timeout. Recall cross-project verified (Rust/C#/SQL/docs all recalled).
