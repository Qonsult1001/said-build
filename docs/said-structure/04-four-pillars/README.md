# Four-pillar memory

`.said` classifies every frame into one of six pillars, mirroring the Complementary Learning Systems (CLS) model from cognitive neuroscience. The pillars are independent axes — a frame has exactly one pillar, and retrieval can be scoped to any subset.

## Pillar list

| Pillar | Purpose | Typical source |
|---|---|---|
| [Episodic](episodic.md) | Raw turns, session events, append-only | `/remember`, session_end, tool_completion, dialogue ingests |
| [Semantic](semantic.md) | Distilled facts (usually caller-written at read time) | User-authored summaries, caller's LLM consolidation |
| [Procedural](procedural.md) | Action recipes with outcomes | `remember_as_procedural(trigger, steps, outcome)` |
| [External](external.md) | Pointers or embedded references to documents/URLs | `remember_as_external_pointer` / future `_embedded` |
| [Code](code.md) | AST-chunked source | `remember_as_code`, `said init` on a repo |
| [Memory](memory.md) | Legacy catch-all | Default pillar for pre-Decision-1 files |

## Decision history

- **Decision 1 (2026-04-21)** — Pillar enum shipped on every FrameMeta, forward-compatible on-disk layout
- **Decision 2 (2026-04-21)** — Per-pillar retrieval scope (`search pillar=`)
- **Decision 3 (2026-04-21)** — Explicit Episodic writer + session_end + tool_completion hooks
- **Decision 4 v0 (2026-04-21)** — Salience heuristic scorer
- **Decision 5 v1 (2026-04-21)** — Dream content-consolidation shipped
- **Decision 5 v2 (2026-04-22)** — Dream content-consolidation gutted; brain-state dream remains

See [`SAID_MVP_PLAN.md`](../../SAID_MVP_PLAN.md) rows 30-35 for the full narrative.

## Retrieval ranking hint per pillar (per architecture spec)

From `docs/said-memory-architecture.md` (originally designed; `rerank_by_pillar` implements the Episodic recency decay today):

| Pillar | Ranking formula | Why |
|---|---|---|
| Episodic | `SCA × exp(-age_hours / τ)` | Generative Agents recency weighting |
| Semantic | `SCA × confidence × (1 - decay)` | Confidence-gated relevance |
| Procedural | `task_match × success_rate` | Voyager-style skill retrieval |
| External | `metadata_filter + schema_query` | Pointer — don't rank by body content |
| Code | standard SCA + sym + grep | Unchanged — already good |
| Memory | standard | Legacy path |

Current implementation ships Episodic recency decay (`rerank_by_pillar` in `recall.rs`); the other per-pillar rankings are still on the planned `rerank_by_pillar` expansion.

## Where pillar is stored

- **On disk** — 1 byte per frame in FTOC. See [2.3 Frame layout](../02-file-format/2.3-frame-layout.md).
- **As tag** — also written as `pillar:<name>` for tag-scoped retrieval / admin filters.

Both paths are kept in sync by `remember_with_pillar` which calls `FrameStore::set_pillar` after the TOC write. Without that call, Factual-mapped pillars collapse to Semantic — [see Known Limitations](../11-known-limitations.md).
