# said forge reset

Tombstone a story's frames and remove its projection folder + skill file. Destructive.

## Synopsis

```
said --path <file.said> forge reset <slug> [--yes]
```

## Arguments

| | |
|---|---|
| `slug` | Story slug to reset |

## Flags

| Flag | Description |
|---|---|
| `--yes` | Skip interactive confirmation |
| `--json` | Emit JSON summary |

## What it does

1. Tombstones every `forge:*` frame for the story (`story`, `request`, `run:*`, `spec`, `plan`, `tasks`, `brain`). Frames remain in the brain's history and can be inspected via `said history` / `said checkout` — they're not physically deleted.
2. Removes `.forge/<slug>/` directory.
3. Removes `.claude/skills/<slug>/` directory.

Next `said forge run` on that story starts fresh from `r1`.

## Outputs

Text mode:
```
tombstoned 10 frames, removed .forge/post-pet/ and .claude/skills/post-pet/
```

JSON mode:
```json
{"tombstoned_frames": 10, "slug": "post-pet"}
```

## Exit codes

- `0` — success (including when nothing existed to remove)
- non-zero — no directive loaded, or filesystem error

## Examples

```
said --path demo.said forge reset post-pet --yes
```

## Related

- Feature: [`../05-features/forge.md`](../05-features/forge.md)
- MCP equivalent: `forge_reset` (requires `confirm: true`)
- Source: [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) `forge_cli::cmd_reset`
