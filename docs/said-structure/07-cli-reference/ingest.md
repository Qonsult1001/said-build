# said ingest

Single-file or folder ingest with per-format routing (PDF / DOCX / TXT / MD / MP4 / MP3 / SQL). Optional `--pointer` for Enterprise breadcrumb ingest.

## Usage

```
said [--path FILE] ingest <TARGET> [--pointer] [--summary <TEXT>]
```

## Arguments

- `<TARGET>` — file path or directory
- `--pointer` — Enterprise mode: register as External pointer, don't embed bytes
- `--summary <TEXT>` — pointer-mode summary override (else uses filename as fallback)

## Modes

### Content embed (default)

Runs the appropriate plugin pipeline:

| Extension | Plugin | Feature flag |
|---|---|---|
| `.pdf`, `.docx`, `.txt`, `.md` | [docs](../06-ingestion-plugins/docs.md) | `docs` |
| scanned PDF | [ocr](../06-ingestion-plugins/ocr.md) (auto-detect) | `ocr` |
| `.mp3`, `.mp4`, `.wav`, `.m4a`, `.flac` | [whisper](../06-ingestion-plugins/whisper.md) | `whisper` |
| `.sql` | in-tree SQL parser (custom GO-batch) | (always on) |

For folders: gitignore-aware walk, filtered to supported extensions. One frame per chunk (paragraph / symbol / transcript segment).

**Refused on Enterprise brains.** Error prompts caller to use `--pointer`.

### Pointer mode (`--pointer`)

Register a searchable pointer frame per file without embedding content:

```
said --path corp-index.said ingest /share/drive/docs/ --pointer
```

Each file becomes one frame:

```
file:///share/drive/docs/memo.pdf
mime: pdf
title: memo.pdf
summary: <--summary text or auto-generated "Pointer to memo.pdf at file:///...">
```

Tags: `pillar:external`, `external:pointer`, `mime:<type>`.

Allowed on both Portable and Enterprise brains.

## Output

```
Ingesting 142 file(s) from /share/drive/docs/
  ✓ memo.pdf (8 pages, 23 frames)
  ✓ contract.docx (12 paragraphs, 12 frames)
  ...
Ingested 142 files, 1847 frames stored.
```

Pointer mode:

```
Ingesting 142 file(s) as pointers from /share/drive/docs/
✓ Pointer ingest complete: 142 file(s) → 142 frame(s)
```

## Behavior

1. Open brain + load static encoder (needed for SCA encoding)
2. If Enterprise + NOT `--pointer` → error early
3. For each target:
   - Pointer mode → `remember_as_external_pointer` per file
   - Content mode → `document_ingest::ingest_document` (routes to docs/ocr/whisper/sql)
4. `brain.build_index()` + `brain.save()`

Every frame write audit-logs via `remember_with_pillar`.

## Examples

```bash
# Single file
said ingest report.pdf

# Folder (PDFs + DOCX + TXT)
said ingest /path/to/legal-archive/

# Pointer ingest (enterprise)
said --path corp.said ingest /share/sharepoint/ --pointer

# Pointer with custom summary
said ingest api-docs.pdf --pointer --summary "API reference for v2.3"

# Video transcription (requires whisper feature)
said ingest meeting.mp4
```

## Performance

Real-world numbers from the SAID-ECHO test suite:

| Workload | Time |
|---|---|
| 500-page PDF (native text) | ~35 sec |
| 500-page PDF (OCR'd) | ~3 min 20 sec |
| 1-hour video (Whisper small, CPU) | ~4 min |
| 1-hour video (Whisper small, DirectML GPU) | ~1 min |
| 1000-file legal archive (DOCX + PDF mix) | ~10 min |

## Matching MCP tool

[MCP `ingest` tool](../08-mcp-reference/ingest.md) — same `pointer` + `summary` params.

## See also

- [docs plugin](../06-ingestion-plugins/docs.md)
- [ocr plugin](../06-ingestion-plugins/ocr.md)
- [whisper plugin](../06-ingestion-plugins/whisper.md)
- [Row 36 Enterprise pointer mode](../05-features/row-36-external-pointer.md)
- [Row 37 Brain mode](../05-features/row-37-brain-mode.md)
