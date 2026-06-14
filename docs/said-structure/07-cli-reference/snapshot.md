# said snapshot

Extract a module out of a monolithic codebase into its own physical folder + a **lens** (glass view) onto the parent brain.

Snapshot is the first half of the "modularize a monolith" workflow. The second half is [`said sandbox`](sandbox.md), which takes the snapshot output and boots it on a real database so you can prove the module works standalone before you actually cut it out.

## The problem it solves

You have one `.said` brain ingested from a 977-table SQL Server monolith (or an 80k-file TypeScript repo, etc.). Somewhere inside it is a coherent "card" module: tables, procs, triggers, views, functions, TS services, tests. You need to:

1. **See it in isolation** — all card-related objects in one place, nothing else
2. **Know what's safe to move** vs **what's shared infrastructure**
3. **Keep the parent brain authoritative** — no copy/paste drift
4. **Pick it up on a fresh laptop** even if the original source tree is gone

Snapshot produces a folder-on-disk plus a tiny `.said` lens that answers all four.

## Usage

```
said [--path FILE] [--json] snapshot <MODULE> [--output DIR]
```

- `<MODULE>` — semantic module name (e.g. `card`, `billing`, `onboarding`)
- `--output DIR` — override output directory (default: `.said-code/<module>.<brain-stem>/`)

### Examples

```bash
said snapshot card
# → .said-code/card.vivere/  (when your brain is vivere.said)

said snapshot billing --output ~/modules/billing
# → ~/modules/billing/
```

## Output layout

```
.said-code/card.vivere/
├── Exclusive/                  ← objects this module OWNS
│   ├── dbo/card/
│   │   ├── card_table.sql
│   │   ├── card_activate_proc.sql
│   │   └── card_audit_trigger.sql
│   └── src/card/
│       ├── card-service.ts
│       └── card-api.ts
├── Shared/                     ← objects this module TOUCHES but doesn't own
│   ├── users_table.sql         ← hub table (used by card + billing + auth)
│   ├── users.CARD_USAGE.md     ← which card procs hit it
│   └── billing.CARD_USAGE.md   ← card files that import from billing
├── BOUNDARY.md                 ← architectural analysis
├── MODULE_MAP.md               ← complete object inventory
└── card.vivere.said            ← LENS FILE (16-byte magic + pointers)
```

Nothing here is copied from the parent brain's **frame store**. Every `.sql` / `.ts` file in `Exclusive/` and `Shared/` is either:

1. **Copied from disk** if the original source tree is still reachable, OR
2. **Reconstructed from brain content** — concatenating every chunk whose `doc_id` starts with `<file_path>::` in source-order (line-number sort) rebuilds a usable file byte-for-byte (or nearly so — comment whitespace may differ)

That second path is the one that makes snapshots survive a wiped laptop. As long as you still have the `.said`, you can get back to code.

## The lens file — "glass view"

The file named `card.vivere.said` in the output dir is **not** a copy of the parent brain. It's a ~kilobyte-scale pointer.

### Layout (binary)

```
┌───────────────────────────────┐
│ 4 bytes  magic "LENS"         │
├───────────────────────────────┤
│ 4 bytes  parent_path_len (LE) │
│ N bytes  parent_path utf-8    │ ← relative to lens file (e.g. "../../vivere.said")
├───────────────────────────────┤
│ 4 bytes  module_name_len      │
│ N bytes  module_name utf-8    │ ← "card"
├───────────────────────────────┤
│ 8 bytes  snapshot_time (u64)  │ ← unix epoch secs
├───────────────────────────────┤
│ 4 bytes  frame_count          │
│ repeated:                     │
│   2 bytes doc_id_len          │
│   N bytes doc_id utf-8        │
└───────────────────────────────┘
```

