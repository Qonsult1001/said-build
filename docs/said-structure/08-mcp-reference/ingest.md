# MCP tool: ingest

Single file or folder ingest. Mirrors CLI [`said ingest`](../07-cli-reference/ingest.md).

## Schema

```json
{
  "name": "ingest",
  "arguments": {
    "path": "string, required",
    "pointer": "boolean, optional, default false",
    "summary": "string, optional (pointer-mode summary)"
  }
}
```

## Description

> Ingest a file or folder into the brain. Supports PDF, DOCX, TXT, MD, MP4/MP3 (if whisper enabled), and SQL files. Auto-detects format, runs OCR on scanned pages, streams progress. BLAKE3 dedup skips unchanged files. ENTERPRISE MODE: set pointer=true to store a URI + summary only (no blob embedded), ideal when originals live in SharePoint, S3, or a system of record. The brain is then a discovery index layer; callers fetch content at read time.

## Behavior

### Pointer mode (`pointer=true`)

1. For each file under `path`: resolve to `file://` URI
2. Call `SaidFile::remember_as_external_pointer(None, uri, mime, name, summary, vec![])`
3. `build_index()` + `save()`

### Content mode (default)

1. Mode guard — refuse on Enterprise brains with helpful error
2. Delegate to `sca_core::document_ingest::ingest_document` (requires `docs` feature)
3. Progress callback is a no-op (MCP doesn't stream progress mid-call; final report returns at the end)
4. `build_index()` + `compact()` + `save()`

## Example — content mode

```json
{"method":"tools/call","params":{"name":"ingest","arguments":{"path":"/docs/report.pdf"}}}
```

Response:
```
Ingested: /docs/report.pdf
Format: pdf
Segments: 42
Frames stored: 42
```

## Example — pointer mode

```json
{"method":"tools/call","params":{"name":"ingest","arguments":{
  "path":"/share/drive/q3.pdf",
  "pointer":true,
  "summary":"Q3 2026 report — revenue +12%, runway 18mo"
}}}
```

Response:
```
Pointer ingest complete.
Mode: enterprise (no blobs embedded)
Files: 1
Frames: 1
```

## Example — pointer folder ingest

```json
{"method":"tools/call","params":{"name":"ingest","arguments":{
  "path":"/share/drive/",
  "pointer":true
}}}
```

Walks the top level of the directory (one-deep, not recursive like CLI). Each file becomes an External pointer frame.

## Differences from CLI

- CLI walks directories recursively with gitignore filtering; MCP walks one-deep and ignores `.gitignore`. Matches the "simpler API" positioning of MCP — agents that want deep recursion use `init` instead.
- CLI has live progress streaming; MCP returns one final result.

## Enterprise error

```
This brain is in ENTERPRISE mode — content-embedding ingests are refused.
Use `--pointer` to register a searchable pointer without embedding content.
Enterprise and Portable are licensed separately and cannot be swapped...
```

Agents should respect this error and retry with `pointer=true`.

## See also

- [CLI said ingest](../07-cli-reference/ingest.md)
- [docs plugin](../06-ingestion-plugins/docs.md)
- [Row 36 External pointer mode](../05-features/row-36-external-pointer.md)
