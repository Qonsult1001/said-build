# Cargo feature flags

`sca-core` ships with no default features — every optional dependency must be turned on explicitly. This keeps the library portable (a plain `cargo add sca-core` builds in seconds on any platform) and lets downstream consumers opt into only what they need.

Source of truth: [`crates/sca-core/Cargo.toml`](../../crates/sca-core/Cargo.toml).

## Recommended build recipes

| Goal | Command |
|------|---------|
| Minimal library (no ingest, no embedder) | `cargo build -p sca-core` |
| CLI with static encoder baked in | `cargo build --release -p said-cli --features "static-embed embed-model"` |
| Full shipping CLI | `cargo build --release -p said-cli --features "static-embed embed-model docs ocr whisper code"` |
| MCP server for IDEs | `cargo build --release -p said-mcp --features "static-embed embed-model docs ocr whisper code"` |
| Benchmarks only | `cargo build -p sca-core --features "static-embed" --examples` |

## Full feature list

### `static-embed` — static encoder
Pulls in `model2vec-rs`. Enables `brain.load_encoder(path)` so the brain can turn text into 64-bit fingerprints without calling a transformer. This is the default encoder used by every shipping build.

### `embed-model` — bake the encoder in
Implies `static-embed`. Compiles `said-lam-static/model.safetensors` (3.9 MB) and `said-lam-static/tokenizer.json` (952 KB) directly into the binary via `include_bytes!`. Binary size: +4.8 MB. Runtime: no external files needed. See [3.2 static encoder](03-core-subsystems/3.2-static-encoder.md).

### `simd` — SIMD Hamming
Pulls in `simsimd`. Accelerates the 64-bit Hamming distance core that SCA top-50 retrieval lives and dies by. Typical speedup on x86-64 with AVX2: 3-4×. No-op on targets without SIMD.

### `gpu` — wgpu compute shader
Pulls in `wgpu` + `pollster`. Runs the Hamming search as a compute shader on any GPU that speaks Vulkan / Metal / DX12. Kicks in above ~10k frames; below that the CPU path wins.

### `bert` — BERT tokenizer
Pulls in `tokenizers`. Enables fine-grained BERT-level tokenization for the trigram index. Only useful when ingesting multi-lingual corpora or running token-level overlap scoring; the default whitespace/punctuation tokenizer handles code + English cleanly.

### `encryption` — per-frame AES-256-GCM
Pulls in `aes-gcm`. Encrypts frame payloads at rest. Key lives in the user's OS keychain; brain still searches fingerprints in the clear (the fingerprint is 64 bits — leaking nothing recoverable about the content). Used for Enterprise legal archives.

### `mmap` — memory-mapped blocks
Pure feature flag (no deps). Enables the `memmap2`-backed zero-copy block reads. On by default in CLI release builds; off in tests (tempfile cleanup is cleaner without mmaps holding handles).

### `code` — tree-sitter AST chunking
Pulls in `tree-sitter` + 7 grammars: Rust, Python, JavaScript, TypeScript, Go, Java, C#. Enables AST-aware chunking for `said init` — one chunk per function/class/struct/etc. with exact line ranges, so the trigram + symbol index gets precise hit locations. See [06-ingestion-plugins/code.md](06-ingestion-plugins/code.md).

**Note on SQL**: SQL does NOT use tree-sitter. The SQL plugin uses a custom GO-batch parser that tracks `CREATE TABLE / CREATE PROCEDURE / CREATE TRIGGER / CREATE VIEW / CREATE FUNCTION` statements directly. See `sql_chunk` module.

### `lsp` — language-server client
Pulls in `lsp-types`. Adds `said lsp-def`, `said lsp-refs`, `said lsp-hover` commands that talk to `rust-analyzer`, `tsserver`, `pyright` via JSON-RPC and cache results as `Code` pillar frames. See [06-ingestion-plugins/lsp.md](06-ingestion-plugins/lsp.md).

### `docs` — document ingestion
Pulls in `pdf-extract` (pure-Rust text), `pdfium` (layout-aware, multi-column, C++ via pdfium.dll), `quick-xml` (DOCX parse), `zip` (DOCX unpack). Two-tier PDF extraction: pdfium first (if the DLL is reachable at runtime), fallback to pdf-extract. Scanned-image PDFs skipped unless `ocr` is also on. See [06-ingestion-plugins/docs.md](06-ingestion-plugins/docs.md).

