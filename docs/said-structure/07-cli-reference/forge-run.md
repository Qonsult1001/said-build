# said forge run

Generate per-story artifacts against the configured BYO LLM. Runs retrieval → generation → validation → frame writes → disk projection → skill deployment.

## Synopsis

```
said --path <file.said> forge run [--all | --ids <csv> | --filter <expr>]
                                   [--force] [--yes]
                                   [--halt-after <N>]
```

## Flags

| Flag | Description |
|---|---|
| `--all` | Run every story in the current directive |
| `--ids <csv>` | Comma-separated list of slugs (e.g. `post-pet,get-pet-petid`) |
| `--filter <expr>` | Filter expression (same DSL as `forge list`) |
| `--force` | Re-run even if a story is already complete (bumps `run_n`) |
| `--yes` | Skip interactive cost-preflight confirmation |
| `--halt-after <N>` | Override circuit-breaker threshold (default 5) |
| `--json` | Emit JSON summary |

You must pass exactly one of `--all`, `--ids`, or `--filter`.

## Outputs

Text mode prints a per-story progress line:
```
Selected 3 stories.  Estimated cost (at 3k in / 1k out each): $0.27
Continue? [y/N] y
[1/3] post-pet       ✓  1023 tokens in / 245 out, 842ms
[2/3] put-pet        ⊘  skipped
[3/3] get-pet-petid  ✓  1103 tokens in / 287 out, 891ms
done — 2 succeeded, 1 skipped, 0 failed
```

When the circuit breaker trips (5 consecutive same-class failures):
```
circuit breaker: 5 consecutive auth failures — halting
```

JSON mode:
```json
{"total":3,"succeeded":2,"skipped":1,"failed":0,"halted":false}
```

## Side-effects

For each successful story:
- Writes `forge:request:<hash>:<slug>:rN` frame (run envelope)
- Writes 4 audit frames: `forge:run:*:rN:{input,prompt,output,meta}`
- Writes 4 artifact frames: `forge:{spec,plan,tasks,brain}:<hash>:<slug>`
- Projects `.forge/<slug>/` folder: `story.md`, `plan.md`, `tasks.md`, `brain.md`, `.forge-meta`
- Writes `.claude/skills/<slug>/SKILL.md` (picked up live by Claude Code, no restart)

## Exit codes

- `0` — batch completed (some stories may have failed)
- non-zero — fatal error before the loop started (no directive, bad config)

## Examples

Run a specific story:
```
said --path demo.said forge run --ids post-pet --yes
```

Run every DELETE endpoint:
```
said --path demo.said forge run --filter method:DELETE --yes
```

Run everything, non-interactive (suitable for CI):
```
said --path demo.said forge run --all --yes --json
```

Force a re-run to get fresh output:
```
said --path demo.said forge run --ids post-pet --force --yes
```

## Configuration

```toml
# ~/.said/config.toml
[forge]
concurrency = 4
halt_after = 5

[forge.llm]
provider = "anthropic"
model = "claude-opus-4-7"
api_key = "${ANTHROPIC_API_KEY}"

[forge.grounding]
max_frames = 24
inline_top_n = 8

[forge.costs]
input_usd_per_mtok  = 15.0
output_usd_per_mtok = 75.0
```

## Related

- Feature: [`../05-features/forge.md`](../05-features/forge.md)
- MCP equivalent: `forge_run` — **deferred to CLI in MVP** (MutexGuard !Send over .await). The MCP tool returns a CLI hint and asks the caller to run from the shell.
- Source: [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) `forge_cli::cmd_run`, [`crates/said-forge/src/runner.rs`](../../../crates/said-forge/src/runner.rs) `run_one`
