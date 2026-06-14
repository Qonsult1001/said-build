# Row 31 — Per-pillar retrieval scope

**Status:** ✅ shipped 2026-04-21 (Decision 2)

## What it does

`search` / `ask` can narrow results to a subset of pillars. A caller asking for "the API spec" can pass `pillar=semantic,external` to exclude chat turns and runbooks.

## Where it lives

- [`crates/sca-core/src/recall.rs`](../../../crates/sca-core/src/recall.rs) — `search_full_scoped_pillars`, `rerank_by_pillar`
- [`crates/sca-core/src/said_file.rs`](../../../crates/sca-core/src/said_file.rs) — `SaidFile::recall_by_pillar`
- [`crates/said-mcp/src/tools.rs`](../../../crates/said-mcp/src/tools.rs) — `SearchTool.pillar` field
- [`crates/said-mcp/src/handler.rs`](../../../crates/said-mcp/src/handler.rs) — `handle_search` parses the comma-list

## Inputs

- Rust API: `Option<&HashSet<Pillar>>` — `None` = all pillars; `Some(set)` = only those
- MCP: comma-separated string (`"episodic,semantic"`) in `SearchTool.pillar`. Unknown names ignored silently.

## Outputs

`Vec<RecallResult>` filtered by pillar set; otherwise identical to the normal recall pipeline.

## How to test

Regression check — with `None` filter, MTEB + LoCoMo baselines must be unchanged. Verified:

```
LEMBNeedleRetrieval   1.00000 (unchanged)
LEMBWikimQARetrieval  1.00000 (unchanged)
```

Scoped check:
```rust
let mut pillars = HashSet::new();
pillars.insert(Pillar::Episodic);
let hits = sf.recall_by_pillar("what happened?", 10, Some(&pillars));
for h in &hits {
    assert_eq!(sf.frames.get_meta(&h.doc_id).unwrap().pillar, Pillar::Episodic);
}
```

## How to extend

Add a new filter (e.g. `created_after=`, `tag=`):
1. Thread it through `search_full_scoped_pillars` as a new parameter
2. Build the filter HashSet at the top of the function before scoring
3. Let the existing scoring pipeline treat the narrowed set as the whole corpus
4. Expose on MCP `SearchTool` + CLI `--<flag>`

## Known limitations

- Current scoping is post-ranking (over-fetch 4× then filter). For a very skewed corpus (e.g. 99% Code, 1% Episodic), a pre-filter would be more efficient; tracked as an optimization.

## See also

- [3.5 Retrieval pipeline](../03-core-subsystems/3.5-retrieval-pipeline.md)
- [Row 30 Pillar enum](row-30-pillar-enum.md)
