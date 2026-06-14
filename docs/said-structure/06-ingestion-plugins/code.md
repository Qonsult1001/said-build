# Code plugin — tree-sitter AST chunking

**Feature flag:** `code` in [`crates/sca-core/Cargo.toml`](../../../crates/sca-core/Cargo.toml).

Entry point: [`crates/sca-core/src/code_search.rs`](../../../crates/sca-core/src/code_search.rs). Invoked by `said init <dir>` in the CLI.

## What it does

Parses source files with [tree-sitter](https://tree-sitter.github.io/) per-language grammars, identifies symbol boundaries (functions, classes, methods), chunks the file at those boundaries, and writes one frame per chunk. The symbol name + location gets recorded in the symbol index (`said sym`).

Plus SQL: custom GO-batch parser (tree-sitter doesn't have a solid SQL grammar for T-SQL / PL/pgSQL).

## Supported languages

| Language | Tree-sitter crate | Version |
|---|---|---|
| Rust | `tree-sitter-rust` | 0.23 |
| Python | `tree-sitter-python` | 0.23 |
| JavaScript | `tree-sitter-javascript` | 0.23 |
| TypeScript | `tree-sitter-typescript` | 0.23 |
| Go | `tree-sitter-go` | 0.23 |
| Java | `tree-sitter-java` | 0.23 |
| C# | `tree-sitter-c-sharp` | 0.23 |
| SQL (T-SQL / PL/pgSQL) | custom GO-batch parser (in-tree) | — |

Tree-sitter itself at `0.24`.

## Per-language symbol kinds

| Language | Symbol kinds detected |
|---|---|
| Rust | fn, struct, enum, trait, impl, const, type |
| Python | class, method, function (def, async def), const |
| JavaScript | function, class, method, arrow function, const |
| TypeScript | function, class, interface, type, enum, method |
| Go | func, method, struct, interface, const, type |
| Java | class, interface, method, field |
| C# | class, interface, method, property, field |

Kind strings are short ASCII — `"fn"`, `"struct"`, `"class"`, etc.

## Pipeline

```
said init <dir>
  │
  ▼
walk_dir_gitignore(dir)
  filter to supported extensions + text extensions
  │
  ▼
for each file:
  code_search::ingest_file(path)
    tree-sitter parse → AST
    walk AST identifying top-level symbols
    chunk source at symbol boundaries
    for each chunk:
      brain.remember_as_code(
        Some(&format!("{}::{}", file, symbol_name)),
        language,
        chunk_source,
        Some(symbol_name),
        Some(file_path),
        tags,
      )
      brain.record_symbol(
        symbol_name,
        &format!("{}::{}", file, symbol_name),
        kind,
        start_line,
        end_line,
      )
  │
  ▼
brain.build_index()
  → builds TRGM (trigram index)
  → builds SYMS (symbol index from record_symbol entries)
  → serialize both on next save
```

## Chunking strategy per language

- **Rust:** one frame per `fn`, `impl` block, `struct`, `enum`, `trait`. Module-level docs go as their own frame.
- **Python:** one frame per top-level `def` / `class`. Methods inside classes get their own frame.
- **JavaScript / TypeScript:** one frame per top-level `function` / `class` / `const = () =>`. Arrow functions assigned to consts are captured.
- **Go:** one frame per `func` / `type`.
- **Java / C#:** one frame per method + one per class (with just field declarations and class-level docs).

Chunk sizes are symbol-sized, not length-capped. An `impl` block with 20 methods becomes 20+1 frames.

## Outputs

- Frame body format: `[<lang>] <symbol> (<path>)\n<source>`
- Tags: `pillar:code`, `lang:<language>`, `symbol:<name>`, `source:<path>`, `kind:<symbol-kind>`
- `FrameMeta.pillar` — currently `Pillar::Memory` for direct `put_with` callers (see [Known limitations](../11-known-limitations.md)); `remember_as_code` would produce `Pillar::Code` if the `said init` walker migrated to use it
- Symbol index populated for `said sym <name>` — sub-millisecond lookup

## Performance

Observed on SAID-ECHO's own codebase:

| Corpus | Time |
|---|---|
| SAID-ECHO repo (5132 files, ~400k LOC) | **5 min 43 sec** total — parse + chunk + SCA + compact |
| Single 500-line Rust file | ~80 ms |
| 50k-line Go file | ~3 sec |

Tree-sitter parse is not the bottleneck; SCA encoding + compact is.

## How to test

```
# Initialize a fresh .said on the SAID-ECHO repo itself
cargo run --release -p said-cli --features "static-embed code" -- \
    init .

# Expect: walks ~5000 files, creates said.said, reports "27k frames across 5132 files"
said sym FrameStore
# → crates/sca-core/src/frames.rs::FrameStore (struct:325-354)

said ask "how does compact work?"
# → SCA + symbol + grep fused results
```

## How to extend

### Add a new language
1. Add the tree-sitter crate as an optional dep: `[dependencies.tree-sitter-<lang>] version = "0.23" optional = true`
2. Extend the `code` feature: `code = [..., "dep:tree-sitter-<lang>"]`
3. Add a per-language module to `code_search.rs` implementing the chunk walk
4. Wire into the extension → language dispatcher
5. Add sample file to the test corpus; verify `said sym` finds symbols

### Tune chunk size
Current policy = one chunk per top-level symbol. For very large classes (Java enterprise code), a further "method-per-chunk" split might improve retrieval precision. Configurable via a `ChunkPolicy` enum — not yet shipped.

### LSP-enriched code frames
The `lsp` feature (see [lsp plugin](lsp.md)) can provide cross-file reference edges. Not yet baked into code ingest; shipped as a separate query-time overlay.

## Known limitations

- Only top-level symbols (no nested closures, lambdas within methods)
- No macro expansion; Rust `macro_rules!` bodies get frame-ified but generated code isn't
- Generics + template code chunk at declaration site; no per-monomorphization expansion
- Pillar persistence sweep: code frames still land as `Pillar::Memory`; migration to `Pillar::Code` tracked in [Known limitations](../11-known-limitations.md)

## See also

- [Code pillar](../04-four-pillars/code.md) — how Code frames interact with retrieval
- [3.6 Trigram + symbol index](../03-core-subsystems/3.6-trigram-symbol-index.md) — the exact-lookup path
- [LSP plugin](lsp.md) — cross-file refs on top of tree-sitter
- [Row 47 Code pillar writer](../05-features/row-47-code.md)
