# MCP tool: open

Switch the MCP server to a different `.said` file at runtime.

## Schema

```json
{"name": "open", "arguments": {"path": "string, required"}}
```

## Description

> Switch the MCP server's currently-attached brain to a different .said file. Useful when the server started with a placeholder brain and you want to point it at your real data — or when managing multiple brains from one agent session.

## Behavior

1. Resolve + canonicalize path
2. If the current brain is an empty placeholder (default-created on server start), auto-promote — drop the placeholder, attach to the new file
3. Open the new file via `SaidFile::open(path)` — loads SCRM, BRAN, MODE, AUDT, FTOC
4. Replace the in-memory handle (protected by the server's brain mutex)
5. Subsequent tool calls operate on the new brain

## Response

```
✓ Opened willie.said (19149 active frames, mode=enterprise)
```

## Error paths

```
Error: file not found: /missing/path.said
Error: .said magic not found or corrupt header: /bad/file
```

## Use cases

- MCP started with `./brain.said` placeholder; user wants to attach to `./willie.said`
- Switching between a personal notes brain and a work code brain mid-session
- Agent dispatching across multiple `.said` files (less common; usually one agent = one brain)

## Auto-promotion

When the currently-attached brain has zero active frames AND the target brain is populated, the server auto-promotes (logs it, drops the placeholder file). This avoids leaving a `brain.said` stub next to a real brain.

Behavior observed in MCP startup logs:
```
[brain] Auto-promoted from placeholder 'brain.said' to populated brain '.\willie.said'
```

## See also

- [create](create.md) — make a new empty brain
- [status](status.md) — see what's attached
- [CLI said use](../07-cli-reference/other-commands.md#said-use) — CLI equivalent
