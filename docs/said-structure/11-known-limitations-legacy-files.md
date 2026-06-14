# Legacy file recovery — pre-2026-05-04 builds

Status: **known issue, fix-forward only**.
Last updated: 2026-05-04

## What happened

Builds of `sca-core` before commit `<TODO-fill-on-merge>` had a bug in
`SaidFile::save()` that produced corrupted frame offsets in any brain that:

1. Had ever been compacted (i.e., contained block-encoded frames), AND
2. Had subsequently received re-ingest / edit / resurrect operations
   that produced new pending Plain frames.

Symptom in the affected file: tombstoned-frame metadata claims
`encoding=Plain` with an `offset` value pointing inside the DICT, BLKT,
or other section bytes. `read_frame_by_id` returns the wrong bytes (or
returns `None` when those bytes aren't valid UTF-8).

User-visible effect:

- `said grep` returns 0 matches even for content known to be in the file
- `said get <doc_id>` returns "Document not found"
- WASM admin panel: "memory N not found" when previewing tombstones
- Diff modal shows "no content changes" or empty content

## What "fixed" means

Builds from 2026-05-04 onwards do `compact()` before any `save()` if
there are pending Plain frames in a brain that has blocks. New mutations
fold cleanly into a fresh block, so the broken layout never occurs.

## Files written by an older build

**There is no in-place repair.** The compromised offsets are persisted
in the FTC2 frame table and BLKT block table; they reference bytes that
no longer exist where they should. Any tool that loads such a file sees
the same broken state.

The recovery path is to **re-ingest the source documents** into a fresh
`.said` file:

```bash
# 1. Save your old brain's audit log + tombstone list for reference
said --path old.said admin audit > old-audit.txt
said --path old.said admin list-tombstones > old-tombstones.txt

# 2. Create a new brain
said create new.said

# 3. Re-ingest the source documents (or use the hash-tags from
#    `said admin list-tombstones` to figure out what was loaded)
said --path new.said ingest /path/to/sources/

# 4. The new brain is clean. Old version history (lineage, who-deleted)
#    is gone — the audit log and tombstones from the old file are
#    preserved as text files (step 1) for record-keeping but cannot be
#    folded back into the engine.
```

## Migration helper (proposed, not implemented)

A future `said admin migrate <old.said>` command could:

1. Walk the OLD file's FTC2, find every frame regardless of broken
   offsets
2. For each block-encoded frame: read its **block** (which is correct
   in the old file — only Plain offsets were broken)
3. For each tombstoned Plain frame: skip with a warning (bytes are
   irrecoverable)
4. Write a new `.said` containing every readable frame

This would salvage all block-encoded content but lose any Plain-encoded
tombstones. For a brain that's been compacted at least once, that's
~99% of content. Worth building if anyone has irreplaceable old brains;
not in scope for now.

## How to detect an affected file

```bash
# Quick check: does grep over a known-present phrase return matches?
said --path suspect.said grep "<phrase you know is in there>"
# If 0 matches → file is corrupt.

# Deeper check: probe a tombstone
said --path suspect.said admin list-tombstones | head -1
# Pick a frame_id from output, then:
cargo run --release -p sca-core --example read_frame_test -- suspect.said <frame_id>
# "OK: N bytes" → frame readable
# "read_frame_by_id returned None" → corrupt
```

## Why this happened

Honest post-mortem: `save()` had two layout branches (`has_blocks` and
not). The `has_blocks` branch wrote block_data → DICT → BLKT → SCRM
sections, but never accounted for **pending Plain frames** that had
accumulated since the last compact. Those frames had metadata entries
pointing at file positions that, after the new save, contained the
DICT/BLKT bytes instead of the original frame content.

The bug was latent because:
- Fresh ingests (no prior blocks) hit the no-blocks branch — fine
- Compacted brains that never got further mutations hit the has_blocks
  branch with no pending Plain frames — fine
- Only the combination "compacted + then mutated + then saved" tripped it

The 2026-05-04 fix forces compact-on-save when both conditions are
present, eliminating the trailing-Plain-frame layout problem entirely.

## References

- `crates/sca-core/src/said_file.rs` — `save()` entry point with the
  compact-on-save guard at the top.
- `crates/sca-core/src/frames.rs` — `has_pending()` helper added for the
  guard's check.
- [11-known-limitations.md](11-known-limitations.md) — general known-issues
  index.
