# What a coding-agent BRAIN must do — research + `.said` gap analysis

Research basis: 15 memory systems across arXiv + production (Mem0 2504.19413, Zep/Graphiti 2501.13956,
MemGPT/Letta 2310.08560, MemoryBank 2305.10250, Reflexion 2303.11366, Generative Agents 2304.03442,
ExpeL 2308.10144, A-MEM 2502.12110, Voyager 2305.16291, AWM 2409.07429, ReasoningBank 2509.25140,
Self-Refine/Self-Debug/RepoCoder/CodeRAG-Bench, plus Cursor / Claude Code auto-memory / claude-mem).

## The point this benchmark missed

The first benchmark (doc 18) tested `.said` as a **static index built once by `init`** — read-only. But a
brain *writes as it works*: it stores what it did, records successes/failures, reinforces on recall, and
accumulates across sessions. Claude Code (auto-memory) and Cursor both write continuously. We tested the
read half and ignored the half that makes `.said` a **brain** rather than a search index. This doc maps
the field's required capabilities, then audits which `.said` already has.

## Capability checklist (each item: the system that PROVES it matters)

### (a) Write-as-you-work
- Auto-capture during execution, not only at session end — *claude-mem (PostToolUse), Generative Agents, Mem0*.
- Distill salient facts, don't dump raw turns — *Mem0, ReasoningBank ("store the invariant, not the paste")*.
- Async/background write so it doesn't block the agent — *Mem0*.
- Capture code-specific facts (repo conventions, build commands, architecture) — *Claude Code auto-memory, claude-mem, CLAUDE.md*.
- Cheap always-on index + on-demand detail paging — *Claude Code (MEMORY.md→topics), claude-mem (3-layer), MemGPT (core vs archival)*.

### (b) Success / failure outcome recording
- Record FAILURES as first-class lessons (why it failed), on failure — *Reflexion (→91% HumanEval), ReasoningBank (failures→pitfalls)*.
- Record SUCCESSES as replayable artifacts, only after VERIFICATION (tests pass) — *Voyager (store only verified skills), ExpeL*.
- Tie memories to test/CI outcomes; use execution results, not self-opinion — *Self-Debug, Reflexion*.
- Store error-signature → fix mappings retrievable by the error — *Self-Debug, RepoCoder*.

### (c) Recall reinforcement
- Rank recall by importance + recency + relevance, not relevance alone — *Generative Agents (score = recency+importance+relevance)*.
- **Update salience ON retrieval — recall strengthens a memory + refreshes recency** — *MemoryBank (R=e^(−t/S), recall→S+1,t←0), Generative Agents. RARE: Mem0/Zep/MemGPT do NOT do this.*
- Confidence/voting that survives recall (upvote/downvote useful insights) — *ExpeL (prune at 0)*.

### (d) Consolidation / reflection / forgetting
- Periodic reflection synthesizing higher-level lessons — *Generative Agents (reflection trees), ExpeL*.
- Abstract concrete trajectories into general parameterized routines — *AWM, ReasoningBank*.
- Conflict resolution / fact invalidation on contradiction — *Mem0 (ADD/UPDATE/DELETE/NOOP), Zep (bi-temporal), A-MEM (evolution)*.
- Bi-temporal tracking (valid-time vs ingest-time), expire-don't-delete — *Zep/Graphiti*.
- Forgetting/decay of unused memories to bound size + avoid pollution — *MemoryBank, ExpeL*.
- Dedupe before write (NOOP if equivalent) — *Mem0 (top-10 similar), A-MEM*.
- Link related memories for multi-hop recall — *A-MEM, Zep, claude-mem*.

### (e) Cross-session accumulation
- Persist across sessions + auto-inject prior context at next start — *claude-mem (SessionStart), Claude Code, MemGPT*.
- Later tasks get cheaper AND better — measure it — *Voyager (15.3× faster), AWM (+51% WebArena), ReasoningBank (1.4–1.6× fewer steps)*.
- Generalize/transfer to unseen tasks — *AWM (gains widen on novel tasks), ExpeL (HotpotQA→FEVER), Voyager*.
- Keep the store coherent (not append-only) as it grows — *Mem0, Zep, A-MEM*.

### Field's three unsolved gaps (the moat)
1. **Recall reinforcement is rare** — only MemoryBank + Generative Agents update salience on retrieval.
2. **Consolidation/dedupe is shallow** industry-wide — even claude-mem barely dedupes.
3. **Retrieval quality, not storage, is the bottleneck** (CodeRAG-Bench) — validates `.said`'s own semantic+intent fingerprints.

## `.said` gap analysis — it ALREADY has the machinery; the benchmark never used it

| Capability | `.said` mechanism (verified in code) | Exercised by benchmark 18? |
|---|---|---|
| Write-as-you-work | MCP `remember` (persists + indexes), `journal`, `ingest` | ✗ never called |
| Distill salient facts | `remember_with_salience` (pillar + salience scoring) | ✗ |
| Success record (verified) | `learn_fix` (Procedural pillar, only verified fixes) | ✗ (empty fix-store — the T06 miss) |
| Failure / outcome | `tool_completion`, `session_end` | ✗ |
| **Recall reinforcement** | `brain.reconsolidate` via `log_query` on every `ask` — bumps `recall_weight` (1.0→2.0, diminishing + burst bonus, recency-decayed) | ✗ (single cold reads, no accumulation) |
| Importance+recency+relevance rank | float rerank × `recall_weight` × `s_slow_boost` (ask.rs ~734) | partial (no reinforcement history) |
| Consolidation/reflection | `dream`, `brain.consolidate`, latent clustering | ✗ |
| Dedupe before write | `latent_cluster.dedup_check` | ✗ |
| Forgetting/decay | `recall_weight *= decay_rate`, `tombstone` | ✗ |
| Link for multi-hop | `[[wikilinks]]` → `link:` tags, `build_concept_links` | partial (read only) |
| Cross-session accumulation | single portable file persists; `session_end` summary | ✗ never accumulated |

**Conclusion.** `.said` implements nearly the entire research checklist — including the RARE recall
reinforcement (only 2 of 15 surveyed systems have it). The benchmark in doc 18 tested only the cold
read-path. The real test (doc 20) must let `.said` **write as it works across a multi-task session** and
measure whether accumulation makes later tasks cheaper/better — the Voyager/AWM/ReasoningBank effect.
