# Competitor matrix

One table, every system we track, every axis we care about. Empty cells mean **not yet measured** — not "worse than them."

Source of claims about competitors: their public docs + [12-roadmap.md § Competitor parity sweep](../12-roadmap.md). **We have NOT independently run most of these systems yet.** Where we have (Mem0 via `competitor_bench` harness + LoCoMo oracle), the number is cited. Everything else is a placeholder to fill in.

Last updated: 2026-04-23

## Axes (what each column means)


| Axis                  | What we measure                                      | How                                                                         |
| --------------------- | ---------------------------------------------------- | --------------------------------------------------------------------------- |
| Retrieval accuracy    | LoCoMo R@10 + oracle F1 on 20-QA sample              | `[competitor_bench](../../../crates/sca-core/examples/competitor_bench.rs)` |
| Latency (p50 / p99)   | Query → result, ms, warm cache, 10k-frame corpus     | Same harness                                                                |
| Binary / install size | What ships to the user's machine                     | Packaged release                                                            |
| Offline               | Does it work with no internet?                       | Docs + test                                                                 |
| Format coverage       | PDF / DOCX / XLSX / MBOX / code / audio / video / DB | Roadmap § ingestion                                                         |
| Temporal reasoning    | Time-aware scoring of Episodic frames                | LoCoMo Cat 3                                                                |
| Graph                 | Explicit entity/relation graph?                      | Docs                                                                        |
| Pillar model          | Typed memory (Episodic/Semantic/…)                   | Docs                                                                        |
| Ecosystem             | SDKs, framework adapters, IDE integrations           | Docs                                                                        |
| BYO-LLM               | Does the memory system itself call an LLM?           | Source/docs                                                                 |
| License               | OSS / commercial / hybrid                            | Docs                                                                        |


## The matrix


