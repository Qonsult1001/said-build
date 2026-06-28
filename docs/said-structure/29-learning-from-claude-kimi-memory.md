# What `.said` should learn from Claude/Kimi's own memory — and the chat-history safety gap

The owner asked: Claude Code (and Kimi) already persist memory; what can `.said` learn from how they do it,
and there's a safety concern that Claude's chat history "is not safe on my side." Here is what was found on
disk and what it means for `.said`.

## How Claude Code persists memory (observed on this machine)
- **Per-project, path-keyed auto-memory.** `C:/Users/Carter/.claude/projects/<path-slug>/memory/` — the
  slug encodes the project path (`g--development-said-build`). So Claude memory IS auto-scoped per project
  (the thing `.said` does NOT do automatically — see doc 28 §2).
- **An index + one-file-per-fact format.** `MEMORY.md` is a one-line-per-memory index; each fact is its own
  `.md` with YAML frontmatter (`name`, `description`, `metadata.type`, `originSessionId`) + a body with
  **Why / How-to-apply**. This is a clean, human-inspectable, greppable store.
- **Skills carry step/state.** `.claude/skills/<skill>/` holds CONVENTIONS/reference docs and step
  structure — the agent's "where am I in this process" lives in the skill files, not in `.said`.

**The safety concern is real and confirmed:** Claude also writes **full conversation transcripts as
plaintext `.jsonl`** in that same project folder — e.g. a single session here is **31 MB of raw chat**
(`5f03d865….jsonl`). That is every message, tool call, and file content, unencrypted, on local disk,
indefinitely. That is the "not safe on my side" issue: the *transcript* is a large, sensitive, plaintext
artifact you didn't explicitly choose to keep.

## What `.said` should ADOPT from Claude/Kimi
1. **Auto per-project scoping** (Claude's path-keyed dir). `.said` should auto-tag `project:<name>` at
   ingest so memories isolate + delete + federate per project (doc 28 §2/§3). This is the single highest-
   value gap.
2. **The index + frontmatter format is already what `.said`'s learn_fix note approximates** — Title /
   Files / Learnings / Why is the same shape as Claude's frontmatter+Why+How. Keep aligning the
   `ITERATION_TEMPLATE` to it (already done, docs/24-25).
3. **Distil, don't dump** (Claude stores a *distilled* fact per file, NOT the transcript). `.said` already
   enforces this (learn_fix stores the invariant, not the chat) — and it is the answer to the safety gap.

## Where `.said` is SAFER than the chat-history model
- `.said` stores **distilled learnings in a single binary `.said` file**, not 31 MB of plaintext chat.
  The sensitive raw conversation never has to live on disk — only the salient fix/decision does.
- It is **one portable, deletable file**: "forget project said-build" = delete-by-tag (doc 28 §3) or
  delete the file, vs hunting per-project `.jsonl` transcripts.
- **Enterprise mode** refuses content ingest (stores only pointers/summaries) — designed exactly for the
  "don't keep the raw thing on disk" requirement (CLAIMS-COVERAGE: enterprise pointer rows).

## What `.said` should do that neither chat-history nor Claude-auto-memory does well
- **Capture what the agent concludes mid-build into a portable, queryable store** (learn_fix), so it
  survives across tools (Claude today, Kimi tomorrow) — Claude's memory is Claude-only and decays/compacts
  over a long lifecycle (doc 27); `.said` recall is durable and semantic, and BYO-LLM (any model).
- **Optionally ingest Claude/Kimi's own memory files** as external pointers (the `browser_ingest` pattern
  generalizes): index Claude's `MEMORY.md` facts into `.said` so the durable store is the union, and the
  large plaintext transcripts can then be pruned.

## Net recommendation (small, high-value)
1. Auto-tag `project:<name>` at ingest → unlocks isolation + per-project delete + opt-in federation.
   **DONE 2026-06-28** (doc 28 §2; `test_project_scope.rs`).
2. Offer a "import Claude/Kimi memory" connector (their `MEMORY.md`/fact files → `.said` distilled
   pointers) so `.said` becomes the durable, model-agnostic, single-file store — and the 111 MB plaintext
   transcripts become disposable. **Design below.**
3. Keep enforcing distil-not-dump (already the design) — it's both the recall-quality moat AND the privacy
   answer to the chat-history concern.

## The import connector — design (research-grounded, the "ingest then discard the source" model)

The owner's framing: **treat a Claude/Kimi session as a DOCUMENT I import once, then discard the source** —
so the *questions and discussions* (not just the final notes) become durable context in `.said`, and the
raw transcript can be deleted. Research on what to extract (2025 agent-memory state of the art):

- **Mem0** — a two-stage pipeline: an LLM **extracts salient memory candidates** from the conversation,
  then **consolidates** (add/update/delete by semantic similarity) to cut redundancy at the source. ⇒ the
  connector must *distill + dedup*, not dump turns.
