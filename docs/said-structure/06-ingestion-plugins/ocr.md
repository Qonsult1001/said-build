# OCR plugin — PaddleOCR v5 via MNN

**Feature flag:** `ocr` in [`crates/sca-core/Cargo.toml`](../../../crates/sca-core/Cargo.toml). Requires `docs` (needs pdfium for page rendering).

Entry point: [`crates/sca-core/src/ocr_ingest.rs`](../../../crates/sca-core/src/ocr_ingest.rs).

## What it does

Extracts text from **scanned / image-only PDFs** and from standalone images (PNG / JPG). When the `docs` plugin detects a PDF whose extracted text is empty or nonsensical, it routes to the OCR path:

```
PDF page → pdfium renders at 300 DPI → PNG bytes → PaddleOCR v5 → text → remember
```

Standalone image ingest also works (`said ingest image.png`).

## Dependencies

```toml
[dependencies.ocr-rs]  version = "2.2"  optional = true   # PaddleOCR v5 via MNN
[dependencies.image]   version = "0.25"  optional = true  # PNG / JPG decode only
```

ocr-rs bundles the PaddleOCR models at compile time via `include_bytes!` — so there's **no runtime model download**. MNN (Alibaba's inference framework) builds from source via cmake OR uses a prebuilt static library.

## Model bundle

Approximately **6 MB** added to the binary when `ocr` is enabled:

- Text detection model (DB net, ~3 MB)
- Text recognition model (CRNN, ~2 MB)
- Text classification model (direction detection, ~1 MB)

All bundled. The binary ships standalone.

## Pipeline

```
ocr_ingest(brain, path)
  │
  ▼
detect extension:
  .pdf  → for each page:
            pdfium.render_page(page_idx, dpi=300) → PNG bytes
            run OCR pipeline → text blocks
            concatenate with layout hints (tables, columns)
          → Vec<(page_num, ocr_text)>
  .png/.jpg → single-pass OCR → one text block
  │
  ▼
for each (page, text):
  confidence filter: drop blocks where OCR confidence < 0.80
  brain.remember_with_pillar(Some(&doc_id), text, Some(&title),
                              Pillar::Memory, tags)
  // tags: ["source:<path>", "format:pdf|png|jpg", "page:<n>",
  //        "ocr:true", "ocr_confidence:<avg>"]
```

## Confidence threshold

Default confidence floor is `0.80`. Text blocks below it are dropped (usually noise from paper edges, stamps, or photos within the PDF). Tuning:

- Raise to 0.90 for cleaner archives (filters more aggressively)
- Lower to 0.65 for damaged / faxed / photographed documents

Configurable via `ocr-rs` options; CLI / MCP don't currently expose the knob.

## Outputs

- One frame per OCR'd page / image
- Body = concatenated text blocks with layout hints (`\n` between blocks; tables as tab-separated)
- Tag `ocr:true` so admin tools can distinguish OCR'd frames from native-text PDFs
- Tag `ocr_confidence:<average>` for per-frame quality signal

## Performance

| Workload | Time |
|---|---|
| 1 page scanned PDF (A4, 300 DPI) | ~400 ms |
| 10-page scanned contract | ~4 sec |
| Standalone 2 MB PNG | ~300 ms |

OCR is ~20× slower than native PDF text extraction. For archives with mixed native + scanned PDFs, the `docs` plugin handles routing automatically — each file gets the cheapest working parser.

## How to test

Real-world test in the repo: `test_ocr.pdf` (a scanned receipt):

```
cargo run --release -p said-cli --features "static-embed docs ocr" -- \
    --path test.said ingest test_ocr.pdf

# Expected: "Ingested: test_ocr.pdf (5 pages via OCR), 12 frames stored"
# Then:
said --path test.said ask "total amount"
# Expected: finds the OCR'd amount line
```

Verified on scanned passkey + scanned API-key tests in the SAID-ECHO stress suite (April 2026) — both found at rank 1 despite the PDF being entirely image-based.

## How to extend

### Add image-only support (no PDF)
Already works for PNG / JPG. Adding TIFF / HEIC is an `image` crate feature flag update.

### Swap the OCR engine
`ocr-rs` is the abstraction; alternative backends (Tesseract, Google Cloud Vision) could live behind the same `OcrBackend` trait (not yet formalized). Tesseract's advantage: 100+ languages. PaddleOCR wins on accuracy for English / CJK at the bundled size.

### Per-language OCR
PaddleOCR v5 bundled model is English + CJK. Adding other languages = download the PaddleOCR language pack + point `ocr-rs` at it. Not compile-time today.

## Known limitations

- Only English + CJK in the default bundle
- MNN static build occasionally fails on exotic CPUs; falls back to building from source which needs cmake
- Confidence threshold isn't per-call tunable (compile-time constant)
- Image-within-PDF doesn't get per-image confidence — we average across the whole page

## See also

- [docs plugin](docs.md) — routes scanned PDFs here
- [ocr_ingest source](../../../crates/sca-core/src/ocr_ingest.rs)
