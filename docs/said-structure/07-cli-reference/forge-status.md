# said forge status

Inspect the run state of one story or the current directive.

## Synopsis

```
said --path <file.said> forge status [--story <slug>]
```

## Flags

| Flag | Description |
|---|---|
| `--story <slug>` | Show status for a single story (latest run number). Omit to get directive-level summary. |
| `--json` | Emit JSON |

## Outputs

Without `--story`, prints the directive-level summary:
```
directive: c7658   total stories: 20
```

With `--story`, prints the latest run number:
```
story: post-pet  latest_run: r2
```

Run numbers start at `r1` and bump on every `forge run --force` (or every fresh run on an incomplete story).

## Exit codes

- `0` — success
- non-zero — no directive loaded

## Examples

```
said --path demo.said forge status
said --path demo.said forge status --story post-pet
said --path demo.said forge status --story post-pet --json
```

## Related

- Feature: [`../05-features/forge.md`](../05-features/forge.md)
- MCP equivalent: `forge_status` (accepts `story_ids: Vec<String>` for batch status)
- Source: [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) `forge_cli::cmd_status`