- **Episodic vs semantic** (Zep/Letta) — store BOTH: the *episode* (what happened this session: decisions,
  Q&A, file changes) AND the *semantic* fact (the durable invariant). ⇒ map to `.said` pillars: Episodic
  for the session story, Procedural/Semantic for the distilled fix/decision.
- **A-Mem / Zettelkasten** — each memory is a note with links to related notes (evolving graph). ⇒ reuse
  `.said`'s `[[wikilink]]`/concept edges so an imported decision links to the code/fix it concerns.
- **MemoryBank / forgetting** — strength decays unless reinforced. ⇒ imported episodes can decay; the
  distilled facts persist (already `.said`'s decay/reconsolidate model).

**What a Claude session actually contains (verified on disk):** the `.jsonl` has structured records —
`user` (questions), `assistant` (answers/decisions), `attachment`, `file-history-snapshot` (what changed),
`ai-title`, plus the distilled `MEMORY.md` facts. So there are THREE tiers to import, richest-first:
1. **Distilled facts** (`MEMORY.md` + per-fact `.md`) → `remember`/`learn_fix`, tagged `project:<name>`.
   Cheapest, highest-signal, no LLM.
2. **Session episode** (the user↔assistant decisions + file-history) → ONE distilled Episodic note per
   session ("wanted/decided/built/blockers/next" — the same journal shape point 2 resumes), via a BYO-LLM
   extract+consolidate pass (Mem0 two-stage). This is the "questions & discussions become context" the
   owner wants.
3. **Raw transcript** → NOT stored. After 1+2, the `.jsonl` is **disposable** (the safety win).

**Mechanism (reuses what exists, no new core):** generalize `browser_ingest` (external-pointer ingest) into
a `memory_import` that reads a source dir (Claude `.claude/projects/<proj>/`, Kimi equiv), runs the
distill pass (BYO-LLM, outside `.said` — same as the LoCoMo oracle / dream v3 pattern), writes Episodic +
distilled frames tagged `project:<name>` + `source:claude|kimi` + `session:<id>`, then reports which source
files are now safe to delete. Dedup via the existing `dedup_check`. Per-project delete (point 1) and
session-resume (point 2) then work on the imported memories automatically.

**Open questions for the build (decide before coding):** (a) is the distill pass mandatory (needs BYO-LLM)
or optional (facts-only, no LLM)? (b) do we auto-delete the source `.jsonl` or just report "safe to
delete"? (recommend: report, never auto-delete — the user owns that). (c) Kimi's on-disk memory format
needs the same disk-shape check we did for Claude before wiring its reader.

## DECISION — pointer/breadcrumb FIRST, incremental sync for freshness (NOT a full copy)

The owner weighed two models: (A) `.said` stores **breadcrumbs/pointers** to Claude's live files (recall
points to the file, returns only what's needed; nothing Claude does changes) vs (B) **trigger live
updates** that copy/distill Claude's writes into `.said`. Research settles it
([incremental indexing](https://medium.com/@vasanthancomrads/incremental-indexing-strategies-for-large-rag-systems-e3e5a9e2ced7),
[CocoIndex real-time code index](https://cocoindex.io/blogs/index-code-base-for-rag/),
[LEANN live-data pointer model arXiv:2506.08276]):

- **Primary = Design A (pointer).** `.said` stores a small **external pointer** per Claude memory file (a
  distilled one-line "what's in here" + `external:uri=<path>`, `Pillar::External`, tagged `project:`).
  Claude keeps writing to its files **unchanged** — clean integration, nothing existing changes. Recall
  finds the breadcrumb by meaning and returns ONLY the pointed-to slice (max token economy + zero
  staleness — the live file is the source of truth). `.said` already has this:
  `remember_as_external_pointer` (used by `browser_ingest`).
- **Freshness = Design B, the CHEAP half only.** A light watcher / `sync` re-points or re-distills a
  breadcrumb **only when a file changes** (incremental, never a full re-copy) and **tombstones** it if the
  file is deleted. `.said` already has this: incremental `build_index` + the `sync` reconcile/tombstone
  path. Research's three invariants (stable ids, versioning, tombstones) are all already present
  (blake3 ids, incremental index, tombstone deletes).
- **NOT a full copy.** The bulk (111 MB of `.jsonl`) never enters `.said` — only the breadcrumb (and,
  optionally, the point-3 tier-2 distilled session-episode). This satisfies safety + clean-integration +
  token-economy simultaneously.

**Net build = thin layer over existing primitives:** a `memory_import --watch`/`memory_link` that
(1) writes one external pointer per Claude/Kimi memory file (project-tagged), (2) on change, incrementally
re-points (the `sync` path), (3) at query time recall returns the breadcrumb → the agent opens the live
file for the slice it needs. No core mechanism is new; it's wiring `remember_as_external_pointer` +
`sync` + point-1 tagging + point-2 injection together.
