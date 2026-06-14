# MCP tool: status

Brain state + mode + frame counts. The MCP equivalent of `said stats`, with richer context for LLM clients.

## Schema

```json
{"name": "status", "arguments": {}}
```

## Description

> Report the current state of the attached brain. Shows deployment mode, active frame count, file size, compression ratio, symbol / trigram presence, brain state (dream cycles, S_slow magnitude), and an "next steps" headline pointing at likely next tools.

## Behavior

1. Detect drift between in-memory state and on-disk file (another process may have rewritten the file). If drift detected, reload from disk first.
2. Gather stats + mode
3. If brain is empty, list populated sibling `.said` files in the same directory (common: MCP started on a placeholder, user wants to attach to a real brain)
4. Build a helpful text block with the headline + details

## Example response (populated brain)

```
Brain is POPULATED and ready to query.

Next steps:
• overview                    — list detected modules/products
• search "<query>"            — semantic search
• sym <name>                  — exact symbol lookup
• snapshot <module>           — extract a module workspace

─── Brain details ───
File:          willie.said
Mode:          enterprise (pointer-only; content-embedding ingests REFUSED)
Active frames: 19149  (ingested pieces of code/docs/memory)
Size on disk:  19 MB (19132432 bytes)
Search index:  present (fast grep available)
Symbols:       12847 named functions/classes/tables
Queries run:   1847 (brain learns from usage)
Dream cycles:  12   (memory consolidation events)
```

## Example response (empty brain with populated siblings)

```
⚠ Brain is EMPTY — but there are populated brains nearby:

  • willie.said (19.0 MB)
  • backup.said (5.4 MB)

You probably want to attach to one of those. Run:

• open path="<name>.said"    — switch to the real brain

…or if you want to stay empty and ingest fresh:

• init dir="<path>"          — bulk-ingest a whole folder
• remember content="…"        — store a single note/memory
```

This helps LLM clients that attached to a default empty brain figure out what to do next.

## Mode line

`Mode:` shows prominently so agents planning destructive operations know whether they're on Enterprise (no embed) or Portable:

```
Mode:          portable (embeds full content; USB-offline friendly)
Mode:          ENTERPRISE (pointer-only; content-embedding ingests REFUSED)
```

## See also

- [CLI said stats](../07-cli-reference/other-commands.md#said-stats)
- [Row 37 Brain mode](../05-features/row-37-brain-mode.md)
