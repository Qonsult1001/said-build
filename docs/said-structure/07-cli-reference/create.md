# said create

Make a new empty `.said` file with a chosen deployment mode.

## Usage

```
said create <FILE> [--mode portable|enterprise] [--json]
```

## Arguments

- `<FILE>` — path for the new `.said` file. Must NOT already exist.
- `--mode portable` (default) — embeds full content; USB-portable; what most users want
- `--mode enterprise` — refuses content-embedding ingests; pointer-only; licensed separately

## Output

```
Created: willie.said (mode: portable, immutable)
```

Or JSON:

```json
{"created": "willie.said", "mode": "portable"}
```

## Behavior

1. Check target path doesn't exist (refuses to overwrite)
2. Call `SaidFile::create_with_mode(path, parsed_mode)`
3. Save immediately — produces an empty ~19 KB file with header + `MODE` section + empty `BRAN` / `FTOC`
4. **Mode is IMMUTABLE** — there is no `said mode` command to switch. Users must create a new file with the other mode and re-ingest if they need it.

## Why mode is immutable

Portable and Enterprise are licensed differently:

- **Portable** — personal / single-user / offline. Full content embedding.
- **Enterprise** — compliance-gated. Refuses content embeds. Content stays in SoR (system of record); `.said` is the searchable index layer.

Allowing mode switches in place would undermine the licensing separation. The file either ships as Portable or Enterprise; users who need the other model create a new brain. See [Row 37](../05-features/row-37-brain-mode.md).

## Examples

```bash
said create notes.said
# → notes.said exists, 19 KB, portable mode

said create corp-index.said --mode enterprise
# → refuses `said ingest file.pdf` unless `--pointer` is used

said --json create api-brain.said | jq .mode
# → "portable"
```

## Refusal on existing file

```
$ said create notes.said
Error: File already exists: notes.said
```

Intentional — we refuse to clobber. If you want a fresh brain, delete the existing file explicitly or create it at a different path.

## Matching MCP tool

[MCP `create` tool](../08-mcp-reference/create.md) — same contract, plus safety check that refuses to overwrite populated brains (>32 KB).

## See also

- [Row 37 Brain mode (immutable)](../05-features/row-37-brain-mode.md)
- [2.5 Version history](../02-file-format/2.5-version-history.md)
