# MCP tool: init

Bulk-ingest a directory. Mirror of CLI [`said init`](../07-cli-reference/init.md).

## Schema

```json
{
  "name": "init",
  "arguments": {
    "dir": "string, required — directory to ingest"
  }
}
```

## Description

> Ingest a directory of code/SQL into the currently-open brain. Uses tree-sitter AST chunking per language (Rust / Python / JS / TS / Go / Java / C# / SQL) plus document parsing for text/markdown. Respects `.gitignore`. Idempotent — re-running skips unchanged files via BLAKE3 dedup.

## Behavior

1. Mode guard — refuse on Enterprise brains
2. Walk the directory with gitignore-aware filtering
3. Per file, dispatch to the right plugin (tree-sitter for code, document parser for docs)
4. `build_index()` + `compact()` + `save()`

## Response

```
Init complete for /code/project-x/:
  Files indexed: 487
  Frames stored: 2103
  Symbols: 1247
  Compression: 12.4×
  Elapsed: 38s
```

## When to use init vs ingest

- `init <dir>` — **full repo / archive bulk load** with tree-sitter AST for code. Idempotent — re-run is cheap.
- `ingest <file|dir>` — **ad-hoc additions** using document parsers (PDF / DOCX / …). Simpler but no AST chunking.

Typical flow: `init` once on a codebase; `ingest` for incremental docs added later.

## Enterprise refusal

Same error as CLI — refuses content embed on Enterprise brains with remediation pointing to `--pointer` (use `ingest` with `pointer=true` for per-file pointers).

## See also

- [CLI said init](../07-cli-reference/init.md)
- [code plugin](../06-ingestion-plugins/code.md)
- [docs plugin](../06-ingestion-plugins/docs.md)
