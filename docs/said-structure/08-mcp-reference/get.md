# MCP tool: get

Read the full content of a frame by its `doc_id`.

## Schema

```json
{
  "name": "get",
  "arguments": {
    "doc_id": "string, required"
  }
}
```

## Description

> Read the exact content of a specific frame by its doc_id. Use when a search/ask preview was truncated and you need the whole thing, or when you want to reconstruct the full context around a hit.

## Behavior

`brain.get(doc_id)` decompresses the frame's payload (cache-hit if the containing block was recently touched) and returns the bytes as UTF-8 text.

## Response

```
<full frame content>
```

## No match

```
Frame not found: <doc_id>
```

## See also

- [ask](ask.md) / [search](search.md) — return doc_ids to pass here
- [3.4 FrameStore](../03-core-subsystems/3.4-framestore.md) — compression + cache semantics
