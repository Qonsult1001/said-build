# Pillar — Code

AST-chunked source code. Integrates with the symbol index + tree-sitter + optional LSP client for full code intelligence inside a `.said` file.

## What goes here

- Source code chunks (functions, methods, classes, modules)
- AST-identified boundaries (tree-sitter per-language grammar)
- Imported via `said init <repo>` or explicit `remember_as_code`

## Writer APIs

```rust
brain.remember_as_code(
    Some("code_parse"),
    "rust",                                              // language
    "fn parse_header(bytes: &[u8]) -> Result<Header, String> { /* ... */ }",
    Some("parse_header"),                                // symbol name
    Some("crates/sca-core/src/said_file.rs"),            // source path
    vec![],                                              // extra tags
);
```

Body layout (indexed + searchable):
```
[rust] parse_header (crates/sca-core/src/said_file.rs)
fn parse_header(bytes: &[u8]) -> Result<Header, String> { /* ... */ }
```

Tags auto-applied:
- `pillar:code`
- `lang:<language>` — lowercased
- `symbol:<name>` — when provided
- `source:<path>` — when provided

CLI (the actual daily use):
```
said init <dir>            # walks dir, AST-chunks every code file, writes Code frames
said add --dir <path>      # similar but lighter
said ingest <file>         # single-file ingest
```

## Supported languages

Via tree-sitter grammars (see [code plugin](../06-ingestion-plugins/code.md)):

| Language | tree-sitter crate | Symbol kinds detected |
|---|---|---|
| Rust | tree-sitter-rust | fn, struct, enum, trait, impl, const, type |
| Python | tree-sitter-python | class, method, function, const |
| JavaScript | tree-sitter-javascript | function, class, method, arrow function |
| TypeScript | tree-sitter-typescript | function, class, interface, type, method |
| Go | tree-sitter-go | func, method, struct, interface, const |
| Java | tree-sitter-java | class, interface, method, field |
| C# | tree-sitter-c-sharp | class, interface, method, property, field |

## Symbol index integration

Every Code frame's AST-chunked symbol goes through `brain.record_symbol(name, doc_id, kind, start_line, end_line)`. At compact time the pending symbols fold into `SymbolIndex` and serialize to the `SYMS` section.

At query time `said sym <name>` is sub-millisecond — pure HashMap lookup — and returns full location:

```
said sym FrameStore
FrameStore    crates/sca-core/src/frames.rs::FrameStore (struct:325-354)
```

## Retrieval ranking

Architecture spec:
```
score = standard SCA + sym + grep   (unchanged — already good for code)
```

**Current reality** — exactly as specified. Code frames participate in the full 3-engine ask fusion:
- **Sym** exact match → confidence 1.00
- **Grep** keyword match → confidence 0.40-0.95
- **SCA semantic** → confidence 0.30-0.80

No per-pillar reranking needed; the code-intent query router already prefers PureLexical mode for code queries, which weights BM25 and exact symbol matches heavily.

## LSP integration (optional)

With the `lsp` Cargo feature, a language server (rust-analyzer, tsserver, pyright, ...) can be attached on-the-fly for deeper code intelligence:

```rust
#[cfg(feature = "lsp")]
brain.enable_lsp_for_path("/path/to/repo").await?;

let references = brain.find_references("FrameStore")?;
// returns cross-file ref edges discovered by LSP
```

Currently the LSP feature is gated behind its Cargo feature and adds `lsp-types` + async runtime. References aren't yet persisted in REFS section (reserved slot in the header; not populated).

## Typical queries

```
said ask "where is parse_header defined?"             # sym hit, confidence 1.00
said ask "how does compact block dict work?"          # multi-word query, symbol candidate generation catches `compact_block_dict`
said sym FrameStore                                   # exact symbol lookup, <1 ms
said search pillar=code lang:rust query="trait impl"  # scoped
```

## How to test

```rust
let fid = sf.remember_as_code(
    Some("code_test"),
    "rust",
    "pub fn hello() -> &'static str { \"hi\" }",
    Some("hello"),
    Some("test.rs"),
    vec![],
);
sf.build_index()?;

let meta = sf.frames.get_meta("code_test").unwrap();
assert_eq!(meta.pillar, Pillar::Code);
assert!(meta.tags.iter().any(|t| t == "lang:rust"));
assert!(meta.tags.iter().any(|t| t == "symbol:hello"));
```

Functional verification in [`examples/pillar_writers_probe.rs`](../../../crates/sca-core/examples/pillar_writers_probe.rs).

## See also

- [Row 47 Code writer](../05-features/row-47-code.md)
- [3.6 Trigram + symbol index](../03-core-subsystems/3.6-trigram-symbol-index.md)
- [Code ingestion plugin](../06-ingestion-plugins/code.md)
