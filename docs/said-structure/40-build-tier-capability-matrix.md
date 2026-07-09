# Build-tier capability matrix — what each shipped bundle can and cannot do

The release pipeline ships **four** binary bundles, each a different feature set compiled with
`--no-default-features`. A command/tool that a bundle doesn't compile in is **absent from that bundle**
— not hidden, not disabled: it does not exist in the binary and (for MCP) does not appear in
`tools/list`. This page is the authoritative "who ships what" reference so nobody documents a command a
user's bundle doesn't have.

- **Source of truth for the feature composition:** [`crates/said-mcp/Cargo.toml`](../../crates/said-mcp/Cargo.toml)
  + [`crates/said-cli/Cargo.toml`](../../crates/said-cli/Cargo.toml) `[features]`.
- **Source of truth for the tool surface:** the `tool_box!` macros in
  [`crates/said-mcp/src/tools.rs`](../../crates/said-mcp/src/tools.rs) and the `#[cfg(feature = …)]`
  gates on each handler.
- **What each feature flag pulls in (deps, binary size):** [`09-cargo-features.md`](09-cargo-features.md).
  This page is the *capability* view; that one is the *build* view.

## The four bundles

| Bundle | Feature composition | For whom |
|--------|---------------------|----------|
| **brain** | `embed-model` | The free personal-memory tier: notes, facts, decisions, recall by meaning. No code intelligence. |
| **coding** | `embed-model` + `code` | Brain **plus** source-code intelligence (AST indexing, symbol search, code fixes). |
| **coding-plus** | `embed-model` + `code` + `lsp` | coding **plus** a language-server client (cross-file defs/refs/hover). |
| **full** | `embed-model` + `code` + `docs` + `ocr` + `lsp` | coding-plus **plus** document ingestion (PDF/DOCX) and OCR of scanned pages. |

Each higher bundle is a **superset** of the one before it. `embed-model` (the baked-in encoder) is in
every bundle, so semantic recall works offline everywhere.

## What each feature turns on

