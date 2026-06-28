# 14 — Novel mechanisms

Most memory systems are a **vector database + BM25 + a reranker**. SAID is a different architecture: every retrieval or cognitive operation is a deliberate mathematical departure from the standard. This section lays out each one, with the formula, the measured benchmark impact, and the literature it relates to.

Nothing here is marketing. Every claim in the **Shipped** section has either a measured number on a public benchmark (MTEB LongEmbed / LoCoMo / SummScreenFD / QMSum / NarrativeQA / BEIR) or a formula whose correctness can be audited against the source. Everything in the **Horizon** section is honestly labelled as unshipped.

## Index

### Shipped (in production today)

- [14.1 — Binary latent retrieval with asymmetric QJL rerank](14.1-qjl-asymmetric.md)
- [14.2 — `S_slow`: rank-1 outer-product accumulator](14.2-s-slow.md)
- [14.3 — Relative-cutoff retrieval](14.3-relative-cutoff.md)
- [14.4 — Three-engine max-confidence fusion](14.4-max-fusion.md)
- [14.5 — Latent-drift auto-dream trigger](14.5-auto-dream.md)
- [14.6 — Two-signal surprise / reconsolidation kernel](14.6-surprise.md)
- [14.7 — Retrieval-time graph reconstruction (Layer 6)](14.7-layer6-graph.md)
- [14.8 — Matryoshka sign-bit binarization preserves 1.0 on long-context MTEB](14.8-matryoshka-signbit.md)
- [14.14 — Byte-exact tombstone restore with BLAKE3-chained audit](14.14-byte-exact-restore.md) — the retention + verification moat
- [14.15 — Canon memory (the 80/20 split, stored once, rendered per language)](14.15-canon-memory.md) — store the 80% framework canon as language-neutral structured-NL intent; recall guides the LLM to render it in the active language. The structural-reuse moat: one canon → every language, agent writes only the 20%.

### Horizon (designed, mathematically grounded, not yet shipped)

- [14.9 — Holographic K-view fingerprint](14.9-holographic-k-view.md)
- [14.10 — Latent PageRank via `S_slow](14.10-latent-pagerank.md)`
- [14.11 — Frame-level differential privacy](14.11-differential-privacy.md)
- [14.12 — Crystalline annealing](14.12-crystalline-annealing.md)
- [14.13 — Query-side continuous rehearsal](14.13-query-rehearsal.md)

### Cross-cut

- [Cross-reference matrix](cross-reference.md) — mechanism → benchmark it moves → roadmap item

## Why each mechanism matters as a group

A standard retrieval stack uses latent space for exactly one thing: semantic search. SAID uses the same 64-bit latent space for retrieval (14.1, 14.8), ranking (14.4, 14.3), prior shaping (14.2), consolidation triggering (14.5), dedup, reconsolidation (14.6), and graph traversal (14.7). **Reuse of one mathematical medium is the architectural thesis.** The horizon items extend the same idea: PageRank in latent space (14.10), DP in latent space (14.11), basis re-optimization in latent space (14.12), rehearsal in latent space (14.13).

Separate axis: **14.14 byte-exact tombstone restore** lives outside the latent-space thesis. It's an enterprise-compliance primitive — retention + BLAKE3-chained audit + legal-hold-aware sweep — that no other memory system (mem0, Zep, Cognee, Dume, LEANN, Mem.ai, Hindsight, MemoryLake, usecortex) offers. This is the moat shaped like M365 Recycle Bin + SOX audit trail + GDPR right-to-erasure, all at frame-of-memory granularity.

## Template (every mechanism page follows this shape)

1. **Plain-English** — what it does in two sentences.
2. **Formal statement** — the math, typeset in LaTeX.
3. **Proof sketch** (where novelty is contested) — why the math works.
4. **Source** — file + line range in the repository.
5. **Measured impact** — benchmark delta, if shipped.
6. **Literature positioning** — related work + what's genuinely new.
7. **Known limitations** — honest list.

## How to use this section

- **If you're a SAID contributor:** read 14.1 through 14.8, plus 14.14, to understand why the codebase looks the way it does.
- **If you're evaluating SAID against mem0 / Zep / Cognee:** the [cross-reference matrix](cross-reference.md) is the one-page scoreboard.
- **If you're an academic auditor:** every formula has a source-file citation. The proof sketches are deliberately terse; run the benchmark harness if you want numbers.
- **If you're a product manager:** the four horizon items (14.10, 14.11, 14.9, 14.12) are on [12-roadmap.md](../12-roadmap.md) as concrete work items in priority order.

