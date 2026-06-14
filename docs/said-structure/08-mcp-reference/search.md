# MCP tool: search

Pure SCA semantic retrieval with pillar scoping. Distinct from [ask](ask.md) — no Sym/Grep fusion, just the SCA + BM25 + graph fan-out path from [recall_fused](../03-core-subsystems/3.5-retrieval-pipeline.md).

## Schema

```json
{
  "name": "search",
  "arguments": {
    "query": "string, required",
    "deep": "boolean, optional, default false (top 10 vs 500 fetch)",
    "pillar": "string, optional (comma-separated pillar names)"
  }
}
```

## Description

> Search the brain for information. Handles code, documents, SQL, passkeys, cross-document synthesis — the engine routes automatically using semantic search, keyword matching, and symbol lookup all fused together. Use deep=true for full narrative (all relevant chunks, no cap). Default returns top-10 (correct answer always in window). Optional pillar= narrows results to one CLS memory pillar: episodic, semantic, procedural, external, code, memory. Comma-separated for multiple.

## Pillar scoping

```json
{"name":"search","arguments":{"query":"deployment","pillar":"procedural,code"}}
```

Unknown pillar names silently ignored. When the filter would return zero results, returns whatever the unscoped search would return (safe default).

## Example response

```
1. [score=8.234] crates/sca-core/src/frames.rs::compact
pub fn compact(&mut self) -> (usize, u64) { ... }

2. [score=7.891] ...
```

Each line: `[score=<float>] <doc_id>` followed by a 500-char content preview.

## Auto-dream side effects

Same as `ask` — brain state auto-saves; dream cycle fires if threshold crossed.

## When to use search vs ask

- **`ask`** — user/agent question phrased naturally. Default choice. Handles entity extraction, symbol candidates, confidence ranking.
- **`search`** — when you want raw similarity scores without the confidence-cutoff filter, OR when you need pillar scoping.

## See also

- [CLI said search](../07-cli-reference/other-commands.md#said-search)
- [ask](ask.md) — fusion layer on top of search
- [Row 31 Per-pillar retrieval](../05-features/row-31-per-pillar-retrieval.md)
