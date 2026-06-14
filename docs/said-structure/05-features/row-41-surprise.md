# Row 41 — Surprise / reconsolidation detector

**Status:** ✅ shipped 2026-04-22. Step 8 in the original 12-step plan.

## What it does

When a new frame would contradict or update an existing frame about the same topic, the writer tags it so downstream tools (dream, admin UI, agent feedback) can surface the conflict. Two detection paths run in parallel:

1. **Lexical** — `score_turn` sees `actually`, `no wrong`, `i meant` → tags `reconsolidation`
2. **Semantic** — `find_prior_match` looks up the Hamming-nearest existing frame and `classify_surprise` returns `{Benign, TopicalUpdate, Contradiction}` based on similarity + overlap

## Where it lives

- [`crates/sca-core/src/salience.rs`](../../../crates/sca-core/src/salience.rs) — `Surprise`, `PriorMatch`, `classify_surprise`
- [`crates/sca-core/src/said_file.rs`](../../../crates/sca-core/src/said_file.rs) — `find_prior_match`, hook in `remember_with_salience`
- MCP `remember` surfaces contradictions in the response line

## Classification thresholds

```
similarity < 0.25                                  → Benign
similarity ≥ 0.25 AND overlap ≥ 0.90               → TopicalUpdate
similarity ≥ 0.25 AND 0.40 ≤ overlap < 0.90        → Contradiction
similarity ≥ 0.25 AND overlap < 0.40               → Benign
```

Where:
- `similarity` = dominance-based score (top-1 / top-2 ratio from `search_internal(content, 3)`)
- `overlap` = fraction of meaningful query tokens (≥4 chars, non-stopword) that appear in the prior frame's body

## Inputs

Automatic — fires on every `remember_with_salience` call. Caller doesn't pass anything extra.

## Outputs

Tags appended to the new frame:

**Contradiction:**
- `reconsolidation`
- `reconsolidation:contradicts`
- `contradicts:<prior_doc_id>`

**TopicalUpdate:**
- `reconsolidation`
- `reconsolidation:update`
- `updates:<prior_doc_id>`

**Benign:** no surprise tags added.

MCP `remember` response line:
```
✓ Saved to brain (frame #142, pillar=semantic). salience=35 (medium) · ⚠ contradicts prior frame `fact_veg`
```

## How to test

[`examples/surprise_probe.rs`](../../../crates/sca-core/examples/surprise_probe.rs) — 5-frame fixture:

1. First fact (empty brain) → Benign
2. Same topic, different value → **Contradiction** with `contradicts:<prior_id>` ✓
3. Unrelated topic → Benign
4. Explicit "actually" correction → Lexical `reconsolidation` fires via `score_turn`
5. Topical expansion (same topic + more tokens) → Benign (overlap drops with added detail)

## How to extend

Tuning thresholds (edit `classify_surprise`):
```rust
if m.similarity < 0.25 { return Benign; }
if m.token_overlap >= 0.90 { return TopicalUpdate; }
if m.token_overlap >= 0.40 { return Contradiction; }
Benign
```

Add new fixtures to `surprise_probe.rs` when tuning so regressions show up.

To swap to an embedded ML classifier (v1):
1. Keep `Surprise` enum + `classify_surprise` signature
2. Replace internals with a Model2Vec embedding + logistic regression
3. Preserve the same three-tag output shape

## Known limitations

- Thresholds tuned on the `surprise_probe` fixture (5 cases). More diverse tests could refine them.
- Direct `FrameStore::put_with` callers (document_ingest, code_search, whisper_ingest) don't route through `remember_with_salience` so they don't get surprise detection. Not usually wanted for bulk ingest; explicit callers opt in.

## See also

- [Row 33 Salience scorer](row-33-salience.md)
- [Semantic pillar](../04-four-pillars/semantic.md)
