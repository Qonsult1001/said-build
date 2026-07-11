# CLI reference

Every `said <cmd>` subcommand shipped by [`crates/said-cli`](../../../crates/said-cli/). Grouped for readability.

## Global options

Every subcommand accepts:

```
said [--path <file.said>] [--json] <subcommand> [args]
```

- `--path` — target `.said` file (overrides the "default brain" from `said use`)
- `--json` — JSON output instead of human-readable (where supported)

## Reading (no mutation)

- [ask](ask.md) — smart 3-engine fusion (Sym + Grep + SCA). **The primary query verb.**
- [search](search.md) — pure semantic via SCA (deprecated in favor of `ask` for most use cases)
- [sym](sym.md) — exact symbol name lookup (code intelligence)
- [grep](grep.md) — literal substring via trigram index
- [get](get.md) — fetch full content by `doc_id`
- [history](history.md) — lineage trail for a symbol or `doc_id`
- [stats](stats.md) — file + brain state (mode, frame counts, S_slow magnitude, …)
- [discover](other-commands.md#said-discover) — detect modules/products in a monolithic codebase
- [overview](other-commands.md#said-overview) — list detected modules

## Writing

- [create](create.md) — make a new empty `.said` file with a mode (portable/enterprise)
- [add](add.md) — write a memory (text or file)
- [remember](remember.md) — shorthand for `add` with pillar hints
- [init](init.md) — bulk-ingest a directory via tree-sitter + all plugins
- [ingest](ingest.md) — single file or folder with `--pointer` for Enterprise
- [import](import.md) — import the user's OWN personal data (`browser` / `email` / `chatgpt` / `claude`; brain + full bundles) **or** migrate from mem0 / memvid (`import from`)
- [checkout](checkout.md) — restore a past version as the new HEAD
- [edit](edit.md) — surgical anchored insert/replace/delete on a source file (no whole-file rewrite)

## Admin (enterprise ops)

- [admin list-tombstones](admin.md#list-tombstones) — recycle bin view
- [admin restore](admin.md#restore) — undelete a frame
- [admin who-deleted](admin.md#who-deleted) — lineage + attribution for `doc_id`
- [admin legal-hold-add / release](admin.md#legal-hold) — block retention sweeps
- [admin retention-sweep](admin.md#retention-sweep) — drop tombstones by age / keep-per-doc
- [admin audit](admin.md#audit) — view/verify the BLAKE3-chained AUDT log

## Maintenance

- [delete / forget](delete.md) — soft-delete a frame
- [compact](compact.md) — reclaim bytes after tombstones + drop-history
- [sync](sync.md) — detect + tombstone files that disappeared from disk
- [clean](clean.md) — remove dangling state

## Module workflow (monolith → extracted module)

- [snapshot](snapshot.md) — extract a module into its own folder + lens (glass view over parent brain)
- [sandbox](sandbox.md) — deploy a snapshot against a real SQL Server in Docker for cross-module interaction testing

## Shortcuts / utilities

- [use](other-commands.md#said-use) — set the default brain for subsequent commands
- [journal](other-commands.md#said-journal) — append a timestamped journal entry

## Binary build

```
# Feature bundles (all build with --no-default-features; each bakes the encoder in):
cargo build --release -p said-cli --no-default-features --features coding   # code intel
cargo build --release -p said-cli --no-default-features --features full     # code + docs + OCR + LSP
```

Bundles: `brain` (memory only) · `coding` (+code) · `coding-plus` (+lsp) · `full` (+docs/ocr/lsp).
See [Cargo feature flags](../09-cargo-features.md) for what each feature brings in.

## Full command list

From `said --help`:

```
Commands:
  create        Create a new .said file
  add           Add a document (text or file or directory)
  remember      Remember text
  search        Semantic search
  ask           Smart router — sym + grep + SCA fusion
  sym           Exact symbol lookup
  grep          Literal substring search
  get           Read a frame by doc_id
  history       Lineage for a symbol or doc_id
  checkout      Restore a past version
  edit          Surgical anchored edit of a source file (insert/replace/delete)
  delete        Soft-delete a frame
  stats         File + brain stats
  discover      Detect modules in a codebase
  overview      List detected modules
  snapshot      Extract a module workspace
  sandbox       Test a module subset in isolation
  clean         Remove dangling state
  init          Bulk-ingest a directory
  ingest        Ingest a document, video, or folder
  import        Import your data (browser/email/chatgpt/claude) or migrate (from mem0/memvid)
  admin         Recycle bin + compliance (list/restore/legal-hold/audit/sweep)
  use           Set default .said file
  journal       Append a timestamped journal entry
  sync          Sync filesystem changes into the brain
```

MCP equivalents: see [MCP reference](../08-mcp-reference/README.md).
