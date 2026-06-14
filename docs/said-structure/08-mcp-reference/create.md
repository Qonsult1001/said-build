# MCP tool: create

Create a new empty `.said` brain file with a chosen deployment mode. Mirror of CLI [`said create`](../07-cli-reference/create.md).

## Schema

```json
{
  "name": "create",
  "arguments": {
    "file": "string, required — absolute path for the new .said file",
    "mode": "string, optional — 'portable' (default) | 'enterprise'"
  }
}
```

## Description

> Create an empty .said brain file. Use this when no brain exists yet. The empty brain is ~19 KB with just the header; you'd follow this with 'init' to bulk-ingest source, or 'remember'/'ingest' to add content piece by piece.
>
> Note: the MCP server already has a brain open (the one it was launched with). This tool creates a NEW file at the given path — you must restart the MCP server pointing at the new path to use it (or call `open` to switch in-place).

## Behavior

1. Check if the target path already exists; if populated (>32 KB), refuse overwrite
2. If the path matches the currently-attached brain, warn the caller (overwriting the live brain would desync in-memory state from disk)
3. Parse `mode` string via `BrainMode::parse`; default to Portable on missing / unknown
4. Construct via `SaidFile::create_with_mode(path, mode)` — mode is IMMUTABLE from this point
5. `save()` — produces ~19 KB file

## Safety check — populated-file refusal

```json
{"method":"tools/call","params":{"name":"create","arguments":{"file":"willie.said"}}}
```

If `willie.said` already exists and is >32 KB:

```
Refusing to overwrite willie.said — it's 19132432 bytes, which looks populated.

If this is intentional, call with `overwrite: true` (when supported) or delete the file manually first.

NOTE: this IS the brain this MCP server is attached to. Overwriting it while MCP is running will desynchronize in-memory state from disk.
```

The guard prevents accidental loss. For a fresh start, delete the file externally or use a different path.

## Example — new portable brain

```json
{"method":"tools/call","params":{"name":"create","arguments":{
  "file":"/workspace/notes.said"
}}}
```

Response:
```
Created: /workspace/notes.said (~19 KB, portable mode)
Restart the MCP server pointing at this path, or use `open` to switch.
```

## Example — new enterprise brain

```json
{"method":"tools/call","params":{"name":"create","arguments":{
  "file":"/workspace/corp-index.said",
  "mode":"enterprise"
}}}
```

Response:
```
Created: /workspace/corp-index.said (~19 KB, enterprise mode — content-embedding ingests REFUSED).
```

## Immutability reminder

Mode is locked at creation. There is no `said mode` MCP action; no backdoor to switch. See [Row 37](../05-features/row-37-brain-mode.md).

## See also

- [CLI said create](../07-cli-reference/create.md)
- [open](other-tools.md#open) — switch to an existing brain
- [Row 37 Brain mode](../05-features/row-37-brain-mode.md)
