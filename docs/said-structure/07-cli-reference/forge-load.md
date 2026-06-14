# said forge load

Load a directive document (OpenAPI YAML/JSON, Markdown) into `.said` as frames.

## Synopsis

```
said --path <file.said> forge load <path-or-url> [--source <adapter>]
```

## Arguments

| | |
|---|---|
| `path_or_url` | Local file path or HTTP(S) URL to the directive |

## Flags

| Flag | Description |
|---|---|
| `--source <name>` | Force a specific adapter (`openapi`, `markdown`). Auto-detected from extension + content peek if omitted. |
| `--json` | Emit structured JSON instead of human text |

## Outputs

Text mode:
```
Loaded directive c7658 via openapi: 20 stories
```

JSON mode:
```json
{"directive_hash": "c7658", "adapter": "openapi", "stories": 20}
```

Side-effects:
- Writes one `forge:directive:<hash>` frame (pillar External) with the raw bytes + metadata.
- Writes one `forge:story:<hash>:<slug>` frame per extracted story.

## Exit codes

- `0` — success
- non-zero — bad flag, unreachable path/URL, malformed directive, or no adapter matches

## Examples

Load a local Swagger Petstore fixture:
```
said --path demo.said forge load crates/said-forge/fixtures/petstore.yaml
```

Load a live OpenAPI spec from a URL:
```
said --path demo.said forge load https://petstore3.swagger.io/api/v3/openapi.json
```

Load a Markdown checklist, forcing the adapter (skips auto-detect):
```
said --path demo.said forge load requirements.md --source markdown
```

## Related

- Feature: [`../05-features/forge.md`](../05-features/forge.md)
- Ingestion plugin (for parsing): [`../06-ingestion-plugins/openapi.md`](../06-ingestion-plugins/openapi.md)
- MCP equivalent: `forge_load` (see [`../08-mcp-reference/forge-tools.md`](../08-mcp-reference/forge-tools.md))
- Source: [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) `forge_cli::cmd_load`
