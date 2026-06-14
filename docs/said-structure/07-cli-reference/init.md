# said init

Bulk-ingest a whole directory. Designed for codebases (tree-sitter AST chunking per language). Works on non-code directories too by falling back to document parsing.

## Usage

```
said init <DIR> [--incremental]
```

## Arguments

- `<DIR>` — directory to ingest
- `--incremental` — reserved; currently a no-op. Intended for partial updates when a directory has grown since last init.

## Behavior

1. Canonicalize the directory path
2. Auto-name the `.said` file after the directory's name (`project_name.said` placed in CWD). Override with `--path` at the CLI root.
3. Load `.gitignore` rules; walk the directory skipping ignored paths
4. Filter files to supported extensions:
   - **Code** (via [code plugin](../06-ingestion-plugins/code.md)): `.rs`, `.py`, `.js`, `.ts`, `.go`, `.java`, `.cs`, `.sql`
   - **Text** (via [docs plugin](../06-ingestion-plugins/docs.md)): `.txt`, `.md`, `.pdf`, `.docx`
5. For each file:
   - Code → tree-sitter AST chunk, write one frame per symbol + populate symbol index
   - Text → paragraph chunk, write one frame per chunk
   - Tag every frame with `source:<relative_path>`, `format:<ext>`
6. `brain.build_index()` — SCA + trigram + symbol index
7. `brain.compact()` — block-compress, train zstd dict
8. `brain.save()` — atomic write to disk

**Refused on Enterprise brains** (`ensure_content_ingest_allowed()` error). Use pointer-mode `said ingest --pointer` instead.

## Output

Live progress:

```
Indexing: crates/sca-core/src/frames.rs (32 symbols)
Indexing: crates/sca-core/src/said_file.rs (78 symbols)
...
✓ Indexed 5132 files into said.said (27,418 frames, 267 MB uncompressed → 18 MB compressed)
  Symbol table: 12,847 unique names
  Trigram index: built
  Compression: 14.8×
```

JSON mode (`said --json init .`):

```json
{
  "files_indexed": 5132,
  "frames_stored": 27418,
  "symbols": 12847,
  "uncompressed_bytes": 267000000,
  "compressed_bytes": 18000000,
  "compression_ratio": 14.8,
  "elapsed_ms": 343000
}
```

## Re-init behavior

If the `.said` file already exists:

```
  Re-init over existing said.said (preserving frames + brain)
```

Frames + brain state are preserved. New files get new frames; files with identical BLAKE3 (unchanged on disk) get skipped (dedup). Changed files lineage the old frame to Tombstone (byte-exact-restorable).

This makes `said init .` idempotent + cheap after the first run — only new / changed files get re-processed.

## Performance

Real-world observations:

| Repo | Files | Frames | Time |
|---|---|---|---|
| SAID-ECHO (self) | 5132 | 27,418 | 5 min 43 sec |
| Legal-archive (docs) | 4127 | 19,149 | 12 min |
| Small service (~500 files Rust) | 487 | 2,103 | 38 sec |

Dominated by SCA encoding + compact, not by tree-sitter parse.

## Examples

```bash
# The canonical flow — auto-name the brain after the repo
said init .
# → creates ./SAID-ECHO.said (or ./<project>.said) with everything indexed

# Explicit target
said --path /brains/project-x.said init /code/project-x/

# Re-init after pulling from remote — only new/changed files re-ingested
cd repo/
git pull
said init .
```

## What gets skipped

- `.gitignore`d paths (node_modules, target, .venv, build artifacts)
- Binaries, images, zips that don't match a supported extension
- Files exceeding a large size threshold (avoids embedding multi-GB binaries)
- Files where BLAKE3 matches an existing frame (dedup)

## How to extend

### Add a new extension
1. Update `CODE_EXTENSIONS` or `TEXT_EXTENSIONS` list in [`said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs)
2. Add the parser (tree-sitter grammar for code, format handler for text)
3. Test on a sample repo

### Custom chunking policy
Currently one-frame-per-symbol for code, one-frame-per-paragraph for text. A per-method chunker for Java / C# monoliths is a reasonable future addition.

## Known limitations

- `--incremental` is a no-op; in practice the BLAKE3 dedup in step 6 gives you incremental behavior for free
- Frames land with `pillar:code` tag but `FrameMeta.pillar = Pillar::Memory` (direct `put_with` path — see [Known limitations](../11-known-limitations.md))
- Large monorepos (>100k files) stress the mmap path; works but takes 20+ min

## Matching MCP tool

[MCP `init` tool](../08-mcp-reference/init.md) — same contract; accepts `dir` param.

## See also

- [code plugin](../06-ingestion-plugins/code.md)
- [docs plugin](../06-ingestion-plugins/docs.md)
- [3.4 FrameStore](../03-core-subsystems/3.4-framestore.md) — where everything gets stored