| Feature | Adds |
|---------|------|
| `embed-model` | The 3.9 MB static encoder, baked in. Every memory + recall path. **In all bundles.** |
| `code` | tree-sitter AST chunking (Rust/Python/JS/TS/Go/Java/C#) + the code/SQL tool surface (see below). |
| `lsp` | Language-server client tools (`lsp_def`, `lsp_refs`, `lsp_hover`, `lsp_symbols`). |
| `docs` | PDF + DOCX text extraction during ingest. |
| `ocr` | OCR of scanned-image PDFs (requires `docs`). |

## Tool / command surface by bundle

### The memory core — in EVERY bundle (brain and up)

These are the personal-memory verbs. The **brain** bundle is exactly this set; every richer bundle keeps
all of them.

| MCP tool | CLI command | What it does |
|----------|-------------|--------------|
| `ask` | `ask` | Find memories by meaning (the main command). |
| `remember` | `add` (alias `remember`) | Save a note/fact/decision. |
| `get` | `get` | Read one memory's exact text by id. |
| `delete` | `delete` | Remove a memory (recoverable). |
| `history` | `history` | A memory's past versions. |
| `checkout` | `checkout` | Roll a memory back to an earlier version. |
| `status` | `stats` | How many memories the brain holds + the semantic-index state. |
| `list_concepts` | `list-concepts` | The `[[wikilink]]` concept vocabulary, per-concept counts. |
| `list_tags` | `list-tags` | The `tags` metadata vocabulary, per-tag counts. |
| `open` / `create` | `create` / `use` | Make or switch to a brain file. |
| `admin` (basic) | `admin` | Recycle-bin recovery: `list-tombstones`, `restore`, `who-deleted`. |

> **`admin` is split by tier.** The everyday **recovery** actions — `list-tombstones`, `restore`,
> `who-deleted` — are in **every** bundle (a user must always be able to undo a delete). The
> **compliance** actions — `legal-hold-add`, `legal-hold-release`, `retention-sweep`, `audit` — are
> **Enterprise-only** (the `full` bundle), gated behind the `enterprise` cargo feature. On a lighter
> build those action names return a clear "needs the Enterprise build" message, not a silent unknown.
> See [Admin sub-actions](#admin-sub-actions-basic-in-every-bundle-compliance-in-full-only) below.

### Added by `code` — coding, coding-plus, full only (ABSENT from brain)

Everything about indexing and reasoning over source code. **A brain build has none of these**; asking a
brain MCP for them returns a clean "unknown tool", and the brain CLI has no such subcommands.

`search` · `ingest` · `init` · `sync` · `sym` · `edit` · `edit_batch` · `discover` · `overview` ·
`snapshot` · `sandbox` · `clean` · `journal` · `session_end` · `tool_completion` · `salience` ·
`dream` · `recall_fix` · `learn_fix` · `recall_blueprint` · `learn_blueprint` · `harvest_*`

Notably, **bulk folder ingest (`init` / `ingest`) is a code-tier capability** — it is why a *memory*
brain has no "bulk import": that path belongs to the code bundles, which index folders of source. A
memory brain is populated one distilled memory at a time (see the brain how-tos).

### Added by `lsp` — coding-plus, full only

`lsp_def` · `lsp_refs` · `lsp_hover` · `lsp_symbols` — cross-file intelligence via a language server
(`rust-analyzer`, `tsserver`, `pyright`). Compile-gated with `#[cfg(feature = "lsp")]`; absent below
coding-plus.

### Added by `docs` + `ocr` — full only

Document ingestion: `ingest` accepts **PDF and DOCX** (via `docs`) and runs **OCR on scanned-image
pages** (via `ocr`). Lower bundles' `ingest` (coding/coding-plus) handles source + SQL + text but not
office documents or scanned PDFs.

### `forge` — optional, orthogonal (not part of the four shipped bundles)

`forge` is a separate crate, not an `sca-core` feature, and is **not** enabled by the brain/coding/
coding-plus/full aliases. When compiled in, it adds `forge_*` tools (CLI `said forge …`). See
[`09-cargo-features.md`](09-cargo-features.md#forge--spec-driven-workspace-generator-said-cli--said-mcp-only)
and [`05-features/forge.md`](05-features/forge.md).

## Admin sub-actions: basic in every bundle, compliance in full only

The `admin` tool/command dispatches on an `action`. The **recovery** actions ship in every bundle; the
**compliance** actions ship only in the `full`/Enterprise bundle (gated behind the `enterprise` cargo
feature).

**Recovery — every bundle (brain and up):**

| Action | Aliases | What it does | Required params |
|--------|---------|--------------|-----------------|
| `list-tombstones` | `list` | List deleted/superseded frames in the recycle bin (optional `like` filter). | `action` (+ `like`) |
| `restore` | — | Restore a tombstoned memory as the active head; the current head becomes a tombstone. | `action`, `doc_id` |
| `who-deleted` | `lineage` | Full version/deletion trail for a memory (active, tombstoned, superseded-by links, attribution). | `action`, `doc_id` |

**Compliance — `full`/Enterprise bundle only (`enterprise` feature):**

| Action | Aliases | What it does | Required params |
|--------|---------|--------------|-----------------|
| `legal-hold-add` | `hold-add` | Pin frames so retention sweeps cannot purge them (litigation/compliance hold). | `action`, `doc_id`, `case` |
| `legal-hold-release` | `hold-release` | Remove a legal hold from a doc's frames. | `action`, `doc_id`, `case` |
| `retention-sweep` | `sweep` | Purge tombstones past an age cutoff; keep N newest per doc; skip legal holds. | `action` (+ `older_than_days`, `keep_per_doc`) |
| `audit` | `audit-log` | Tamper-evident audit log of admin actions (remember, restore, legal holds, …); chain-verified. | `action` (+ optional `doc_id`/`case` filters) |

On a non-Enterprise build, a compliance action returns a clear "needs the Enterprise build" message
(not a silent "unknown action"), so the caller knows the capability exists and where to get it.

> **Why gated:** legal holds, retention sweeps, and tamper-evident audit are regulated-deployment
> (SOX/GDPR/HIPAA) features, not personal-memory features — they belong to the paid Enterprise tier.
> Everyday recovery (undo a delete, see a memory's history) stays free in every bundle.

> **Schema-parity note (fixed):** `audit` was fully implemented in the handler but was **missing from
> the MCP `admin` tool-schema description**, so an agent reading the schema never learned it existed.
> The schema now lists every action it dispatches (with the compliance ones marked Enterprise-only).
> This is the same class of surface-parity defect tracked in
> [FIXES-LOG](FIXES-LOG.md) #15; the standing rule is: **every implemented action/tool must be
> discoverable from its own schema.**

## The rule this page enforces

When documenting or building a walkthrough, **name the bundle** and only reference commands that bundle
ships. Concretely:

- A **brain** guide must not mention `search`, `sym`, `init`, `overview`, `ingest`, `snapshot`, or any
  LSP tool — they don't exist there. (This is why the brain-MCP `status` "next steps" and the brain CLI
  help were corrected to suggest only `ask`/`remember`/`get`.)
- A **coding** guide may use the `code` surface but not the LSP tools (those need coding-plus).
- Only a **full** guide may document PDF/DOCX/OCR ingestion.

## File-lifecycle safety guarantees (every bundle)

Two invariants protect a user's data. They hold on **all** surfaces (CLI, MCP) and **all** bundles.

### 1. No tool ever deletes a `.said` file

`delete` (and every other tool) operates on memories **inside** the brain — it tombstones frames
(recoverable via `admin restore`). **Nothing deletes the `.said` file itself.** The recycle bin lives
*inside* the file, so deleting the file would be total, unrecoverable loss with no undo — an agent must
never be able to do that. The one place the MCP does `fs::remove_file` is an **empty, never-populated
placeholder brain it auto-created itself** (guarded by `is_pristine_brain`: <32 KB, openable, **zero**
active frames). A populated brain is never fs-deleted by any code path. Removing a real brain is a
**manual, human-only** act — the user deletes the file by hand.

### 2. One brain per machine on the free tier (multi-brain is Enterprise)

`create` is one-brain-per-PC by default: once a brain is registered (`said use`), a second `create` is
refused and points the user to grow the existing brain (`init`/`remember`). On the free/dev tiers this
holds even with `--force`. **Creating multiple brains on one machine is an Enterprise (`full` build)
capability** — only there does `--force` (CLI) / the MCP `create` tool make a second, different brain.
Both surfaces enforce it; the free build's message names the Enterprise upsell. (Re-attaching or
overwriting the *same* file is always allowed; a populated file is never silently overwritten.)

## Documented behaviors that are correct-by-design (not bugs)

Surfaced during testing; each is intended and worth knowing, not a defect:

- **Contradicting facts coexist.** Two memories with **different** ids that disagree (API port 8080 vs
  9090) both persist and both can surface in `ask` — the brain preserves conflicting facts rather than
  silently overwriting. **Same id** = a version chain (latest wins in `ask`; `history`/`checkout` reach
  the priors).
- **Retention-sweep is destructive beyond the recycle bin.** `retention-sweep` (Enterprise) *permanently*
  removes swept frames — after a sweep, `checkout` to a swept version fails ("frame is deleted"). This is
  the point (GDPR/SOX erasure): the recycle bin is a soft tier; the sweep is the hard tier. `legal-hold`
  frames are skipped.
- **Procedural pillar ranks high.** Workflow/rule memories (`procedural`) score higher salience than
  plain facts — intended, so "how we do X" surfaces above trivia.
- **Enterprise mode is pointer-only.** A brain created with `--mode enterprise` refuses content-embedding
  ingests (stores URI + summary); explicit `remember` still works. Mode is immutable at creation.

## See also

- [`09-cargo-features.md`](09-cargo-features.md) — the build/compile view: what each flag pulls in, binary sizes.
- [`07-cli-reference`](07-cli-reference/) / [`08-mcp-reference`](08-mcp-reference/) — full per-command / per-tool detail.
- [`39-packaging-and-distribution.md`](39-packaging-and-distribution.md) — how the bundles are built and shipped.
- [`35-production-build.md`](35-production-build.md) — the production surface-parity standard.
