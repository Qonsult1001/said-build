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

## STEP 2 REFRAME (owner) — the core is WORK-STATE CONTINUITY, not just fact-import

This doc originally framed transcripts only as a SAFETY/disposal problem (the 31 MB `.jsonl`). That missed
the actual reason Claude/Cursor/Kimi carry conversations forward: **so the model REMEMBERS WHAT IT WAS
WORKING ON mid-task** — "you were building X, you did Y, the next step is Z, blocker is W." That work-state
continuity is the day-to-day value to BEAT them at, and `.said` should OWN it portably.

What exists vs the gap (audited):
- HAVE: `.said` already carries work-state across its OWN sessions — on `SessionStart`, `steering::decide`
  injects the most-recent `kind:journal` frame ("where you left off"), proven in `test_session_resume.rs`
  (see doc 16 "Session resume"). The mechanism is real.
- GAP 1 (framing): this doc treated that capability as transcript-disposal, not as the headline feature.
  Fixed here.
- GAP 2 (import their live state): `.said` resumes from ITS OWN journals, but nothing IMPORTS Claude's/
  Cursor's in-progress work-state (their plans/file-history/recent session) so a switch mid-task carries
  over. The Claude/Cursor adapter (still DESIGNED-ONLY — no code) must capture this, not only distilled facts.
- GAP 3 (cross-TOOL + proof): the win is **portable** continuity — resume the same work-state in Claude OR
  Cursor OR Kimi from one `.said` file. Their native carry-forward is per-tool and dies when you switch
  tools/machines. Not yet built, not measured.

So STEP 2 = **portable work-state continuity**: (a) capture richer mid-task work-state into `.said`
(current task / files touched / decisions / next step / blockers — beyond the SessionEnd journal backstop);
(b) IMPORT Claude/Cursor's in-progress state via the adapter; (c) resume it on SessionStart in ANY host
tool; (d) PROVE it beats per-tool carry (survives tool switch + machine move + Claude deletion). The
fact-import connector (below) is the bulk/learning half; work-state continuity is the half that wins daily.

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

## STRATEGY (locked) — `.said` OWNS the knowledge; mirror Claude, clean per-project, carry learnings forward

The product thesis (owner): the moat is **`.said` as a portable brain that OWNS distilled knowledge**, not
an index over someone else's files. "If I only point to existing data, anyone can build an index and the
value lives in their files." So:

1. **Mirror everything Claude saves on a project INTO `.said`** (owned copy, not a pointer) — `.said`
   becomes a complete, portable copy of that project's memory.
2. **Per-project, with a clean-when-done option** — finish a project → wipe its raw/episodic memory…
3. **…but the LEARNINGS carry forward** — distilled fixes/invariants/decisions (the 80%-replicated reuse)
   survive the cleanup and seed the next project.
4. **The moat = owned memory done undeniably best** (`learn_fix` + journal + cross-project + lifecycle
   compounding) — where `.said` beats Claude/Kimi start to finish.

