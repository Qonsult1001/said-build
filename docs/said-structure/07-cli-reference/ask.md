# said ask

The primary query verb. Runs the 3-engine fusion — Sym + Grep + SCA — and returns the highest-confidence hits across all three.

## Usage

```
said [--path FILE] [--json] ask <QUERY> [--top N] [--deep]
```

## Arguments

- `<QUERY>` — natural-language question in quotes
- `--top N` — max results, default 10
- `--deep` — widen SCA fetch (100 vs 20) and return ALL candidates above the relative cutoff. Used for cross-document synthesis.

## Behavior

1. Keyword extraction (stopword-filtered, stem-aware)
2. Engine A — Sym exact match across symbol-candidate spellings (snake_case, camelCase, PascalCase) — confidence 1.00
3. Engine B — Grep per keyword with term-overlap scoring — confidence 0.40 – 0.95
4. Engine C — SCA semantic via [recall_fused](../03-core-subsystems/3.5-retrieval-pipeline.md) — confidence 0.30 – 0.80
5. Merge by doc_id, keep highest confidence per doc
6. Apply **relative cutoff** — drop results below `top_confidence × 0.30`. Guarantee top-3 SCA hits always survive.
7. Auto-trigger brain-state dream if `pending_dream_queries ≥ dynamic_threshold`

## Output (default)

```
Ask: "how does compact work"  (8 results in 12.34ms)

  1. [1.00][symbol] crates/sca-core/src/frames.rs::compact (fn:1107-1122)
      pub fn compact(&mut self) -> (usize, u64) { self.flush_pending(...) ... }
  2. [0.85][text] crates/sca-core/src/said_file.rs::compact
      Compact: block-compress all frames, drop Deleted ones, rebuild DICT ...
  3. [0.68][semantic] docs/...
      ...
```

Legend: `[confidence][engine_kind]` where engine_kind ∈ {symbol, text, semantic}.

## Output (JSON mode)

```json
{
  "query": "how does compact work",
  "keywords": ["compact", "work"],
  "results": [
    {"doc_id": "...", "confidence": 1.00, "kind": "symbol", "location": "fn:1107-1122", "content": "..."},
    ...
  ],
  "elapsed_ms": 12.34,
  "dreamed": false
}
```

## Examples

```bash
# Code repo
said ask "how does the block cache work"

# Personal memory brain
said ask "when did Alice say she'd ship the feature?"

# Enterprise legal archive — cross-document synthesis
said ask "what are the termination clauses" --deep

# JSON for agent consumption
said --json ask "retention policy" | jq '.results[] | select(.confidence > 0.5)'
```

## Auto-dream

Every `ask` call accumulates the query embedding. When the count crosses `dynamic_dream_threshold(active_frames)` (50-500 based on corpus size), `brain.dream(threshold)` fires silently and adjusts the fingerprint centre toward the query distribution. The CLI notes this at the end of the run if it happened:

```
[brain] dream cycle complete — corpus drift toward recent query patterns
```

## Brain state persistence

Every `ask` call writes back the brain's partial state (recall weights, query log, dream cycle counter). Uses a fast `save_brain_only()` path that rewrites only the `BRAN` section — no risk of frame corruption.

## Matching MCP tool

[MCP `ask` tool](../08-mcp-reference/ask.md) — identical semantics, same fusion, same confidence cutoff.

## See also

- [3.5 Retrieval pipeline](../03-core-subsystems/3.5-retrieval-pipeline.md) — what Engine C does internally
- [3.6 Trigram + symbol index](../03-core-subsystems/3.6-trigram-symbol-index.md) — what Engines A and B consult
- [Row 35 Brain-state dream](../05-features/row-35-brain-state-dream.md) — auto-fire behavior
