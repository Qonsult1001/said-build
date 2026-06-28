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
2. Offer a "import Claude/Kimi memory" connector (their `MEMORY.md`/fact files → `.said` distilled
   pointers) so `.said` becomes the durable, model-agnostic, single-file store — and the 31 MB plaintext
   transcripts become disposable.
3. Keep enforcing distil-not-dump (already the design) — it's both the recall-quality moat AND the privacy
   answer to the chat-history concern.
