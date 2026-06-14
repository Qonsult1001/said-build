# said forge list

Preview the story list extracted from the most recently loaded directive.

## Synopsis

```
said --path <file.said> forge list [--filter <expr>]
```

## Flags

| Flag | Description |
|---|---|
| `--filter <expr>` | Filter the result set (see below) |
| `--json` | Emit JSON array of `{slug, title, kind, status, tags}` |

## Filter expressions

| Form | Meaning |
|---|---|
| `method:<VERB>` | HTTP method (OpenAPI stories only); case-insensitive |
| `path:<glob>` | Glob over the path field (e.g. `/pet/*`) |
| `kind:<StoryKind>` | snake_case match: `api_endpoint`, `requirement`, `ticket`, `table_row`, `generic` |
| `tag:<name>` | OpenAPI tag match |
| `text:<substr>` | Substring match against title OR slug, case-insensitive |

## Outputs

Text mode prints a numbered list:
```
#001  POST    /pet                                      Add a new pet to the store
#002  PUT     /pet                                      Update an existing pet
#003  GET     /pet/findByStatus                         Finds Pets by status
...
```

JSON mode:
```json
{"stories":[
  {"slug":"post-pet","title":"Add a new pet to the store","kind":"api_endpoint",
   "method":"POST","path":"/pet"},
  ...
]}
```

## Exit codes

- `0` — always (empty list is not an error)

When no directive is loaded, prints "No directive loaded. Run `said forge load <path>` first." and exits 0.

## Examples

List everything:
```
said --path demo.said forge list
```

Filter by HTTP method:
```
said --path demo.said forge list --filter method:DELETE
```

Filter by path glob:
```
said --path demo.said forge list --filter 'path:/pet/*'
```

JSON output (for piping to jq):
```
said --path demo.said forge list --json | jq '.stories | length'
```

## Related

- Feature: [`../05-features/forge.md`](../05-features/forge.md)
- MCP equivalent: `forge_list` (same filter DSL)
- Source: [`crates/said-forge/src/filter.rs`](../../../crates/said-forge/src/filter.rs) — filter parser + matcher, 7 unit tests
