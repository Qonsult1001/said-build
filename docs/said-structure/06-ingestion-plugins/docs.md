# docs plugin — PDF / DOCX / TXT / MD

**Feature flag:** `docs` in [`crates/sca-core/Cargo.toml`](../../../crates/sca-core/Cargo.toml).

Entry point: [`crates/sca-core/src/document_ingest.rs`](../../../crates/sca-core/src/document_ingest.rs) — one public `ingest_document(brain, path, progress_cb)` function that detects format and routes.

## Supported formats

| Extension | Parser | Notes |
|---|---|---|
| `.pdf` | **pdfium** (layout-aware) → fallback **pdf-extract** (pure Rust) | pdfium handles multi-column / tables correctly; pdf-extract is the no-external-dep fallback |
| `.docx` | **quick-xml** + **zip** | DOCX is a zip of XML; we unzip, parse the body XML, extract paragraph text |
| `.txt` | std `fs::read_to_string` | UTF-8 lossy; preserves line structure |
| `.md` | std `fs::read_to_string` | Treated like TXT — markdown structure preserved in body |

## Dependencies

From Cargo:

```toml
[dependencies.pdf-extract]  version = "0.10"  optional = true
[dependencies.pdfium]       version = "0.10"  optional = true   # needs pdfium.dll / .so at runtime
[dependencies.quick-xml]    version = "0.31"  optional = true
[dependencies.zip]          version = "0.6"   optional = true   # default-features = false, deflate only
```

pdfium binds a Google-maintained PDF library. The `pdfium.dll` / `.so` must be installed alongside the binary; auto-fallback to `pdf-extract` kicks in if the DLL is missing, so `.said` keeps working.

## Pipeline per file

```
ingest_document(brain, path)
  │
  ▼
detect extension → choose parser
  │
  ▼
parser extracts structured text:
  - PDF  → Vec<(page_number, text)> preserving column order
  - DOCX → Vec<(paragraph_index, text)>
  - TXT/MD → full text as one segment
  │
  ▼
for each segment:
  BLAKE3 dedup check against existing frames (skip if unchanged)
  chunk into paragraphs (split on "\n\n" or length cap)
  for each chunk:
    brain.remember_with_pillar(Some(&doc_id), chunk, Some(&title), Pillar::Memory, tags)
    // ↑ current behavior — doc ingest frames land as Pillar::Memory
    //   because remember_with_pillar + set_pillar do the right thing
    // tags: ["source:<path>", "format:<ext>", "segment:<n>", ...]
  │
  ▼
brain.build_index()     # SCA + trigram refresh
brain.compact()         # block-compress
```

## Progress callback

Each extracted segment fires a `progress_cb(segment_index, total_segments, message)`. The CLI uses this to draw a live status line:

```
Page 42/180 (PDF: invoice_q3.pdf)
Paragraph 12/48 (DOCX: merger.docx)
```

## Outputs

- One frame per paragraph / chunk (typical DOCX page → 10-30 frames; typical PDF page → 5-20 frames)
- Tags: `source:<path>`, `format:pdf|docx|txt|md`, `page:<n>` (PDFs), `segment:<n>`
- `FrameMeta.pillar` = `Pillar::Memory` for legacy reasons (scheduled migration to `Pillar::External` or `Pillar::Semantic` — see [Known limitations](../11-known-limitations.md))

## Performance

Observed on real corpora:

| Format | Throughput |
|---|---|
| PDF (pdfium, text-heavy) | ~15 pages/sec |
| PDF (pdf-extract fallback) | ~40 pages/sec |
| DOCX | ~100 paragraphs/sec |
| TXT / MD | disk-bound |

Ingesting a 500-page PDF takes 30-40 seconds; a 1000-file docs directory takes ~5 minutes on a modern laptop.

## Encryption

When the `encryption` feature is also enabled, each frame's compressed bytes get AES-256-GCM wrapped. Key management is out-of-band (admin sets the key; `.said` stores the nonce). See [Known limitations](../11-known-limitations.md) — key management is manual today.

## How to test

```
cargo run --release -p said-cli --features "static-embed docs" -- \
    --path test.said init /path/to/docs/
```

Expected: progress line per file; final "Ingested N files, M frames" summary; `said ask "…"` returns relevant hits.

On `willie.said` (the test corpus in the repo): 19,149 frames across ~4000 legal documents (PDFs + DOCX). MTEB-class retrieval works at full quality across the whole corpus.

## How to extend

### Add a new format (e.g. EPUB, HTML)
1. Add the parser crate as an optional dep in Cargo
2. Extend the feature declaration (`docs = [..., "dep:epub-parser"]`)
3. Add a handler in `document_ingest.rs::detect_and_route`
4. Emit `Vec<(segment_index, text)>` like the existing parsers
5. Test with a real-world sample

### Pointer mode for the docs plugin
Already ships — `said ingest <path> --pointer` skips the full parse + chunk pipeline and writes a single External pointer frame per file with the summary instead. See [Row 36](../05-features/row-36-external-pointer.md).

## Known limitations

- Doc ingest frames land in `Pillar::Memory`. Should migrate to `Pillar::External` (for Enterprise) or `Pillar::Semantic` (for distilled chunks) when the pillar-persistence sweep completes.
- pdfium DLL needs manual install on Windows; auto-fallback hides it but silently uses the slower pure-Rust parser.
- OCR for scanned PDFs is a separate plugin — see [ocr](ocr.md).
- No EPUB / HTML / CSV / PowerPoint / Excel support yet (Excel is a plausible enterprise ask — future plugin work).

## See also

- [OCR plugin](ocr.md) — scanned PDF → text before docs plugin consumes it
- [Row 36 External pointer mode](../05-features/row-36-external-pointer.md) — pointer ingest alternative
- [document_ingest source](../../../crates/sca-core/src/document_ingest.rs)
