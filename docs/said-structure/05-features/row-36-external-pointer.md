# Row 36 — External pillar: Enterprise pointer mode

**Status:** ✅ shipped 2026-04-22

## What it does

Registers a searchable **pointer** (URI + mime + title + summary) without embedding the actual document bytes. For enterprise deployments where content lives in SharePoint / S3 / internal systems and `.said` is the discovery index layer.

## Where it lives

- [`SaidFile::remember_as_external_pointer`](../../../crates/sca-core/src/said_file.rs)
- CLI — `said ingest <path> --pointer --summary "…"` in [`said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs)
- MCP — `ingest` tool with `pointer=true` + `summary` in [`said-mcp/src/tools.rs`](../../../crates/said-mcp/src/tools.rs) and [`handler.rs`](../../../crates/said-mcp/src/handler.rs)

## Inputs

Rust API:
```rust
brain.remember_as_external_pointer(
    Some("ext_42"),                    // doc_id
    "https://share/doc.pdf",           // uri
    Some("pdf"),                       // mime
    Some("Q3 Report"),                 // title
    "Brief summary for SCA to index",  // summary
    vec!["quarter:q3".to_string()],    // extra tags
)
```

CLI:
```
said ingest /share/doc.pdf --pointer --summary "Q3 Report — brief summary"
said ingest /share/docs/ --pointer    # folder walk, one pointer frame per file
```

MCP:
```json
{"name": "ingest", "arguments": {"path": "/share/doc.pdf", "pointer": true, "summary": "..."}}
```

## Outputs

One frame with body:
```
https://share/doc.pdf
mime: pdf
title: Q3 Report
summary: Brief summary for SCA to index
```

Tags:
- `pillar:external`
- `external:pointer`
- `mime:<type>` (when mime provided)

Frame `uncompressed_len` = summary length; no blob stored.

## How to test

```rust
let fid = sf.remember_as_external_pointer(
    Some("ext_test"),
    "file:///test.pdf",
    Some("pdf"),
    Some("Test"),
    "Brief test summary",
    vec![],
);
let meta = sf.frames.get_meta("ext_test").unwrap();
assert_eq!(meta.pillar, Pillar::External);
assert!(meta.tags.iter().any(|t| t == "external:pointer"));
assert!(meta.uncompressed_len < 500);  // no blob
```

End-to-end on `tmp_pointer_test.said`:
- 5 KB source file → 17 KB `.said` (header + SCA + summary only)
- Retrievable with `said ask "test summary"` at confidence 0.65

## How to extend

A Portable embedded counterpart (`remember_as_external_embedded` with `content: &[u8]`) requires:
1. Implement the XBLB blob-store section format in the file format
2. Mirror the pointer writer's API with a `content` parameter
3. Tag frames with `external:embedded` + `sha256:<hash>`

Not yet shipped; tracked in [Roadmap](../12-roadmap.md).

## Known limitations

- No sub-section for the external pillar yet — External pointers and External embeds (when shipped) will coexist; admins need a way to distinguish. The `external:pointer` vs `external:embedded` tags are the plan.

## See also

- [External pillar](../04-four-pillars/external.md)
- [Row 37 BrainMode](row-37-brain-mode.md) — Enterprise mode enforces pointer-only
