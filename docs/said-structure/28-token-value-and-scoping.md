# Token value, project scoping, and memory management — answers with evidence

Direct answers to the owner's questions, measured from the code/CLI, not asserted.

## 1. How many tokens does `.said` actually save vs a normal coding agent?

**The mechanism (the owner's framing, correct):** a normal agent *greps a symptom, then reads the
candidate files into its context window* to locate/fix a bug — every file it opens is dumped into context.
`.said` instead returns **only the small relevant slice** (the matching function + a short why, or the
recalled fix note). The saving is the difference between "the whole files in context" and "just the slice."

**Measured, three honest scenarios** (tokens ≈ chars/4; input pricing Sonnet ~$3/M, Opus ~$15/M):

| Scenario | normal agent (into context) | `.said` (slice only) | fewer | $ saved (Sonnet / Opus) |
|---|---|---|---|---|
| Small locate (reads ~3 candidate files) | ~1,715 tok | ~106 tok | **16×** | $0.005 / $0.024 |
| Documented bug-location e2e (`test_bug_location_e2e`) | ~8,587 tok (34,346 chars) | ~86 tok (345 chars) | **~100×** | $0.026 / $0.13 |
| Broad symptom sweep (greps + reads 8 files) | ~125,000 tok | ~134 tok | **~930×** | $0.38 / $1.88 |

**The honest read:**
- Per task the **$ is small** (cents) — but it **recurs on every locate/recall**, hundreds of times over a
  project. The saving **compounds across the lifecycle**.
- The bigger value is **context-window economy**, not just dollars: a normal agent burns its limited window
  on file dumps; `.said` keeps the window for *reasoning*. On large repos the normal path can blow the
  window entirely (the 125K-token sweep) — `.said` answers it in ~130 tokens.
- Defensible headline: **`.said` typically returns the answer in 1–2 orders of magnitude fewer context
  tokens than grep-and-read** (16× small, ~100× the documented e2e, up to ~900× on a broad sweep). Cite the
  ~100× e2e as the conservative, reproducible number (`test_bug_location_e2e`); the 900× is the worst-case
  a broad symptom sweep would otherwise cost.

Reproduce: `crates/sca-core/tests/test_bug_location_e2e.rs` (the 345 vs 34,346 measure);
`hard-eval/token_value.psv` (the three-scenario table).

## 2. Are `said-build`'s memories scoped to said-build? Can other projects reuse them?

**Today (code-cited):**
- **One brain per user by default, NOT auto per-project.** Brain resolution
  (`said-cli/src/resolve.rs:12-44`): explicit `--path` → a single `*.said` in the cwd → a global default
  config. So if each project keeps its own `project.said`, it IS isolated; there is no automatic
  `project:said-build` tagging.
- **Coding fixes ARE now project-tagged (built 2026-06-28).** `learn_coding_fix` writes a `project:<name>`
  tag from `SAID_PROJECT` and folds the project into the fix identity; recall hard-filters on
  `SAID_RECALL_PROJECT`. (The separate `MemoryScope {Personal,Project,Organization,Public}` enum in
  `frames.rs:88-102` still defaults to `Personal` for generic `remember`s — a coarser axis than the
  project tag, left as-is.)
- **Cross-project reuse IS built — federation.** `said-orchestration::recall::best_iterations_federated`
  (`recall.rs:62-103`) queries a PRIMARY brain + mounted read-only **skill packs**, merges, dedups by
  BLAKE3 doc_id, ranks, returns top-k. Skill-pack discovery (`docs/18`): `--skills`, `$SAID_SKILLS_DIR`,
  `<repo>/.said/skills/*.said`, `~/.said/skills/*.said`. **Writes go only to the primary; packs stay
  read-only** (like Docker base layers).

**UPDATE (built 2026-06-28): per-project scoping is now first-class.** `learn_coding_fix` reads
`SAID_PROJECT` and (a) makes the project part of the fix's IDENTITY (so two projects can each hold their
own fix for the SAME task shape — they no longer overwrite each other) and (b) writes a `project:<name>`
tag. Recall hard-filters on `SAID_RECALL_PROJECT` (mirrors `SAID_RECALL_LANG`): set it → only that
project's fixes (plus project-agnostic ones) come back; unset → cross-project reuse stays possible.
Project-tagged delete (`delete {tag_filter:"project:said-build"}`) then removes exactly one project.
Proven: `crates/sca-core/tests/test_project_scope.rs` (isolation when scoped, both visible when open).
This closes the "set scope at ingest" + "delete by project" gaps below. The remaining wiring is to have
the CLI/MCP/orchestrator auto-set `SAID_PROJECT` from the repo/cwd name (so it's automatic, not manual).

**So your "reuse functions from SAID-ECHO/said-cgp-cs" scenario works today like this:**
```
# project said-cgp-cs reuses said-build's verified learnings
cp ../said-build/said-build.said ./.said/skills/said-build.said
orchestrate --brain cgp.said --skills .said/skills/said-build.said ask "<task>"
# -> federated recall over both; the 80%-replicated fix surfaces from the said-build pack,
#    cgp.said (primary) wins ties, and only cgp.said is written
```
This is exactly the "80% replicated, recall it instead of recreating" saving — **proven path, live in the
orchestrator.** Gap: the CLI `ask`/MCP don't yet expose `--skills` (federation is orchestrator-only); and
nothing auto-tags `project:` for one-brain-many-projects. Both are small wiring tasks, not new mechanisms.

## 3. "Delete all memories related to said-build" — and a bundled UI

- **Delete by doc_id:** ✅ `said delete <doc_id>` (`main.rs:2931`), MCP `delete {doc_id}`.
- **Batch delete by tag + time:** ✅ MCP `delete {older_than_days, before_date, tag_filter, dry_run}`
  (`handler.rs:2317-2395`). And coding fixes now carry `project:<name>`, so
  `delete {tag_filter:"project:said-build"}` removes exactly one project's fixes — **live.**
- **Bundled UI:** there's an admin surface (`row-43-admin-ui`); a one-click "delete everything for
  project X" is now just that delete-by-tag wired to a button (the underlying tag + delete both exist).

**Net (now mostly built):** isolate + reuse + delete per project works: `learn_coding_fix` auto-tags
`project:` from `SAID_PROJECT`, recall isolates via `SAID_RECALL_PROJECT` (or stays open for reuse), and
delete-by-tag removes a project. Remaining wiring: auto-set `SAID_PROJECT` from the repo/cwd in the
CLI/MCP/orchestrator entry points, and expose `--skills` federation on the CLI/MCP (orchestrator-only
today). Both small.

## 4. rusqlite / bundled SQLite — size cost

`browser` feature → `rusqlite` with `features=["bundled"]` (`sca-core/Cargo.toml:213-216`) compiles SQLite
from C source and statically links it (no system `libsqlite3` dependency). Off by default, native-only.
Binary-size cost measured: the bundled SQLite static lib (`libsqlite3.a`) compiled from source is
**~4.52 MB** (the actual artifact; link-time dead-code stripping makes the final binary delta somewhat
smaller). Context: the encoder models are ~6 MB, the coding binary ~53 MB — so bundled SQLite is a modest
add, and `browser` is **off by default + native-only**, so a normal `said-coding`/`said-mcp` build pays
**zero** for it. The trade you described is exactly right: a few MB bigger binary in exchange for **zero
runtime dependency** (no system `libsqlite3` needed on the user's machine). Note: `browser` is currently
a `sca-core`-only feature with **no passthrough on said-cli/said-mcp** — to actually ship browser-ingest
in those binaries, a `browser = ["sca-core/browser"]` passthrough must be added (small wiring gap found
during this measurement).
