# What `.said` is

## One-paragraph elevator

`.said` is a **single-file portable brain**: a self-contained memory store for agents, humans, and tools that indexes documents, code, conversations, and pointers into one mmap-friendly binary. Instead of running a vector DB + SQLite + LLM + embedder (four services, four network hops), you open one `.said` file and get semantic search (**64-bit binary latent space, XOR + popcount retrieval — see [3.8 latent space](../03-core-subsystems/3.8-latent-space.md)**, ~0.08 ms encode + ~2 ms search on a 300-doc corpus, measured), lexical search (trigram + BM25), symbol lookup (AST-indexed), lineage-preserved edits (byte-exact checkout by version), and a passive-learning brain state (S_slow tensor, recall-weight decay, dream drift) — all offline, all BYO-LLM, all in a file you can copy to a USB stick.

## What it is not

- **Not a vector database.** It's a file format, not a service. No network, no auth layer, no daemon unless you choose to run the MCP server wrapper.
- **Not an LLM.** The binary itself never calls an LLM. Content consolidation and answer generation live in the caller's LLM at read time — the Prometheus pattern.
- **Not an embedding-only store.** Every frame also carries full text, full lineage, and full trigram + symbol index coverage, so lexical / symbol / grep retrieval works without any embedding math.
- **Not a knowledge graph.** Relationships are implicit via fingerprint similarity + shared tags, not explicit `(subject, relation, object)` edges. A graph layer could be added as a plugin but isn't part of the core.

## How it compares to the usual stack

| Capability | Vector DB + SQLite + LLM + embedder | `.said` |
|---|---|---|
| Semantic search | Requires embedder + vector DB + network | 1-bit SCA fingerprint, in-file, 0.3 ms |
| Lexical / grep | Separate tool (ripgrep, Elasticsearch) | Trigram index inside the file, 0.03 ms |
| Symbol lookup | Separate tool (LSP, ctags) | AST-indexed at ingest, cached in-file, <1 ms |
| Byte-exact version restore | Manual (git, backups) | `said checkout --version N` returns the bytes |
| Offline operation | Rare (most cloud-dependent) | Always — everything ships in the file |
| Single-file portability | No — it's a stack, not a file | Yes — `.said` is the entire brain |
| Passive learning | Retraining / fine-tuning | Brain state auto-updates on every query |
| Cross-document synthesis | Requires a graph layer | S_slow tensor encodes co-occurrence implicitly |
| Encrypted per-frame | Usually column-level or full-DB | AES-256-GCM per frame behind an optional feature |
| Indexable formats | Depends on plugins | PDF, DOCX, TXT, MD, MP4, MP3, source code (7 languages), SQL, OCR-scanned images — all via optional features |

## Core value props (ranked)

1. **Zero infra.** One file replaces four services. Your agent's memory is a file path, not a stack.
2. **Portable.** Copy to USB, drop on another machine, it works. No rebuild of indices, no re-embedding.
3. **Fast.** 0.3 ms semantic, 0.03 ms grep, <1 ms symbol lookup. 7 μs repeat frame lookup (mmap + block cache).
4. **BYO-LLM.** The binary has no LLM dependency at any layer. Users plug in their own model at read time.
5. **Auditable.** Every mutating action is logged in an append-only BLAKE3-chained section (`AUDT`). Tombstone lineage is byte-exact-restorable.
6. **Open file format.** Every byte is documented in [`../02-file-format/`](../02-file-format/). No proprietary blobs.

## Two deployment modes (immutable at creation)

- **Portable** — embeds full content, works offline, USB-friendly. Default. For personal brains, single-agent memories, portable knowledge bases.
- **Enterprise** — refuses content-embed ingests, stores URI + summary pointers only. Content stays in the system of record (SharePoint, S3, corporate doc store). For regulated environments, content-leak containment, compliance audits.

The choice is **licensed and immutable** — a Portable brain cannot be upgraded to Enterprise in place, and vice versa. See [Row 37 — immutable deployment mode](../05-features/row-37-brain-mode.md).

## Pillars (where memories live)

| Pillar | Purpose | Retrieval ranking hint |
|---|---|---|
| **Episodic** | Raw turns, session events, append-only | `SCA_score × exp(-age_hours / 24)` — recent matters more |
| **Semantic** | Distilled facts (usually caller-written at read time) | `SCA_score × confidence × (1 - decay)` |
| **Procedural** | Action recipes (trigger + steps + outcome) | `task_match × success_rate` (Voyager-style) |
| **External** | Pointers or embedded references to documents / URLs | Metadata filter + schema query |
| **Code** | AST-chunked source | Standard SCA + symbol + grep |
| **Memory** | Legacy catch-all (migrates to Semantic / Episodic over time) | Default pillar for pre-Decision-1 files |

See [four-pillar memory](../04-four-pillars/) for detail.

## Where the code lives

- [`crates/sca-core`](../../../crates/sca-core/) — file format, retrieval, pillars, audit, migrate, plugin trait
- [`crates/said-cli`](../../../crates/said-cli/) — terminal UI (`said ask`, `said ingest`, `said admin ...`)
- [`crates/said-mcp`](../../../crates/said-mcp/) — MCP stdio server (25 tools)
- [`SAID-LAM-private/said-lam-static`](../../../SAID-LAM-private/said-lam-static/) — the 4.8 MB Model2Vec static encoder (PCA-reduced nomic-embed-text-v1.5 → 64 dim)

## Next sections

- [File format blueprint](../02-file-format/) — header, sections, frames, blocks, version history
- [Core subsystems](../03-core-subsystems/) — SCA, encoder, brain, FrameStore, retrieval, trigram, symbol, audit
