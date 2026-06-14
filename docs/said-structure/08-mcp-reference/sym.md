# MCP tool: sym

Exact symbol name lookup against the SYMS index. Sub-millisecond.

## Schema

```json
{
  "name": "sym",
  "arguments": {
    "name": "string, required",
    "max": "integer, optional, default 20"
  }
}
```

## Description

> Exact symbol lookup for code entities (functions, classes, methods, types). Returns location info including the source file and line range. Sub-millisecond on any corpus size — pure HashMap lookup via the SYMS section.

## Response

```
Symbol 'FrameStore' (1 match):
  crates/sca-core/src/frames.rs::FrameStore  (struct:325-354)
```

If multiple matches (same name in different files):

```
Symbol 'compact' (3 matches):
  crates/sca-core/src/said_file.rs::compact  (fn:2076-2099)
  crates/sca-core/src/frames.rs::FrameStore::compact  (method:1107-1122)
  crates/said-cli/src/main.rs::cmd_compact  (fn:3421-3445)
```

## No match

```
No symbol found: XYZ
```

## See also

- [CLI said sym](../07-cli-reference/other-commands.md#said-sym)
- [3.6 Trigram + symbol index](../03-core-subsystems/3.6-trigram-symbol-index.md)
