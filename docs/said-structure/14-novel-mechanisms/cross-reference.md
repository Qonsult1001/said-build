# Cross-reference matrix

One page mapping each novel mechanism → which benchmark(s) it moves → which roadmap item implements it (for horizon) or which source file it lives in (for shipped).

## Shipped mechanisms × benchmarks


| §                                    | Mechanism                    | MTEB Needle/Passkey | MTEB WikimQA    | SummScreenFD       | QMSum       | NarrativeQA | LoCoMo R@10 | LoCoMo Cat 2 | LoCoMo Cat 3 | Chambers                                                                                                      |
| ------------------------------------ | ---------------------------- | ------------------- | --------------- | ------------------ | ----------- | ----------- | ----------- | ------------ | ------------ | ------------------------------------------------------------------------------------------------------------- |
| [14.1](14.1-qjl-asymmetric.md)       | QJL asymmetric rerank        | —                   | +0.02 A/B       | +0.01              | +0.005      | +0.01       | 0           | 0            | 0            | 2                                                                                                             |
| [14.2](14.2-s-slow.md)               | `S_slow` accumulator         | —                   | —               | +0.015             | +0.002      | +0.005      | +0.01       | +0.005       | 0            | 2                                                                                                             |
| [14.3](14.3-relative-cutoff.md)      | Relative cutoff              | —                   | —               | +0.003             | +0.001      | +0.005      | 0           | 0            | 0            | 3                                                                                                             |
| [14.4](14.4-max-fusion.md)           | Max-confidence fusion        | base → 1.0          | base → 1.0      | base → 0.98        | base → 0.89 | base → 0.72 | base → 0.55 | base → 0.62  | base → 0.39  | 15                                                                                                            |
| [14.5](14.5-auto-dream.md)           | Auto-dream trigger           | —                   | —               | +0.02 on streaming | —           | —           | 0           | 0            | 0            | 1                                                                                                             |
| [14.6](14.6-surprise.md)             | Surprise kernel              | —                   | —               | —                  | —           | —           | 0           | 0            | 0            | 4 (reconsolidation)                                                                                           |
| [14.7](14.7-layer6-graph.md)         | Layer-6 graph                | —                   | **+0.28 → 1.0** | 0                  | 0           | +0.05       | +0.02       | **+0.15**    | 0            | 3                                                                                                             |
| [14.8](14.8-matryoshka-signbit.md)   | Sign-bit preservation        | **= 1.0**           | **= 1.0**       | **= 0.98**         | = 0.89      | = 0.72      | = 0.554     | = 0.617      | = 0.391      | all                                                                                                           |
| [14.14](14.14-byte-exact-restore.md) | Byte-exact tombstone restore | —                   | —               | —                  | —           | —           | —           | —            | —            | **4** (restore round-trip, legal-hold blocks retention, audit chain break detection, retention-sweep respect) |


**Reading the table.** A cell labelled "+0.XX" is the measured contribution against an ablation where that mechanism is disabled. "base → Y" means the mechanism gets the benchmark from ~0 or an unusable baseline up to $Y$. "—" means no measurable contribution on that benchmark. Chambers column is the count of protected behaviours (see [10-benchmarks/chambers.md](../10-benchmarks/chambers.md)).

## Horizon mechanisms × predicted benchmark lift


| §                                       | Mechanism             | Effort                  | Expected benchmark lift                                           | Roadmap priority                            |
| --------------------------------------- | --------------------- | ----------------------- | ----------------------------------------------------------------- | ------------------------------------------- |
| [14.9](14.9-holographic-k-view.md)      | Holographic K-view    | ~2 weeks                | LoCoMo +0.01–0.03, SummScreenFD +0.005–0.02                       | Medium                                      |
| [14.10](14.10-latent-pagerank.md)       | Latent PageRank       | **~1 day**              | LoCoMo Cat 2 +0.03, NarrativeQA +0.03                             | **High (smallest effort, measurable lift)** |
| [14.11](14.11-differential-privacy.md)  | Differential privacy  | ~2 weeks + paper        | (utility −0.001 to −0.005) trade for formal (ε, δ)-DP             | **High (only differentiator)**              |
| [14.12](14.12-crystalline-annealing.md) | Crystalline annealing | ~1 week                 | Large-brain fingerprint drift fix; enables 16/32-bit fingerprints | Medium                                      |
| [14.13](14.13-query-rehearsal.md)       | Query rehearsal       | ~1 week + bench harness | Session-only lift; not on current benchmarks                      | Low (wait-and-see)                          |