**Prerequisite the owner named:** we must first understand EXACTLY what Claude saves, when, and how
(especially when a new project starts) before we can mirror it. **Pointer-first is the LEARNING vehicle**
(observe Claude's writes) → graduate to OWNING/duplicating them. Map below.

## What Claude saves per project (mapped from disk — the mirror target)

Claude's per-project memory lives under `~/.claude/projects/<slug>/` where `<slug>` is the project's
absolute path with `/` `:` → `-` (so a NEW project = a new slug dir the moment Claude touches it). FIVE
distinct stores:

| # | Store | Path | Size (this repo) | HOW Claude uses it on RESUME (measured) | `.said` verdict |
|---|---|---|---|---|---|
| 1 | **Distilled facts** | `<slug>/memory/MEMORY.md` + per-fact `*.md` | 0.09 MB | **THE core continuity mechanism.** Index always loaded; a Sonnet sideQuery picks ~5 relevant facts; model ADAPTS them (never pastes). Encodes the WHY/invariant. | **OWN — richer.** Map to `remember`/`learn_fix`, `project:<slug>` tag |
| 2 | **Raw transcripts** | `<slug>/*.jsonl` | **147 MB** | **NOT re-read on resume — it's an AUDIT LOG.** Claude reconstructs from facts + file-snapshots + the last conversation boundary, then COMPACTS to a summary. | **DISPOSABLE** (owner was right). Distil ONE episode if anything; never store/replay raw |
| 3 | **Plans** | `~/.claude/plans/*.md` | 0.02 MB | Survive sessions and are readable — BUT plan STATUS is NOT auto-recovered (you re-check where you are). | **OWN + close the gap** — capture plan + WHERE-IN-IT (status), which Claude loses |
| 4 | **File-history** | `<slug>/file-history/<sess>/<hash>@vN` | 24 MB/session | Latest version per edited file = how Claude answers "what was I editing?" on resume (snapshots, not diffs). | **OWN the file-SET + last state** (not 25 versions × 500KB) — the "what was I touching" signal |
| 5 | **Global prefs/rules** | `~/.claude/CLAUDE.md`, `settings.json`, `.claude.json` | 0.03 MB | CLAUDE.md re-injected as `<system-reminder>` EVERY turn; settings cached (model/effort/tool budgets). | **OWN — non-negotiable** (the user's global rules; portable across tools) |

THE KILLER FINDING (measured): Claude's resume = **distilled facts + file-snapshots + last-boundary
summary** — NOT the 147 MB transcript. So `.said` needs NONE of the raw bulk. And Claude's continuity is
**per-tool, per-machine, and loses plan-status on resume** — exactly the seams `.said` wins: own the
ESSENTIAL pieces (facts + work-state + file-set + plan-status + rules) in ONE portable file that survives a
tool switch, a machine move, and deleting Claude. Evidence: real files on this machine, Claude Code
v2.1.193 (`memory/MEMORY.md` 5.48 KB; largest `.jsonl` 38.91 MB / 18,873 lines, audit-only;
`plans/*.md` survive but status doesn't; `file-history/<sess>/<hash>@v24` = 506 KB snapshots).

Each frontmatter fact already carries `name`/`description`/`metadata.type`/`originSessionId` — a clean map
to `.said` `remember`/`learn_fix` (tier-1 facts) + Episodic (tier-2 distilled session). The slug → our
`project:<name>` tag (point 1), so mirror + per-project clean + carry-forward all work via existing tags.

## Cursor + Kimi: they keep almost NOTHING locally — the strategy flips (measured)

Mapped Cursor + Kimi on this machine (and public format for Kimi, which isn't installed here):

| Tool | Local memory? | What's local | Evidence |
|---|---|---|---|
| **Claude** | YES, rich | facts (`memory/*.md`), plans, file-history snapshots, CLAUDE.md | `~/.claude/` (mapped above) |
| **Cursor** | NO real memory | only IDE state (open tabs, cursor pos) in `state.vscdb` SQLite; sparse checkpoint diffs; `.cursorrules` lives in the PROJECT. **Conversations are SERVER-SIDE, not local.** | `AppData/Roaming/Cursor/User/{globalStorage,workspaceStorage}/` |
| **Kimi** | NO (not installed; chat client) | server-side conversations; at most a JSON/SQLite config. No project/work-state. | not present on machine |

**Strategic consequence — the "beat them" case is STRONGER than "import their memory":**
- Only **Claude** has a real local memory worth importing (build the Claude adapter — facts + plans +
  file-set).
- **Cursor + Kimi have NO portable local memory** — their context dies on logout / tool switch / machine
  move. So for them `.said` is not competing with a local store; it **PROVIDES the memory they lack**, via
  the steering hook (`recall` + `SessionStart` resume) — there is little to import FROM them.
- Therefore step 2's emphasis is right where the owner put it: **`.said` as the portable work-state layer
  all three plug into** — IMPORT Claude's local memory where it exists; PROVIDE owned portable work-state
  to Cursor/Kimi where it doesn't. This is the undeniable moat: one file, survives tool switch + machine
  move + logout, which none of the three offer.

## DECISION (revised by strategy) — OWN by default; pointers are the LEARNING step, not the product

Given "`.said` owns the knowledge", the earlier pointer-first lean is **demoted to a means, not the end**:
- **OWN (default):** mirror Claude's tier-1 facts + a distilled tier-2 session episode INTO `.said`
  (self-contained, portable, survives deleting Claude). This is the asset companies buy.
- **POINTER (learning/bulk only):** use `remember_as_external_pointer` to (a) cheaply OBSERVE what Claude
  writes while we learn its format, and (b) reference bulk we deliberately don't copy. Not the moat.
- The original pointer+sync analysis below is retained as the freshness/observation mechanism — we reuse
  its `sync`/incremental/tombstone machinery to keep the OWNED mirror current.

### (retained) pointer/breadcrumb + incremental sync — now the freshness/observation layer

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
