# arXiv / published memory-benchmark research — gate targets + hard cases

## Published benchmarks (gate .said against these)
| Benchmark | Size | Categories | Best reported (2025-26) | Metric |
|---|---|---|---|---|
| **LoCoMo** | 1,540 Q / 10 dialogues | single-hop, multi-hop, temporal, open-domain | Mem0 = 92.5 | accuracy |
| **LongMemEval** | 500 Q | info-extract, multi-session, temporal, knowledge-update, abstention | Mem0 = 94.4; LiCoMemory 73.8% acc / 76.6% recall (gpt-4o-mini) | accuracy / recall |
| **BEAM** | 1M & 10M token scale | multiple | Mem0 = 64.1 / 48.6 (1M/10M) | accuracy |
| Retrieval quality (all) | — | — | — | Hit@K, MRR, NDCG, P@K, **R@K**, F1@K |
| Token efficiency | — | — | Mem0 < 7,000 tokens/retrieval | tokens/call |

## The 5 LongMemEval hard-case CATEGORIES (add these test types to our battery)
1. **Information extraction (IE)** — single-hop: recall a specific stored fact.
2. **Multi-session reasoning (MS)** — compose an answer across multiple sessions.
3. **Temporal reasoning (TR)** — HARDEST: track dates/ordering, answer with the right timeline.
4. **Knowledge update (KU)** — a fact CHANGED over time; must use the LATEST, not the stale one.
5. **Abstention (ABS)** — the query asks about info that was NEVER stored; must DECLINE ("I don't know"),
   NOT fabricate. "The failure mode most retrieval evals ignore." (.said already has an abstention path.)

## Open problems named in the literature (honest framing for our report)
cross-session identity, temporal abstraction at scale, memory staleness.

## How this feeds the .said mega-test
- Build a LongMemEval-STYLE battery: N items per the 5 categories (IE/MS/TR/KU/ABS), measure
  recall@1/@5/@10 + abstention-accuracy per category, and PLACE .said's numbers next to the published
  Mem0/LongMemEval bars (honest: our corpus is code+docs, theirs is chat — comparison is on the
  METRIC + method, framed as "same discipline", not a claim of identical dataset).
- Sources: mem0.ai/blog/ai-memory-benchmarks-in-2026, arxiv 2410.10813 (LongMemEval),
  emergentmind LoCoMo/LongMemEval, LiCoMemory.
