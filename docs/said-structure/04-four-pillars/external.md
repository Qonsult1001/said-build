# Pillar — External

Pointers (or, in Portable mode, embedded content) to external resources — documents, URLs, database rows, API responses. The pillar that keeps `.said` usable as a discovery layer without embedding gigabytes of corporate content.

## Two modes

### Enterprise pointer mode — SHIPPED

A searchable **pointer** (URI + mime + title + summary) with no embedded bytes. The original stays in its system of record (SharePoint, S3, Dropbox, database). Agents find what exists; callers fetch the bytes at read time if they need them.

```rust
brain.remember_as_external_pointer(
    Some("ext_q3_report"),
    "file:///share/drive/reports/q3.pdf",
    Some("pdf"),
    Some("Q3 2026 Financial Report"),
    "Revenue +12% YoY, EBITDA +8%, cash runway extended to 18 months",
    vec!["quarter:q3".to_string()],
);
```

CLI:
```
said ingest /path/to/doc.pdf --pointer --summary "brief description"
```

MCP:
```json
{"name": "ingest", "arguments": {"path": "/share/doc.pdf", "pointer": true, "summary": "..."}}
```

Body layout on disk (everything searchable):
```
file:///share/drive/reports/q3.pdf
mime: pdf
title: Q3 2026 Financial Report
summary: Revenue +12% YoY, EBITDA +8%, cash runway extended to 18 months
```

Tags auto-applied:
- `pillar:external`
- `external:pointer`
- `mime:<type>` (if provided)

### Portable embed mode — planned

Full content embedded in the `.said` file so the brain works offline. Formalized in the architecture spec as an `XBLB` blob section:

```rust
pub struct ExternalEmbedded {
    uri: String,              // informational (may be stale)
    mime: String,
    title: String,
    content_bytes: Vec<u8>,   // full file, stored in BLOB section
    content_sha256: [u8; 32],
    chunks: Vec<FrameId>,     // SCA frames pointing into content_bytes
    summary: String,
    summary_fp: [u8; 8],
    ingested_at: u64,
}
```

**Current reality** — regular `said ingest` on a Portable brain already embeds full content, chunks it, and indexes each chunk. The chunks today go into `Pillar::Memory`, not `Pillar::External`. Formalizing XBLB means:

1. Tag ingested-doc frames with `pillar:external` + `external:embedded` + `uri:<source>` + `sha256:<hash>`
2. Group the chunks into an `ExternalEmbedded` record
3. Add an `XBLB` section for raw blob storage (today the chunks ARE the storage)
4. Add `remember_as_external_embedded` as a writer API

Not yet shipped. See [Row 36](../05-features/row-36-external-pointer.md) for pointer mode; the embedded side is deferred.

## Enterprise-mode enforcement

Enterprise-mode brains **refuse** content-embedding ingests entirely. Only pointer ingests + explicit `remember` (text) are allowed. See [Row 37 — BrainMode](../05-features/row-37-brain-mode.md).

Error on violation:
```
This brain is in ENTERPRISE mode — content-embedding ingests are refused.
Use `--pointer` to register a searchable pointer without embedding content.
Enterprise and Portable are licensed separately and cannot be swapped —
create a fresh brain with `said create <file> --mode portable` if you need
full content embedding.
```

## Retrieval ranking (planned)

Architecture spec:
```
score = metadata_filter + schema_query    (never rank by content, it's a pointer)
```

**Current reality** — External frames rank via standard SCA + BM25 on the body text (which is just URI + mime + title + summary). This is fine for pointer mode — the summary IS the discoverable text. When XBLB embedded mode ships, ranking may need to distinguish "rank by summary" vs "rank by embedded chunks."

## Typical queries

```
said ask "financial reports"                           # ← External pointers with matching summaries
said search pillar=external mime:pdf query="Q3"        # ← scoped
said admin list-tombstones --like ext_                 # ← admin view of External frames
```

## How to test

```rust
let fid = sf.remember_as_external_pointer(
    Some("ext_test"),
    "https://example.com/doc.pdf",
    Some("pdf"),
    Some("Test Doc"),
    "A test document about the widget project",
    vec![],
);

let meta = sf.frames.get_meta("ext_test").unwrap();
assert_eq!(meta.pillar, Pillar::External);
assert!(meta.tags.iter().any(|t| t == "pillar:external"));
assert!(meta.tags.iter().any(|t| t == "external:pointer"));
assert!(meta.tags.iter().any(|t| t == "mime:pdf"));

// Storage check — no blob. Body should be short (~50-200 bytes).
assert!(meta.uncompressed_len < 500);
```

## See also

- [Row 36 External pillar — Enterprise pointer mode](../05-features/row-36-external-pointer.md)
- [Row 37 Brain mode](../05-features/row-37-brain-mode.md)
- [Planned — XBLB embedded section](../12-roadmap.md)
