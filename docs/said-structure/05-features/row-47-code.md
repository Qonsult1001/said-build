# Row 47 — Code pillar writer

**Status:** ✅ shipped 2026-04-22

## What it does

`SaidFile::remember_as_code(language, source, symbol, source_path, tags)` writes a code-chunk frame into the Code pillar. Integrates with the existing symbol index (`said sym`) + tree-sitter AST chunking + optional LSP client.

## Where it lives

[`SaidFile::remember_as_code`](../../../crates/sca-core/src/said_file.rs). Routes through `remember_with_pillar(Pillar::Code, ...)` which uses `FrameStore::set_pillar` so persisted pillar is correct.

## Inputs

```rust
pub fn remember_as_code(
    &mut self,
    doc_id: Option<&str>,
    language: &str,
    source: &str,
    symbol: Option<&str>,
    source_path: Option<&str>,
    extra_tags: Vec<String>,
) -> u64;
```

Example:
```rust
brain.remember_as_code(
    Some("code_parse"),
    "rust",
    "fn parse_header(bytes: &[u8]) -> Result<Header, String> { /* ... */ }",
    Some("parse_header"),
    Some("crates/sca-core/src/said_file.rs"),
    vec![],
);
```

## Outputs

Body format:
```
[rust] parse_header (crates/sca-core/src/said_file.rs)
fn parse_header(bytes: &[u8]) -> Result<Header, String> { /* ... */ }
```

Tags:
- `pillar:code`
- `lang:<language>` (lowercased)
- `symbol:<name>` (when provided)
- `source:<path>` (when provided)
- Caller-supplied tags

Title: symbol if provided, else source_path.

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
assert!(meta.tags.iter().any(|t| t == "pillar:code"));
assert!(meta.tags.iter().any(|t| t == "lang:rust"));
assert!(meta.tags.iter().any(|t| t == "symbol:hello"));
```

Verified in [`examples/pillar_writers_probe.rs`](../../../crates/sca-core/examples/pillar_writers_probe.rs):

```
=== Code writer ===
  frame_id=1 pillar=Code tags=["lang:rust", "symbol:parse_header",
                               "source:crates/sca-core/src/said_file.rs", "pillar:code"]
```

## Integration with symbol index

`remember_as_code` does NOT automatically call `record_symbol`. The caller is responsible for that if they want `said sym <name>` lookup. The full integration:

```rust
brain.remember_as_code(
    Some("code_parse"),
    "rust",
    source_code,
    Some("parse_header"),
    Some("crates/sca-core/src/said_file.rs"),
    vec![],
);
brain.record_symbol(
    "parse_header",
    "code_parse",
    SymbolKind::Fn,
    start_line,
    end_line,
);
// After compact, `said sym parse_header` finds it.
```

`said init <dir>` (the tree-sitter-driven directory walker) handles both steps automatically.

## How to extend

New language = add a tree-sitter grammar and update the per-language chunker in [`code_search.rs`](../../../crates/sca-core/src/code_search.rs). See [code plugin](../06-ingestion-plugins/code.md).

New language-aware search boost: in `recall_fused`, read the `lang:` tag and boost matches when the query's code-intent matches. Low-risk — purely ranking tweak.

## Known limitations

- Direct `said init` ingestion still uses `put_with` (not `put_with_pillar`) and lands as `Pillar::Memory`. Pillar persistence sweep tracked in [Known limitations](../11-known-limitations.md).

## See also

- [Code pillar](../04-four-pillars/code.md)
- [3.6 Trigram + symbol index](../03-core-subsystems/3.6-trigram-symbol-index.md)
- [Code ingestion plugin](../06-ingestion-plugins/code.md)