| System             | R@10      | F1        | Latency p50 | Size             | Offline           | Formats                                     | Temporal         | Graph                  | Pillars       | Ecosystem                                | BYO-LLM | License               |
| ------------------ | --------- | --------- | ----------- | ---------------- | ----------------- | ------------------------------------------- | ---------------- | ---------------------- | ------------- | ---------------------------------------- | ------- | --------------------- |
| **SAID (v0.2)**    | **0.554** | **0.856** | < 15ms      | ~70 MB           | **Yes**           | PDF/DOCX/MD/code/audio                      | planned          | fan-out only           | **5 pillars** | MCP + CLI                                | **Yes** | MIT                   |
| Mem0               | —         | 0.684     | —           | SaaS             | No                | —                                           | partial          | Yes                    | No            | CrewAI / LangGraph / LangChain / AutoGen | No      | Apache-2 (OSS) + SaaS |
| Zep / Graphiti     | —         | —         | —           | SaaS + self-host | partial           | —                                           | **Yes (strong)** | **Yes (temporal KG)**  | No            | LangChain, LlamaIndex                    | No      | Apache-2 + SaaS       |
| MemoryLake         | —         | —         | —           | Lib              | —                 | —                                           | —                | —                      | —             | multi-LLM bridge                         | —       | —                     |
| Cognee             | —         | —         | —           | Lib + SaaS       | partial           | docs-heavy                                  | —                | **Yes (full KG)**      | No            | LangChain                                | No      | Apache-2 + SaaS       |
| Hindsight          | —         | —         | —           | OSS              | Yes               | —                                           | —                | **Yes (4-net hybrid)** | partial       | —                                        | —       | MIT                   |
| usecortex.ai       | —         | —         | —           | SaaS             | No                | voice + code                                | —                | —                      | —             | voice agents                             | No      | Commercial            |
| Rewind / Limitless | —         | —         | —           | Native app       | Yes (local-first) | screen / audio / video                      | —                | No                     | No            | macOS / wearable                         | No      | Commercial            |
| Dume.ai            | —         | —         | —           | SaaS             | No                | ~6 native + Composio's 500 via MCP rebadge  | —                | —                      | —             | chat UX over Composio                    | No      | Commercial            |
| LEANN              | —         | —         | —           | Python lib       | **Yes**           | mail/history/imessage/slack/code/docs/imgs  | —                | —                      | flat          | apps/* offline readers                   | partial | MIT                   |
| Composio (infra)   | n/a       | n/a       | —           | SaaS gateway     | No                | 500+ toolkits, 20,000+ tools                | —                | —                      | n/a           | MCP gateway + SDKs                       | n/a     | Apache-2 (core) + SaaS |
| Mem.ai             | —         | —         | —           | SaaS             | No                | notes                                       | —                | Yes (note-linking)     | No            | Web / mobile app                         | No      | Commercial            |
| Lindy              | —         | —         | —           | SaaS             | No                | workflow                                    | —                | —                      | rule-based    | workflow platform                        | No      | Commercial            |
| myNeutron          | —         | —         | —           | IDE plugin       | —                 | code only                                   | —                | —                      | —             | IDE                                      | No      | Commercial            |


**Legend:** `—` = not yet measured / not publicly documented. Fill in from the `competitor_bench` harness output as each competitor is wired up.

## How to fill a row

For any competitor row, the minimum measurement run:

1. Install the competitor per their quickstart.
2. Ingest the same 1000-doc corpus SAID uses in `competitor_bench` (see `[realworld-probes.md](realworld-probes.md)`).
3. Run the 20-query LoCoMo sample + 50-query MTEB WikimQA sample.
4. Log F1 (oracle-judged with Claude Opus 4.7), R@10, p50 latency, p99 latency.
5. Record package/install size.
6. Copy results into this table, preserving the **last-measured date** in a footnote per row.

Rows that cannot run offline, or require paid accounts beyond a free tier, get a note explaining the partial measurement.

## Where SAID clearly wins today

- **F1 on LoCoMo oracle sample:** 0.856 vs Mem0 0.684 (measured 2026-04-21).
- **Offline-first integrations:** LEANN validated the offline-first playbook in Python (mail / browser / imessage / slack / code from local files with no OAuth). SAID implements the same playbook in Rust with 64-bit fingerprint retrieval — see [13 integrations](../13-integrations.md) for the Q2 six-crate shipping plan.
- **BYO-LLM:** no LLM in the memory-system hot path. LLM-using work (dream v3, KG build) lives in a separate [`said-think`](../13-integrations.md#said-think--the-one-online-component) process — audit story is "run `ls`, see if it's installed."
- **Binary size:** 70 MB ships vs hundreds of MB of Python deps + transformer weights.
- **Pillar model:** 5 typed pillars vs most competitors being flat.
- **Latent-space retrieval:** 64-bit binary fingerprints with XOR + popcount Hamming vs 768-float dense + cosine. Measured (2026-04-23, scalar fallback, no SIMD): **0.08 ms encode + 0.07–2.0 ms per-query Hamming search**, 8 bytes/frame on disk — 384× smaller than dense and ~100× faster per comparison. Full math + numbers: [3.8 latent space](../03-core-subsystems/3.8-latent-space.md).
- **Single-file portability:** one `.said` file holds content + index + fingerprints + audit log + pillar metadata. Competitors keep vector DB + relational store + graph store separate.
- **Byte-exact tombstone restore with BLAKE3-chained audit:** every prior version of every frame is retained through a two-step Tombstone → Deleted lifecycle. `admin restore <doc_id>` reactivates the newest tombstone; restored content is byte-identical to the original, verified by cryptographic digest match. Legal-hold-aware retention sweep. First memory system in the category (mem0, Zep, Cognee, Dume, LEANN, Mem.ai, Hindsight, MemoryLake, usecortex) to ship this. Full mechanics: [14.14 byte-exact restore](../14-novel-mechanisms/14.14-byte-exact-restore.md). Not compression magic — retention + verification.

## Where SAID does not yet compete

- **Temporal reasoning:** Zep/Graphiti are built for this. We have planned temporal scoring; LoCoMo Cat 3 is currently 0.391.
- **Pre-materialised typed knowledge graph:** Cognee and Hindsight extract `(subject, relation, object)` triples at ingest time. We have a **retrieval-time entity graph** (Layer-6 bridge walk — see [3.9 Graph layer](../03-core-subsystems/3.9-graph-layer.md)) which matches on multi-hop factual accuracy (1.0 on WikimQA) but loses on exhaustive enumeration, typed-relation filters, and at-scale traversal cost. Opt-in `said build-graph` (via the [`said-think`](../13-integrations.md#said-think--the-one-online-component) agent) is on the [roadmap](../12-roadmap.md).
- **Ecosystem breadth (frameworks):** Mem0 has adapters for CrewAI, LangGraph, LangChain, AutoGen. We have CLI + MCP — MCP is itself the integration surface that matters for Claude Code / IDE agents, but we haven't shipped first-party framework glue yet.
- **Screen/video capture:** Rewind / Limitless' core moat. We don't, and per Rule 1 in [13-integrations.md](../13-integrations.md) we would only ever ship this as an offline-first local capture daemon.
- **Cloud-aggregated breadth:** Dume / Composio route user data through their cloud to hit 500+ apps. Per our offline-first / BYO-LLM stance (see [13-integrations.md](../13-integrations.md)), this is **intentionally not our positioning**. We target ~12-15 deep, local-first integrations in year one, matching LEANN's validated playbook rather than Composio's aggregator model.

## Where we don't yet know

Every cell marked `—` above. Highest-leverage next measurements:

1. **Mem0 R@10 on the same 1982-QA LoCoMo corpus** — we have F1 on a 20-QA sample only.
2. **Zep latency p50** — published as "sub-second" but not measured head-to-head.
3. **Hindsight accuracy benchmarks** — they publish on benchmarks we don't run; we should add those.
4. **Cognee KG-build time + retrieval latency** — different shape (build-then-query) that might dominate at scale.

## Action: run the sweep

This page exists to be filled in. Fill order — highest information gain first:

- Mem0 full-corpus sweep (we only have the 20-sample F1)
- Zep / Graphiti (directly addresses our weakest category)
- Hindsight (claimed accuracy leader — verify)
- Cognee (different architecture — useful reference)
- Rewind / Limitless (different use case — positioning, not direct competition)
- MemoryLake, usecortex.ai, Dume.ai, Mem.ai, Lindy, myNeutron (lower priority or different product shape)

On completion: this page updates, and the corresponding roadmap checkbox in [12-roadmap.md § Overall parity roadmap item](../12-roadmap.md) ticks.

## See also

- [Row 49 — Competitor benchmark harness](../05-features/row-49-competitor-bench.md)
- [competitor_bench](../../../crates/sca-core/examples/competitor_bench.rs) — current harness
- [12-roadmap.md § Competitor parity sweep](../12-roadmap.md) — per-competitor action items
- [realworld-probes.md](realworld-probes.md) — the corpus used in these comparisons

