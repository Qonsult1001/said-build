# Other CLI commands

Reference for every remaining `said <cmd>` subcommand. Each section is short — detailed pages exist for the core verbs (ask, create, ingest, init, import, admin).

## Reading

### said search

Pure SCA semantic search. Superseded by [ask](ask.md) for most use cases.

```
said [--path FILE] search <QUERY> [--top N]
```

Returns the pure SCA layer only (no grep / sym fusion). Useful when you want raw semantic similarity without the confidence-threshold logic.

### said sym

Exact symbol lookup against the SYMS index. Sub-millisecond.

```
said [--path FILE] sym <NAME> [--max N]
```

Output:
```
FrameStore    crates/sca-core/src/frames.rs::FrameStore  (struct:325-354)
```

Only exact equality hits count. Use `said sym` + `said ask` together: ask for conceptual, sym for exact.

### said grep

Literal substring via trigram index.

```
said [--path FILE] grep <PATTERN> [--max N]
```

Sub-millisecond on corpora up to ~100k frames. Case-insensitive. Pattern is treated as literal text (no regex).

### said get

Fetch full content by `doc_id`.

```
said [--path FILE] get <DOC_ID>
```

Outputs the frame's body verbatim. Useful when `ask` / `search` returned a truncated preview and you want the whole thing.

### said history

Lineage trail for a symbol or doc_id.

```
said [--path FILE] history <NAME>
```

Walks the `superseded_by` chain from the Active head backward, showing every version ever stored:

```
v0 [genesis]       frame #0  created_at=1711000000
v1 [delta=0.12]    frame #1  created_at=1711100000  ← "typo fix"
v2 [delta=0.68]    frame #2  created_at=1711200000  ← major rewrite
v3 [current HEAD]  frame #3  created_at=1711300000
```

`v<N>` maps to the `--version N` argument of `said checkout`.

### said checkout

Restore a past version as the new HEAD — git-style time travel.

```
said [--path FILE] checkout <NAME> [--version N | --frame ID] [--write]
```

Behavior:
1. Locate the requested frame by version index OR frame_id
2. Create a NEW Active frame carrying that content
3. Demote the current HEAD to Tombstone
4. Optional `--write` — also write the restored content back to the source file on disk (only for whole-file frames, not AST chunks)

History grows by one entry — checkout is a real event, not a rewind.

### said stats

File + brain state summary.

```
said [--path FILE] stats [--json]
```

Shows: Brain mode, frame counts, tombstone overhead, compression ratio, SCA doc count, symbol table size, trigram presence, brain state (S_slow magnitude, dream cycles, pending dream queue).

Sample output:
```
=== .said File Stats ===
  Brain mode:        portable  (embeds full content; works offline)
  File size:         17 MB
  Active frames:     19149
  Tombstones:        305 frames, 4.2 KB
  Compressed:        13 MB
  Uncompressed:      192 MB
  Compression ratio: 14.5×

=== Brain State ===
  Query log:         1847 entries
  Dream cycles:      12
  s_slow magnitude:  42.17
  Pending dream:     34 queries  (auto-dreams at 500)
```

### said discover / said overview

Detect logical modules in a monolithic codebase — typically used on SQL schemas.

```
said [--path FILE] discover             # detect modules
said [--path FILE] overview [--check]   # list detected modules
```

Clustering by naming-convention prefixes, table co-occurrence, FK relationships. Output is a module list with member counts.

### said snapshot / said sandbox

Extract a module's frames into a sub-brain.

```
said [--path FILE] snapshot <MODULE> [--output <FILE>]
said [--path FILE] sandbox <MODULES...> [--port N] [--compare <FILE>] [--up]
```

`snapshot` produces a standalone `.said` file containing just the module's frames — useful for sharing a focused sub-brain without the whole repo.

`sandbox` is a specialized workflow for SQL schema exploration — deploys a module-subset against a test DB, compares behavior vs a reference.

## Writing

### said add / said remember

Short-form ingest of a single text or file.

```
said [--path FILE] add "<TEXT>"
said [--path FILE] add "<TEXT>" --id <DOC_ID> --title <TITLE>
said [--path FILE] add --file <PATH>
said [--path FILE] add --dir <PATH>
said [--path FILE] remember "<TEXT>"
```

`remember` is a thin alias for `add` with text.

`--dir` uses the `docs` plugin pipeline for a whole directory — simpler than `init` (no tree-sitter AST); mainly used for notes/docs archives.

### said use

Set the default brain for subsequent commands.

```
said use <FILE>
```

Persists in a user-config file. Subsequent `said` commands without `--path` use this default.

### said journal

Append a timestamped journal entry as an Episodic frame.

```
said [--path FILE] journal "<ENTRY>"
```

Shorthand for `add` with pillar=Episodic + auto-generated title with timestamp.

## Maintenance

### said delete / said forget

Soft-delete a frame.

```
said [--path FILE] delete <DOC_ID>
said [--path FILE] forget <DOC_ID>
```

Flips status to `Deleted` (skipping the intermediate `Tombstone` step used for lineage replacements). The frame is still recoverable via `said admin restore` until the next `said compact --drop-history`.

### said compact

Rebuild compressed blocks, optionally drop history.

```
said [--path FILE] compact [--drop-history] [--keep-per-doc N]
```

- No flags — rebuild blocks after many writes; reclaim space from Deleted frames
- `--drop-history` — convert every Tombstone to Deleted (reclaimable by next compact), honoring legal holds
- `--keep-per-doc N` — keep the N most recent tombstones per doc_id even when dropping history

### said sync

Detect files missing from disk vs `source:<path>` tags in the brain; offer to tombstone the orphaned frames.

```
said [--path FILE] sync [--dry-run]
```

Useful after moving or deleting source files — keeps the brain in sync with the filesystem.

### said clean

Remove dangling state (orphaned pending frames, partial-compact leftovers).

```
said [--path FILE] clean
```

## See also

- [ask](ask.md) / [create](create.md) / [ingest](ingest.md) / [init](init.md) / [import](import.md) / [admin](admin.md) — detailed pages
- [MCP reference](../08-mcp-reference/README.md) — same operations via MCP
