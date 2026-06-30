# 31 — The memory-evidence standard (claim → evidence → source, recalled by manifest + OKF links)

The `.said`-native memory format that mirrors how Claude Code, Kimi, and Gemini actually structure and
recall memory. Grounded in their real source (read from `G:\Coding\claude-code-main`), Gemini's published
export prompt, and the retrieval-theory literature. This is the SCHEMA standard; the import *strategy* is
[29](29-learning-from-claude-kimi-memory.md), the compounding *benchmark* is
[30](30-beat-them-benchmark.md).

## The core realization — world-class memory does NOT solve recall@1; it sidesteps it

Recall@1 on a single fixed-dimension embedding has a **mathematical ceiling** — proven in
*Theoretical Limitations of Embedding-Based Retrieval* (arXiv:2508.21038): a d-dim vector can perfectly
retrieve only a bounded subset of query→doc relationships, and it worsens as the relevant set grows
relative to `d`. `.said`'s encoder is d=64 (small + portable by design, [3.2](03-core-subsystems/3.2-static-encoder.md)),
so chasing recall@1 = 100% on one vector is chasing a provably-unreachable target.

So the leading systems don't. Verified from real source:

| System | How it recalls (from source) |
|---|---|
| **Claude Code** (`src/memdir/findRelevantMemories.ts`) | NO embedding, NO vector search, NO recall@1. It scans every memory's `name` + `description` into a **manifest**, hands it to the model, and asks *"select up to 5 relevant"*. The model selects from descriptions; evidence links lead to source. |
| **Claude schema** (`src/memdir/memoryTypes.ts`) | A memory is *"a **claim** that [a function/file/flag] existed when the memory was written"* — so each memory carries its evidence and is verified against current state before use. |
| **Gemini** (export prompt) | Every entry = a **claim** + `Evidence: '<verbatim quote>'` + `Date: [YYYY-MM-DD]`, closed by `Imported from: <assistant>`. |

The universal pattern is **claim → evidence → source**, recalled by **manifest + LLM-select**, not by
nearest-vector. That is why recall@1 never gated them — and it is the pattern `.said` adopts.

## Why this fits `.said` perfectly (the three pieces already exist)

1. **Manifest** — every `.said` memory frame already has a `name` + `description` (title + the
   relevance one-liner). `list-concepts` / a manifest scan over memory frames = Claude's `scanMemoryFiles`.
2. **LLM-select** — `ask` already returns top-k candidates and the host LLM decides
   ([3.5](03-core-subsystems/3.5-retrieval-pipeline.md)). We hand the manifest/candidates to the model;
   it picks. No engine recall@1 needed.
