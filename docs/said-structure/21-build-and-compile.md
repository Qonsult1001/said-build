# Build & compile — feature bundles for CLI and MCP (READ BEFORE BUILDING)

The single source of truth for **how to compile every shipped variant** of `said` (CLI) and `said-mcp`
(MCP server). Getting the feature flags wrong does not error — it ships a binary that **silently runs
degraded** and looks like a recall bug. This doc exists so we never again "fix a bug" that is actually a
build mistake. (See FIXES-LOG #5: an MCP server built without `embed-model` made every `recall_fix`
return "No known fix" while the CLI recalled the same fix at 0.82.)

## THE ONE RULE: every build that does semantic recall MUST include `embed-model`

`embed-model` bakes the said-lam static encoder into the binary. Without it:

- `build_index` produces **0 SCA fingerprints** (semantic recall dead),
- `ask` / `search` fall back to symbol + grep only (no `[semantic]` hits),
- `recall_fix` can NEVER clear its 0.45 score floor (semantic=0 caps the score at 0.30) → always
  "No known fix",
- and **none of this errors** — it just silently under-performs.

### The asymmetry that bit us (now fixed at the source)

Originally `said-mcp`'s default was `["docs"]` — **no embed-model** — so a bare `cargo build -p said-mcp`
shipped an encoder-less server that silently ran symbol+grep only (FIXES-LOG #5). Both defaults now
include the encoder:

| Binary | `default` features | Safe to build with no `--features`? |
|---|---|---|
| **`said` (CLI)** | `["embed-model"]` | ✅ yes |
| **`said-mcp`** | `["embed-model", "docs"]` | ✅ yes (fixed — was `["docs"]`) |

Two belt-and-braces guards remain so this can never silently recur: (1) the defaults bake the encoder,
and (2) the server prints a loud one-time stderr WARNING if it ever starts with no encoder. **Still
prefer an explicit bundle** (`--no-default-features --features "coding"`) for ship builds so the variant
is deterministic and minimal — don't rely on defaults for releases.

## Shipping bundles (identical names for CLI and MCP)

Both crates define the same four bundles; each one includes `embed-model`:

| Bundle | Features | What it's for |
|---|---|---|
| `brain` | `embed-model` | portable semantic memory over text only (no code intel) |
| `coding` | `embed-model`, `code` | brain + code intelligence (24 langs via tree-sitter) — the common dev build |
| `coding-plus` | `embed-model`, `code`, `lsp` | coding + LSP cross-file intelligence |
| `full` | `embed-model`, `code`, `docs`, `ocr`, `lsp` | everything: code + PDF/DOCX ingest + OCR + LSP |

CLI also has `release = [code, docs, ocr, whisper, embed-model, lsp, forge, forge-xlsx, forge-docs]`
(the ship-ready developer binary) and the `forge*` family (orchestration/SQL).

## Canonical build commands

Always `--no-default-features` + an explicit bundle, so the build is deterministic regardless of which
crate's defaults differ:

```bash
# CLI — coding bundle (most common)
cargo build --release -p said-cli --no-default-features --features "coding"
#   produces target/release/said.exe   (we copy it to said-coding.exe)

# MCP — coding bundle (MUST include embed-model — `coding` does)
cargo build --release -p said-mcp --no-default-features --features "coding"
#   produces target/release/said-mcp.exe

# full (PDF/DOCX/OCR/LSP) — either crate
cargo build --release -p said-cli --no-default-features --features "full"
cargo build --release -p said-mcp --no-default-features --features "full"

# browser-history ingest (native-only, bundled SQLite) — ADD `browser` to the bundle
cargo build --release -p said-cli --no-default-features --features "coding,browser"
```

### Feature → capability map (what each flag pulls in)

| Feature | Pulls | Gives you | Notes |
|---|---|---|---|
| `embed-model` | said-lam static encoder (baked, ~4.8MB) | semantic recall, fingerprints, `recall_fix` | **required for any recall**; implies `static-embed` |
| `code` | tree-sitter (24 grammars) | AST code chunking, `sym`, code-graph | |
| `lsp` | lsp-types | LSP cross-file references | |
| `docs` | pdf-extract, pdfium, quick-xml, zip | PDF/DOCX/TXT/MD ingest | native libs |
| `ocr` | ocr-rs, image (+ docs) | OCR for scanned PDFs | implies `docs` |
| `whisper` | sherpa-rs, symphonia | audio/video transcription ingest | native, heavy |
| `browser` | rusqlite (bundled SQLite) | Chrome/Edge history → external pointers | **native-only, OFF the WASM path** |
| `encryption` | aes-gcm | per-frame AES-256-GCM | |
| `pack-sign` | ed25519-dalek | signed skill packs | |

## Windows / this machine build environment (do not skip)

The C: drive is full, so rustc's temp must be redirected or the build fails mid-compile:

```bash
export TMP="G:\\cargo-tmp" TEMP="G:\\cargo-tmp"     # rustc scratch (C: is full)
export CARGO_PROFILE_RELEASE_LTO=false              # fast builds (skip LTO)
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
export PATH="/c/Users/Carter/.cargo/bin:$PATH"
```

`browser`/`full` compile bundled SQLite / pdfium from source — first build is +50–90s; expected, not a hang.

## Post-build VERIFICATION (catch a mis-build before it wastes a session)

A build with the wrong features compiles fine and fails silently at runtime. Always smoke-test recall:

```bash
# CLI: must print the encoder line and return a [semantic] hit
said-coding.exe --path some.said ask "anything descriptive" --top 3   # expect a [semantic] result
# (stderr should show: [SCA] Loaded embedded model (zero external files))

# MCP: initialize must NOT print the no-encoder WARNING
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05",\
"capabilities":{},"clientInfo":{"name":"t","version":"1"}}}\n' \
  | said-mcp.exe --path some.said 2>&1 >/dev/null | grep -i WARNING   # expect NO output
```

If `ask` shows only `[symbol]`/`[text]` and never `[semantic]`, or the MCP prints the WARNING → the
binary lacks `embed-model`. Rebuild with a bundle above. Do NOT start debugging recall — fix the build.

## Binary-copy race (known gotcha)

Building a binary then immediately running a regression that calls it can hit a 0/N result that recovers
on retry (the OS still has the old image mapped). After `cp target/release/said.exe said-coding.exe`,
`sleep 2–3` before driving it, or re-run once.

## The drift checklist (when adding a feature)

1. Add the feature to `sca-core/Cargo.toml` (its own `optional = true` deps) — gate native-only ones
   OFF the WASM path (see architecture-rust §Constitution).
2. Re-export it through `said-cli` AND `said-mcp` `[features]`, and add it to the relevant bundles.
3. **Keep CLI/MCP bundle names identical** (`brain`/`coding`/`coding-plus`/`full`) so a feature can't be
   in one surface and missing in the other.
4. If it touches recall, confirm the bundle still includes `embed-model`.
5. Run the post-build verification above for BOTH binaries.
