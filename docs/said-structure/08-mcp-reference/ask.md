# MCP tool: ask

3-engine smart router — Sym + Grep + SCA fusion. Parity with CLI [`said ask`](../07-cli-reference/ask.md).

## Schema

```json
{
  "name": "ask",
  "arguments": {
    "query": "string, required",
    "top": "integer, optional, default 10",
    "deep": "boolean, optional, default false"
  }
}
```

## Description (as exposed to MCP clients)

> Ask the brain a natural-language question — runs the 3-engine smart router: Sym (exact symbol lookup, confidence 1.00), Grep (literal keyword match, 0.40-0.95), and SCA semantic (the recall pipeline with BM25, entity boost, multi-hop bridge, confidence 0.30-0.80). Results are merged by confidence with a self-calibrating relative cutoff. Use deep=true to widen the candidate pool. Same behavior as `said ask` on the CLI — identical fusion + identical result set.

## Behavior

1. Parse keywords from query (stopword-filtered, stem-aware)
2. Run the 3 engines in sequence (Sym → Grep → SCA semantic)
3. Merge by doc_id, keep highest confidence per doc
4. Apply relative cutoff (drop below `top × 0.30`, guarantee SCA top-3)
5. Build text output
6. Auto-dream check — fired in core (`maybe_dream()` inside `ask()`); handler then calls `save_brain_only()`
7. Return result

## Example call

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"ask","arguments":{"query":"how does compact work","top":5}}}
```

## Example response

```json
{
  "jsonrpc":"2.0","id":1,
  "result":{
    "content":[{
      "type":"text",
      "text":"Ask: \"how does compact work\"  (5 results, keywords: compact, work)\n\n1. [1.00][symbol] crates/sca-core/src/frames.rs::compact (fn:1107-1122)\n...\n"
    }]
  }
}
```

## Differences from CLI

The MCP tool returns text content; the CLI prints it. Agents consuming MCP should expect human-readable output with `[confidence][engine_kind]` per line. For structured consumption, use `ask` and post-parse, or add a caller-side `--json`-equivalent tool (not shipped today).

## Auto-fire side effects

- Auto-dream fires in **core** (`sca_core::ask::ask` → `maybe_dream()`) when the
  pending-query count crosses the threshold — the handler no longer triggers it, so
  CLI/MCP/Rust API dream identically.
- `brain.save_brain_only()` after every call (persists the brain state core evolved —
  recall weights + S_slow + query log)

Both are silent to the caller. State updates on the `.said` file via `save_brain_only` are safe — BRAN-only, no frame corruption risk.

## See also

- [CLI said ask](../07-cli-reference/ask.md)
- [3.5 Retrieval pipeline](../03-core-subsystems/3.5-retrieval-pipeline.md)
- [Row 44 MCP admin tool](../05-features/row-44-mcp-admin.md) — same pattern of fusion