### `ocr` — scanned-image PDFs
Pulls in `ocr-rs` (PaddleOCR v5 via MNN) + `image`. Requires `docs` (for page rendering). Bundles ~6 MB of model weights into the binary. Windows / macOS / Linux x86-64 supported; ARM builds from source.

### `whisper` — audio transcription
Pulls in `sherpa-rs` (prebuilt binaries, no LLVM needed — this is the reason we don't use `whisper.cpp` directly) + `symphonia` (decode MP3 / MP4 / AAC / WAV). Sherpa backs: Whisper, Moonshine, SenseVoice.

### `directml` — Whisper on any Windows GPU
Pulls in `sherpa-rs/directml`. Routes whisper inference through DirectML so any DirectX 12-capable Windows GPU accelerates it — no CUDA, no vendor lock-in. On macOS / Linux, `whisper` uses CPU.

### `python` — PyO3 bindings
Pulls in `pyo3` with `extension-module`. Produces a `cdylib` that can be imported from Python as `sca_core`. Used by the Python MTEB harness + the original research notebooks.

## Binary layout by feature

| Feature set | `said` CLI size (Windows release) |
|-------------|-----------------------------------|
| none (stub) | ~5 MB |
| `static-embed embed-model` | ~11 MB (+4.8 MB model) |
| `+ code` | ~22 MB (+ 7 tree-sitter grammars) |
| `+ docs + ocr` | ~35 MB (+ pdfium + PaddleOCR models) |
| `+ whisper + directml` | ~70 MB (+ sherpa-onnx DirectML binaries) |
| Full (`static-embed embed-model docs ocr whisper code`) | ~70 MB |

Target for the canonical release: ≤ 80 MB. The `embed-model` + `whisper` pair dominates the footprint.

## Implicit feature dependencies

Compile-time invariants encoded in `Cargo.toml`:

```toml
embed-model = ["static-embed"]           # can't embed without loading
ocr         = ["ocr-rs", "image", "docs"] # OCR needs pdfium page rendering
directml    = ["sherpa-rs/directml"]     # must have sherpa first
```

Attempting `--features directml` without `whisper` fails at `cargo resolve` time, not at runtime.

## Features by consumer crate

- `said-cli` — all CLI features typically passthrough from `sca-core`.
- `said-mcp` — same as `said-cli`, plus its own `tokio` features for async JSON-RPC. MCP always needs `static-embed embed-model` at minimum; shipping builds ship everything.
- `sca-core` (as a library) — ship whatever the consumer wants; no mandatory features.

### `forge` — spec-driven workspace generator (said-cli + said-mcp only)

**Unlike the other features on this page, `forge` is NOT an `sca-core` feature.** It's a separate crate (`said-forge`) that `said-cli` and `said-mcp` pull in as an optional dependency:

```toml
# crates/said-cli/Cargo.toml
said-forge = { path = "../said-forge", optional = true }
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros"], optional = true }

[features]
forge = ["dep:said-forge", "dep:tokio"]

# crates/said-mcp/Cargo.toml
said-forge = { path = "../said-forge", optional = true }

[features]
forge = ["dep:said-forge"]
```

Enabling `forge`:
- on `said-cli`: adds `said forge <load|list|show|status|run|reset>` subcommands
- on `said-mcp`: adds 6 `forge_*` MCP tools (25 baseline → 31 total)

`said-forge` itself depends on `sca-core` (with `static-embed`) and brings in its own deps: `reqwest` (HTTPS directive fetch), `serde_yaml` (OpenAPI YAML), `async-trait`, `thiserror`. See [`crates/said-forge/Cargo.toml`](../../crates/said-forge/Cargo.toml).

Test-only feature: `stub-llm` on `said-forge` exposes `llm::stub::StubProvider` to integration tests (which run as a separate crate and don't see the lib's `cfg(test)`).

See [`05-features/forge.md`](05-features/forge.md) for what the feature actually does.

## See also

- [3.2 Static encoder](03-core-subsystems/3.2-static-encoder.md) — what `static-embed` brings in
- [Ingestion plugins](06-ingestion-plugins/README.md) — what `docs`/`ocr`/`whisper`/`code`/`lsp` each enable
- [`Cargo.toml`](../../crates/sca-core/Cargo.toml) — source of truth
