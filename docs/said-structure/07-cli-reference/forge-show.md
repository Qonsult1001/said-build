# said forge show

Print the bundled story + plan + tasks + brain markdown for one slug.

## Synopsis

```
said --path <file.said> forge show <slug>
```

## Arguments

| | |
|---|---|
| `slug` | Story slug (see `said forge list`) |

## Flags

| Flag | Description |
|---|---|
| `--json` | Emit `{"spec": "...", "plan": "...", "tasks": "...", "brain": "..."}` |

## Outputs

Text mode prints 4 sections, each headed:
```
========== spec ==========
<spec body>

========== plan ==========
<plan body>

========== tasks ==========
<tasks body>

========== brain ==========
<brain body>
```

Sections that haven't been generated yet show `(not yet generated)`. This is the CLI equivalent of the MCP `forge_get` tool.

## Exit codes

- `0` — success
- non-zero — no directive loaded

## Examples

After a `forge run`:
```
said --path demo.said forge show post-pet
```

Before a `forge run` (all four sections show "not yet generated"):
```
said --path demo.said forge show get-pet-petid
```

## Related

- Feature: [`../05-features/forge.md`](../05-features/forge.md)
- MCP equivalent: `forge_get` (returns same bundled markdown, 25k-token cap)
- Source: [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) `forge_cli::cmd_show`