## Which mechanism addresses which limitation

From [11-known-limitations.md](../11-known-limitations.md):


| Limitation                    | Addressed by                                                                             | Status              |
| ----------------------------- | ---------------------------------------------------------------------------------------- | ------------------- |
| 1.1 LoCoMo temporal at 0.391  | Temporal scoring (roadmap) + [14.10 latent PageRank](14.10-latent-pagerank.md) (partial) | Planned             |
| 1.2 NarrativeQA at 0.721      | [14.10 latent PageRank](14.10-latent-pagerank.md), deeper Layer-6 walk                   | Planned             |
| 1.3 Static relative cutoff    | Dynamic cutoff variant of [14.3](14.3-relative-cutoff.md)                                | Planned             |
| 1.5 No Unicode trigrams       | Orthogonal — not a novel-mechanism issue                                                 | Planned             |
| 2.1 Factual→Semantic collapse | Orthogonal — code sweep                                                                  | Planned             |
| 2.2 No Episodic distillation  | [14.5 auto-dream](14.5-auto-dream.md) v3 + BYO-LLM                                       | Planned             |
| 3.1 Dream v1 regression       | [14.5 auto-dream](14.5-auto-dream.md) v2 shipped; v3 needs LLM                           | Partially addressed |
| 3.2 Dream threshold time-only | Adaptive threshold in [14.5](14.5-auto-dream.md)                                         | Planned             |
| 4.1 Surprise is heuristic     | Optional ML scorer in [14.6](14.6-surprise.md)                                           | Planned             |
| Privacy (not yet flagged)     | **[14.11 differential privacy](14.11-differential-privacy.md)**                          | Horizon             |


## One-line summary per mechanism


| §                                       | One-line what-it-is                                            | One-line why-novel                                                                                  |
| --------------------------------------- | -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| [14.1](14.1-qjl-asymmetric.md)          | Doc is sign bits, query is f32, QJL rerank                     | Only shipping memory system using it                                                                |
| [14.2](14.2-s-slow.md)                  | 64×64 rank-1 accumulator of corpus second moment               | Continuous-time attractor in latent space                                                           |
| [14.3](14.3-relative-cutoff.md)         | Keep r ≥ 0.3 × top, semantic floor = 3                         | Scale-invariant cross-engine cutoff                                                                 |
| [14.4](14.4-max-fusion.md)              | Engine fusion = `max(σ_A, σ_B, σ_C)` with banded confidences   | No hyperparameters, unlike RRF                                                                      |
| [14.5](14.5-auto-dream.md)              | Dream fires on query-drift + $\sqrt{N}$-scaled counter         | Zero-inference, self-tuning consolidation                                                           |
| [14.6](14.6-surprise.md)                | Two-scalar (similarity, overlap) → Benign/Update/Contradiction | Smallest-possible reconsolidation kernel                                                            |
| [14.7](14.7-layer6-graph.md)            | Graph reconstructed at query time from top-3 doc entities      | Zero-storage multi-hop at 1.0 WikimQA                                                               |
| [14.8](14.8-matryoshka-signbit.md)      | 64-bit fingerprints preserve 1.0 on LongEmbed                  | Empirical research result, measurable                                                               |
| [14.14](14.14-byte-exact-restore.md)    | Byte-exact restore with BLAKE3-chained audit + legal holds     | First memory system in its category to ship this. Not compression magic — retention + verification. |
| [14.9](14.9-holographic-k-view.md)      | $K$ rotated fingerprints, median Hamming                       | Effective 1024-bit at 128 bytes                                                                     |
| [14.10](14.10-latent-pagerank.md)       | PageRank on `S_slow`'s eigenstructure                          | PageRank in latent space, not on a graph                                                            |
| [14.11](14.11-differential-privacy.md)  | Deterministic hash-seeded fingerprint dithering                | First formally-DP memory-system primitive                                                           |
| [14.12](14.12-crystalline-annealing.md) | Periodic eigenbasis re-alignment of fingerprints               | First streaming representation-space update                                                         |
| [14.13](14.13-query-rehearsal.md)       | Subspace projection blend with recent queries                  | Gradient-free in-session sharpening                                                                 |


## See also

- [README.md](README.md) — framing
- [11-known-limitations.md](../11-known-limitations.md) — limitations that horizon items address
- [12-roadmap.md](../12-roadmap.md) — concrete tickets
- [10-benchmarks/README.md](../10-benchmarks/README.md) — benchmark definitions