Lives in [`crates/sca-core/src/lens.rs`](../../../crates/sca-core/src/lens.rs) as [`LensFile`](../../../crates/sca-core/src/lens.rs#L20-L33).

### Why "glass view"?

Because when you query it:

1. The query opens the lens file → reads the parent path → opens the parent brain
2. The parent's SCA / symbol / trigram indexes do all the work
3. Results get **filtered through `frame_ids`** — a doc_id not in the set is dropped
4. What reaches the user is "only card-related hits from vivere.said"

You look through the lens; the data stays in the parent. No frame is ever duplicated. No synchronization is needed. If you ingest a new `card_new_proc.sql` into `vivere.said`, the lens sees it on the next query (once you re-run snapshot to update `frame_ids`).

### Sync semantics — primary vs secondary

| What changed | What the lens sees |
|--------------|-------------------|
| Parent brain gets new frames tagged `module:card` | ❌ not in `frame_ids` yet — re-snapshot to pick up |
| Parent brain updates existing card frame (new version) | ✅ lens reads the current HEAD of that doc_id automatically |
| Parent brain tombstones a card frame | ✅ lens filters it out (tombstoned = not Active) |
| Lens file deleted | ❌ re-run `said snapshot card` to regenerate |
| Parent brain file moves | ❌ edit the `parent_path` in the lens (or re-snapshot) |

**The parent is always authoritative.** The lens is derived state. You never edit the lens manually.

### Why the parent brain is never mutated

Older drafts of snapshot added a `module:card` tag onto every card frame in the parent. Shipping that way triggered a full brain re-save (blocks have to be rewritten), and `save()` has a known corner case where pre-existing blocks-on-disk aren't copied forward when there are no pending block leaders — leaving blocks in the old mmap which is invalidated by the atomic rename on Windows.

The lens-own-its-frame-ids design sidesteps this entirely: the parent stays **read-only** during snapshot. This also matches the contract `"no copy-paste, no drift"` — there is only one place any card frame lives.

## How modules are detected

Lives in [`cmd_snapshot`](../../../crates/said-cli/src/main.rs#L3817-L4530) in said-cli.

**Step 1 — deep semantic recall.** Runs a 500-result recall against the parent brain with a compound query:
```
all stored procedures tables triggers views and functions related to
<module> management processing configuration
```
Every returned doc_id goes into the candidate set.

**Step 2 — path-prefix sweep.** Any doc_id whose string contains the module name (case-insensitive) joins the set. This catches anything the embedding missed.

**Step 3 — classification.** For each candidate the frame's `title` metadata decides what kind of object it is: `(table)`, `(proc)`, `(trigger)`, `(view)`, `(function)`. Code frames get routed by tree-sitter's symbol kind (`class`, `fn`, `struct`, etc.).

**Step 4 — hub-table detection.** A table is a "hub" if ≥ 5 foreign keys across the whole brain point at it. Hubs go to `Shared/`; non-hubs stay with the module. FK counts come from the `fk:<table>.<column>` tags ingested by the SQL docs plugin.

**Step 5 — shared-proc analysis.** For every hub table, scan each module proc/trigger/view's content: if it references the hub by name, record `(hub_table → module_procs_that_touch_it)` in the usage ledger.

**Step 6 — Exclusive vs Shared split for code.** A source file is Exclusive if its path contains the module name as a path segment (e.g. `src/card/foo.ts` for module=`card`). Otherwise Shared.

## BOUNDARY.md

Architect-facing write-up. Sections:

- **Module Statistics** — count of tables/procs/triggers/views/functions
- **Hidden Triggers** — every trigger in the module grouped by parent table, with fire event (INSERT / UPDATE / INSERT_UPDATE_DELETE / INSTEAD OF) and line size. The header reads: *"These triggers fire automatically on INSERT/UPDATE/DELETE. The new API must replicate this logic in the application layer."*
- **Exclusive Objects** — tables (with trigger count + FK list), stored procedures (line range + `dynamic_sql` / `refs:` / `check:` tags), triggers, views, functions
- **Shared Hub Tables** — per hub, how many of this module's procs touch it, with a top-10 list. Header reads: *"These tables should NOT be moved — create API contracts instead."*
- **Code Dependencies** — for non-SQL repos, the shared modules this one imports from, grouped by folder

## MODULE_MAP.md

Developer-facing complete inventory. Lists every symbol in the module with name, file, line range, line count. Grouped by kind (Tables, Stored Procedures, Triggers, Views, Indexes, Classes, Functions, Methods, Structs, Enums, Traits, Impls, Constants, Functions/Exports, Other).

Followed by **Source Files** listing — Exclusive (N files — safe to extract) and Shared (N files — need interfaces).

Meant to be used as a migration checklist: tick each row off as you rebuild it in the new stack.

## File reconstruction from brain content

Every chunk that was ingested via `said init` or `said ingest` carries the original `source:<path>` tag plus its line range in the `doc_id` (format: `path::NAME::kind:start_line`). Snapshot exploits this:

```rust
// Pseudocode from cmd_snapshot step 5
for file_part in module_files {
    // First try: copy from disk
    if let Ok(bytes) = read_from_source_root(file_part) { write_to_exclusive(bytes); continue; }

    // Fallback: reconstruct from chunks in the brain
    let prefix = format!("{}::", file_part);
    let chunks = doc_ids.iter()
        .filter(|id| id.starts_with(&prefix))
        .map(|id| (parse_start_line(id), brain.get(id).unwrap()))
        .sorted_by_key(|(line, _)| *line)
        .collect();
    write_to_exclusive(chunks.iter().map(|(_, c)| c).join("\nGO\n"));
}
```

For SQL, the `\nGO\n` separator is the batch terminator — correct for SQL Server. For code languages it's usually overkill but harmless (tree-sitter chunks are typically whole items so they already have trailing newlines).

**Caveat**: if the brain was ingested from an Enterprise-mode `.said` (pointer-only), chunks have no content — reconstruction returns empty, and snapshot prints a warning per unreachable file. See [Brain mode](../05-features/row-37-brain-mode.md).

## Export for handoff / compilation

Once you have `.said-code/card.vivere/`, the folder is a standalone artifact:

```bash
# Zip and hand to another engineer
tar czf card-for-review.tgz .said-code/card.vivere/
# They can open it anywhere:
cd /tmp/card-for-review
said --path card.vivere.said ask "why does card_activate have dynamic sql"
# ↑ this works ONLY if vivere.said is also reachable at the lens's parent_path.
```

If you want a **self-contained** artifact that does not need the parent brain, you have two options:

1. **Use the generated source tree.** `Exclusive/` and `Shared/` contain the actual SQL / code. Ship those plus `BOUNDARY.md` + `MODULE_MAP.md` — the receiver does not need `said` installed at all.
2. **Promote the lens to a full brain.** Run `said init .said-code/card.vivere/` against just the extracted folder — produces a standalone `card.said` that embeds the frames for those files only. Bigger (~10% of parent size) but independent.

## Dropbox / shared-drive workflow

Snapshot is path-aware. The lens stores `parent_path` as **relative if possible** (computed with `pathdiff::diff_paths`). This means:

```
~/Dropbox/vivere/
├── vivere.said                           ← parent brain
└── .said-code/
    ├── card.vivere/card.vivere.said      ← parent_path = "../../vivere.said"
    └── billing.vivere/billing.vivere.said ← parent_path = "../../vivere.said"
```

Sync the whole `~/Dropbox/vivere/` tree to a colleague's machine — the relative paths still resolve because both the parent brain and the lenses move together. Drop the tree into `C:\Users\Jane\Dropbox\vivere\` and `said --path card.vivere.said` works without modification.

If the parent path turns absolute (Dropbox on a different OS, different username) you'll see:
```
Error: parent brain not found at /home/alice/... — edit parent_path in lens
```
Re-run `said snapshot card` to regenerate with the correct relative path.

## How to test

```bash
# Prereq: brain with at least one module-shaped subset
said init ./sql-monolith --output mono.said

# Extract
said --path mono.said snapshot card
ls .said-code/card.mono/
# → BOUNDARY.md  MODULE_MAP.md  Exclusive/  Shared/  card.mono.said

# Query through the lens
said --path .said-code/card.mono/card.mono.said ask "activate card"
# → only card-module hits

# Verify glass view (no copy): sizeof(lens) << sizeof(parent)
ls -la mono.said .said-code/card.mono/card.mono.said
```

Canonical integration test: ingest a small multi-module SQL fixture, snapshot one module, verify that (a) exclusive count > 0, (b) shared > 0 (hub exists), (c) lens frame_ids ⊆ parent active doc_ids, (d) reconstructed file bytes match source.

## Known limitations

- **No automatic re-snapshot on parent changes.** If you add a new card proc to the parent, the lens doesn't update until you re-run `said snapshot card`. Listed in [11-known-limitations](../11-known-limitations.md).
- **Hub threshold is hard-coded at ≥ 5 FKs.** Tables with 4 inbound FKs count as Exclusive even though they look shared. Tunable as a const in `cmd_snapshot`.
- **Module-name path matching is case-insensitive substring.** `"card"` will match `cardio_metrics_table.sql` — disambiguate with `--output` + selective cleanup, or use more specific module names.
- **Content reconstruction requires Portable mode.** Enterprise brains store pointers, not content, so fallback-from-brain doesn't produce usable files.

## Matching MCP tool

[`snapshot`](../08-mcp-reference/other-tools.md#snapshot) — same semantics, args = `{module, output?}`.

## See also

- [said sandbox](sandbox.md) — the next step: boot this extracted module on a real DB
- [said discover](other-commands.md) — auto-detect candidate module names before snapshotting
- [said overview](other-commands.md) — list what discover found
- [Row 46 — Procedural pillar](../05-features/row-46-procedural.md)
- [`LensFile`](../../../crates/sca-core/src/lens.rs) — implementation
