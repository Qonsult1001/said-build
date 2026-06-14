# BEIR short-doc sanity

MTEB LongEmbed is the main retrieval yardstick, but long-context success can mask short-document regressions. BEIR's short-passage retrieval tasks are the opposite stress case and exist to keep us honest.

## What we check

We do NOT target BEIR leaderboard numbers — those are dominated by dense transformer models with 100× our parameter count. What we do check:

1. **Monotonic non-regression.** Every shipment against the canonical BEIR slice (NFCorpus, FiQA, SCIDOCS) must stay within ±0.01 NDCG@10 of the prior measurement.
2. **No mode collapse.** If a change makes MTEB jump by +0.05 and BEIR drops by −0.10, the change has over-fit the long-context path. Revert or bisect.

Current scores (2026-04-16, unchanged through pillar rollout):

| Task | NDCG@10 |
|------|---------|
| NFCorpus | 0.289 |
| FiQA-2018 | 0.245 |
| SCIDOCS | 0.172 |

Not competitive with retrieval-specialised dense models (NFCorpus SOTA is ~0.38). But SAID's tradeoff is deliberate: a 4.8 MB encoder that works offline on mobile vs a 440 MB encoder that needs a GPU. Within that class, SAID is at or near the top.

## Running

```bash
cd crates/sca-core
cargo run --release --example beir_short --features static-embed -- nfcorpus
```

(If the BEIR harness isn't on your current branch, the short-doc slice is folded into `realworld_recall_probe`; see [realworld-probes.md](realworld-probes.md).)

## Why we keep it around despite not competing

Short-document retrieval is the bread-and-butter of every "chatbot with memory" competitor (mem0, memvid, Zep, LangMem). If our competitor benchmark ([Row 49](../05-features/row-49-competitor-bench.md)) ever starts skewing because BEIR-equivalents regress, this check catches it early.

## See also

- [MTEB](mteb.md) — the long-context track
- [Real-world probes](realworld-probes.md) — mid-sized corpora
- [Row 49 — Competitor benchmark harness](../05-features/row-49-competitor-bench.md)
