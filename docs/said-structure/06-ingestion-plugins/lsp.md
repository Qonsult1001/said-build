# LSP plugin — cross-file code intelligence

**Feature flag:** `lsp` in [`crates/sca-core/Cargo.toml`](../../../crates/sca-core/Cargo.toml).

Entry point: [`crates/sca-core/src/lsp_client.rs`](../../../crates/sca-core/src/lsp_client.rs).

## What it does

Attaches to a running **language server** (rust-analyzer, tsserver, pyright, gopls, …) and asks it for cross-file facts that tree-sitter alone can't resolve:

- "Goes to definition of symbol X"
- "Find all references to symbol X"
- "Workspace symbols matching query"
- Hover info (type signatures, doc comments)

The LSP's answers get surfaced through `said sym` / `said ask` and (eventually) persisted in the `REFS` section of the `.said` file so cross-file ref queries work offline without re-running the LSP.

## Dependencies

```toml
[dependencies.lsp-types]  version = "0.97"  optional = true
[dependencies.serde_json] version = "1"
```

`lsp-types` is the LSP protocol schema. `serde_json` is already a hard dep.

## Status

**Partially shipped.** The `LspClient` struct exists; it can spawn a language server, send requests, parse responses. The `REFS` section reservation exists in the v7_1 header. What doesn't exist yet:

- Persistence — LSP-discovered refs aren't written to the `REFS` section
- MCP tool surface for LSP queries
- Stable per-language server configurations (rust-analyzer works; others are TBD)

## Planned pipeline

```
said init <dir> --lsp     # opt-in flag; requires `lsp` feature
  │
  ├─ code plugin normal pipeline — tree-sitter chunks + symbol index
  └─ for each language detected:
       spawn language server (rust-analyzer for .rs, pyright for .py, ...)
       for each indexed symbol:
         lsp.find_references(symbol) → Vec<Location>
         persist refs into REFS section
       shutdown language server
```

Query-time cross-file refs would then be served from `REFS` without re-running the LSP — instant, offline, frozen at init time.

## Current Rust API

```rust
#[cfg(feature = "lsp")]
pub struct LspClient { /* ... */ }

impl LspClient {
    pub async fn new(server_cmd: &str, workspace_root: &Path) -> Result<Self, Error>;
    pub async fn find_references(&self, symbol: &str, position: Position)
        -> Result<Vec<Location>, Error>;
    pub async fn goto_definition(&self, position: Position)
        -> Result<Option<Location>, Error>;
    pub async fn workspace_symbol(&self, query: &str)
        -> Result<Vec<SymbolInformation>, Error>;
}
```

Lives off the main thread (LSP needs an async runtime). Communication is JSON-RPC over stdio with the language server subprocess.

## How to test (current — partial)

```rust
#[cfg(feature = "lsp")]
async fn test_rust_analyzer() {
    let client = LspClient::new("rust-analyzer", Path::new("/path/to/repo")).await?;
    let refs = client.find_references("FrameStore", Position { line: 325, character: 11 }).await?;
    assert!(!refs.is_empty());
}
```

No integration test in tree today. Manual verification against `rust-analyzer` passes for basic ref queries.

## How to extend

### Persist refs into `REFS` section
The refs section has a reserved offset in the header (v7_1) but isn't populated. Implementation path:

1. Walk `SymbolIndex::all_entries()`
2. For each symbol, call `lsp.find_references`
3. Collect `(source_doc_id, target_doc_id, kind)` tuples
4. Serialize to `REFS` section (magic `"REFS"`, format TBD)
5. Read back on open into an in-memory edge list
6. Expose via `brain.find_references(symbol)` — no LSP needed at query time

### Per-language server configuration
Different LSPs expect different init params. Current client is rust-analyzer-shaped. Generalizing:

```rust
pub struct LspConfig {
    pub command: String,          // "rust-analyzer" / "pyright" / ...
    pub file_extensions: Vec<&'static str>,
    pub init_params: serde_json::Value,
}
```

Keep a built-in registry of known-good configs.

### Incremental updates
LSP servers can notify on file changes. A `said watch` daemon could keep the `REFS` section fresh. Not shipped; see [Roadmap](../12-roadmap.md).

## Known limitations

- LSP is async (requires tokio); the rest of `sca-core` is sync. Keeps the feature cleanly gated but adds a dep pull when enabled.
- Language server startup is slow (~2-5 sec for rust-analyzer on a large repo). Init-time LSP is feasible; per-query LSP isn't.
- No refs persistence today; every LSP query re-runs the server.
- No test coverage in CI for the LSP path.

## See also

- [code plugin](code.md) — the tree-sitter layer that LSP enhances
- [3.6 Trigram + symbol index](../03-core-subsystems/3.6-trigram-symbol-index.md)
- [2.2 Sections — REFS](../02-file-format/2.2-sections.md#refs-reference-edges)
