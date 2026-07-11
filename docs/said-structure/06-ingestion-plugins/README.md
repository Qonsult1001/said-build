# Ingestion plugins

Feature-gated modules that extend `.said` to ingest formats beyond plain text. Each is opt-in — the core binary stays small; Cargo features pull in the heavier deps only when needed.

## Contents

- [docs](docs.md) — PDF / DOCX / TXT / MD via pdfium, pdf-extract, quick-xml, zip
- [OCR](ocr.md) — PaddleOCR v5 via MNN; models bundled (~6 MB)
- [whisper](whisper.md) — audio / video transcription via sherpa-rs with DirectML acceleration
- [code](code.md) — AST-chunked source via tree-sitter (7 languages)
- [LSP](lsp.md) — language-server integration (rust-analyzer / tsserver / pyright) for cross-file refs
- [personal-import](personal-import.md) — **FREE-tier** `said import <browser|email|chatgpt|claude>`: pull the user's own personal data (browser history, mail, AI-chat exports) into memories. Distinct from code-tier `init`/`ingest` — a *memory* feature (`import` verb, External/Episodic pillar), not code intelligence.

## Which feature pulls in which plugin

```toml
# Cargo.toml defaults (crates/sca-core/Cargo.toml)
[features]
default = []
static-embed = ["dep:model2vec-rs"]   # always wanted; small
embed-model  = ["static-embed"]        # bundles encoder bytes (+4.8 MB)
docs         = ["dep:pdf-extract", "dep:quick-xml", "dep:zip", "dep:pdfium"]
ocr          = ["dep:ocr-rs", "dep:image", "docs"]      # OCR needs pdfium for page rendering
whisper      = ["dep:sherpa-rs", "dep:symphonia"]
directml     = ["sherpa-rs/directml"]                   # Windows GPU acceleration
code         = ["dep:tree-sitter", "dep:tree-sitter-rust", ... /* 7 langs */]
lsp          = ["dep:lsp-types"]
browser      = ["dep:rusqlite"]   # personal-import: Chrome/Edge History SQLite → External-pointer memories (see personal-import.md). NATIVE-only.
gpu          = ["dep:wgpu", "dep:pollster"]             # GPU Hamming search
encryption   = ["dep:aes-gcm"]
mmap         = []
python       = ["dep:pyo3"]                             # Python bindings via PyO3
bert         = ["dep:tokenizers"]
simd         = ["dep:simsimd"]
```

See [Cargo feature flags](../09-cargo-features.md) for the full enumeration.

## Which binary ships which plugin

- **`said-cli`** — default + `static-embed` + `docs` + `ocr` + `whisper` + `code` typically enabled on release builds
- **`said-mcp`** — same features, so any ingest tool (`ingest`, `init`) that lands in MCP works the same as CLI

## Writer pattern

Every ingestion plugin hits the frame store through one of:

- `SaidFile::remember_with_pillar(…, pillar, tags)` — preferred; pillar routing + audit hook
- `SaidFile::put_with_pillar(opts, pillar)` — for lower-level callers
- `FrameStore::put_with(opts)` — direct; legacy, doesn't set pillar byte correctly for Code/Procedural/External (see [Known limitations](../11-known-limitations.md))

Current plugins still use the third form for bulk ingestion, which is a migration target but not a behavioral bug (tags preserve the pillar discriminator).

## Enterprise mode

Any content-embedding ingest (every plugin here except pointer mode of the `docs` plugin) is refused by Enterprise brains. Users on Enterprise must use `said ingest --pointer` or the plugin's pointer equivalent.

See [Row 37](../05-features/row-37-brain-mode.md).
