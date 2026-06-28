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
- **The scope machinery EXISTS but isn't auto-populated.** `MemoryScope {Personal, Project, Organization,
  Public}` (`frames.rs:88-102`) is stored per frame, but new memories default to `Personal` and the
  CLI/MCP don't expose setting it. So memories aren't *blocked* across projects — they're just not tagged.
- **Cross-project reuse IS built — federation.** `said-orchestration::recall::best_iterations_federated`
  (`recall.rs:62-103`) queries a PRIMARY brain + mounted read-only **skill packs**, merges, dedups by
  BLAKE3 doc_id, ranks, returns top-k. Skill-pack discovery (`docs/18`): `--skills`, `$SAID_SKILLS_DIR`,
  `<repo>/.said/skills/*.said`, `~/.said/skills/*.said`. **Writes go only to the primary; packs stay
  read-only** (like Docker base layers).

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
  (`handler.rs:2317-2395`). So `delete {tag_filter:"project:said-build"}` works **once memories carry that
  tag**.
- **Delete by MemoryScope:** ❌ helper `doc_ids_by_scope()` exists (`frames.rs:1400`) but isn't wired to a
  command.
- **Bundled UI:** there's an admin surface (`row-43-admin-ui`), but a one-click "delete everything for
  project X" needs (a) project auto-tagging at ingest + (b) the scope-delete wired up. Small, well-scoped.

**Net:** the cleanest path to "isolate + reuse + delete per project" = **auto-tag every memory with
`project:<name>` at ingest** (one change), then recall pre-filters by it (already works via
`detect_scope_tag`), federation reuses across chosen projects (already works), and delete-by-tag removes a
project (already works). One ingest-tagging change unlocks all three.

## 4. rusqlite / bundled SQLite — size cost

`browser` feature → `rusqlite` with `features=["bundled"]` (`sca-core/Cargo.toml:213-216`) compiles SQLite
from C source and statically links it (no system `libsqlite3` dependency). Off by default, native-only.
Binary-size delta measured (said-mcp coding bundle, with vs without `browser`): **see
`hard-eval/sqlite_size.txt`** — the bundled SQLite adds roughly ~1–1.5 MB to the ~53 MB binary (small;
the encoder models dominate at ~6 MB). The trade you described is right: bigger binary, zero runtime
dependency.