3. **Evidence-link traversal** — the OKF concept graph (`build_concept_links`, default-on at init,
   [14.7](14-novel-mechanisms/14.7-layer6-graph.md)) is the wiki-tree. Each memory's `link:` edges point
   to its **evidence** (a commit, a source frame, a dated quote). OKF traversal
   (`frames_linking_concept`, the Engine-D bridge) then **reaches the evidence from any entry point** —
   the reachability property ([OKF is reachability, not precision](#)). Recall@1 becomes irrelevant: you
   don't need the one exact vector hit when the graph guarantees you reach the evidence tree.

## The standard — a `.said` memory frame

Every memory frame carries, mirroring Claude's 4-type taxonomy (which `.said` already uses):

```
---
name:        <short-kebab-slug>            # the manifest key
description: <one-line, specific>          # what the LLM matches on to SELECT
type:        user | feedback | project | reference
date:        YYYY-MM-DD                     # absolute (convert relative dates at save)
---

<CLAIM — lead with the fact/decision, in the agent's own words>

**Why:**  <the evidence/reason — often the incident, constraint, or measurement>
**How to apply:** <when/where this kicks in>

Evidence: "<verbatim quote or measured result>"     # the source-of-truth snippet
```

Plus **`link:` evidence edges** on the frame (the wiki-tree, a few bytes each, hash-safe envelope tags):

| Edge | Points to | Example |
|---|---|---|
| `link:commit-<hash>` | the git commit that is the evidence | `link:commit-edd8a3c` |
| `link:<source-frame-id>` | the code/doc frame the claim is about | `link:ask.rs` |
| `link:<concept>` | the shared entity (OKF auto-links these) | `link:recall` |

### The 4 types (from Claude `memoryTypes.ts`, already mirrored in `.said` MEMORY.md)

- **user** — who the user is (role, expertise, preferences).
- **feedback** — guidance on HOW to work; body = rule + **Why:** + **How to apply:**.
- **project** — ongoing work/goals/constraints not derivable from code or git; absolute dates.
- **reference** — pointers to external resources (URLs, dashboards, tickets).

### What NOT to save (from Claude `memoryTypes.ts` WHAT_NOT_TO_SAVE)

Code patterns, architecture, file structure, git history, fix recipes already in the commit, anything in
CLAUDE.md, ephemeral task state. These are **derivable** — the evidence link points at them instead of
duplicating them. *"If asked to save a PR list, ask what was surprising/non-obvious — that is the part
worth keeping."*

### The drift rule (from Claude `memoryTypes.ts` TRUSTING_RECALL)

A memory that names a file/function/flag is a **claim it existed when written**. Before recommending from
it, follow the evidence link and verify against current state (the file exists / grep the function / the
commit is reachable). *"The memory says X exists" ≠ "X exists now."* This is exactly why the evidence
LINK matters: it makes verification one hop, not a re-search.

## Recall flow (claim → evidence → source, no recall@1 dependency)

```
query
  → MANIFEST: scan memory frames' name+description (Claude scanMemoryFiles)
  → SELECT:   host LLM picks the relevant memories from the manifest (Claude findRelevantMemories)
  → TRAVERSE: for each selected memory, follow its link: edges (OKF Engine-D bridge)
              → reach the evidence (commit / source frame / dated quote)
  → VERIFY:   check the evidence link against current state (drift rule) before acting
```

The embedding/vector path ([3.5](03-core-subsystems/3.5-retrieval-pipeline.md)) still runs and helps —
but it is the *candidate generator*, not the gate. The **graph reachability + LLM-select** is what makes
recall complete, so the d=64 recall@1 ceiling (arXiv:2508.21038) stops being the limiting factor.

## Why this beats a pure-embedding memory

- **No recall@1 ceiling dependence** — reachability via the evidence graph, not one nearest vector.
- **Verifiable** — every claim links to its source; the drift rule makes stale memory detectable.
- **Portable + cross-tool** — one `.said` file; the manifest+links travel (Claude/Kimi keep theirs in
  per-tool dirs, [29](29-learning-from-claude-kimi-memory.md)).
- **Compounding** — evidence links accrete into a navigable knowledge tree, the
  self-consolidation axis of [30](30-beat-them-benchmark.md).

## Status

DEFINED (this doc). Foundations SHIPPED: OKF concept graph default-on
([14.7](14-novel-mechanisms/14.7-layer6-graph.md)), 4-type taxonomy in MEMORY.md, `ask` top-k + LLM-select,
per-pillar + per-project scope ([28](28-token-value-and-scoping.md),
[row-31](05-features/row-31-per-pillar-retrieval.md)). NEXT: an e2e proof — store real project memories in
this format with evidence links, then show manifest+select reaches the memory and OKF traversal reaches the
git evidence (100% reachability), measured against the recall@1 baseline.

## Sources

- Claude Code memory: `G:\Coding\claude-code-main` — `src/memdir/memoryTypes.ts` (schema, 4 types,
  what-not-to-save, drift rule), `src/memdir/findRelevantMemories.ts` (manifest + LLM-select, no vector).
- Gemini export prompt (owner-provided) — claim + Evidence + Date entry format.
- *Theoretical Limitations of Embedding-Based Retrieval*, arXiv:2508.21038 — the d-dim recall ceiling.
- *Static Word Embeddings for Sentence Semantic Representation*, arXiv:2506.04624 — PCA + All-But-The-Top
  + distillation lifts mean-pool static embeddings (the ABTT half is already in `.said`'s float rerank).
