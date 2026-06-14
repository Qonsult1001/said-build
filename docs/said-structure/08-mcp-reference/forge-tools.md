# Forge MCP tools

Six tools exposed when `said-mcp` is built with `--features forge`. All six carry `[forge]` in their description so clients can filter visually.

| Tool | Purpose | Destructive | Read-only |
|---|---|---|---|
| `forge_list` | List stories in current directive, optional filter | no | yes |
| `forge_get` | Fetch bundled story+plan+tasks+brain markdown (γ format, 25k-token cap) | no | yes |
| `forge_status` | Per-story pending/incomplete/completed status | no | yes |
| `forge_load` | Load a new directive (requires `confirm: true`) | **yes** | no |
| `forge_run` | Batch-generate stories (requires `confirm: true`) — **MVP: deferred to CLI** | **yes** | no |
| `forge_reset` | Tombstone frames + remove projection (requires `confirm: true`) | **yes** | no |

## Ask-first convention for write tools

`forge_load`, `forge_run`, and `forge_reset` refuse calls without `confirm: true`. The Claude Code skill file written by `said-forge` instructs the AI to ask the user first and show cost/destructive impact before sending the confirm flag.

If `confirm: false`:
```
forge_load requires confirm:true — ask the user first and state which path/URL will be loaded
```

## forge_list

**Input**:
```json
{"filter": "method:GET"}   // optional
```

**Output** (JSON array):
```json
[
  {"slug":"post-pet","title":"Add a new pet to the store","kind":"api_endpoint","status":"pending","tags":["pet"]},
  ...
]
```

Filter DSL: `method:<VERB>`, `path:<glob>`, `kind:<StoryKind>`, `tag:<name>`, `text:<substr>`.

## forge_get — bundled markdown (γ format)

**Input**:
```json
{"story_ids": ["post-pet", "get-pet-petid"]}
```

**Output** (plain text / markdown):
```
# Story: post-pet

## spec — post-pet

<spec frame body>

## plan — post-pet

<plan frame body>

## tasks — post-pet

<tasks frame body>

## brain — post-pet

<brain frame body>

# Story: get-pet-petid
...
```

### Token guardrail

Combined response capped at **25 000 tokens** (approx. 100 000 chars, 4 chars/token). On overflow, each section is proportionally truncated at a UTF-8 char boundary and an explicit marker is appended:
```
[TRUNCATED — call get <frame-id> for full content]
```

Clients can always fall back to the `get <frame-id>` tool to fetch an individual frame at full fidelity.

## forge_status

**Input**:
```json
{"story_ids": ["post-pet"]}
```

**Output**:
```json
[
  {"slug":"post-pet","status":"pending","last_run_n":0,"last_error":null}
]
```

Status values: `pending` (no run attempted), `incomplete` (run attempted but didn't produce all artifacts), `completed` (all 4 artifacts + all 5 disk files present).

## forge_load

**Input**:
```json
{
  "path_or_url": "crates/said-forge/fixtures/petstore.yaml",
  "source": "openapi",         // optional
  "confirm": true              // MUST be true
}
```

**Output**:
```
loaded directive c7658 (20 stories)
```

## forge_run (deferred — returns CLI hint)

The MCP server holds `self.brain: Arc<Mutex<SaidFile>>` under a `std::sync::Mutex`. Its guard is not `Send` across `.await`. `run_one()` awaits the LLM inside the generation loop, so the full MCP-native async batch path requires `tokio::sync::Mutex` on the brain handle — an architectural change beyond the forge MVP scope.

Current behaviour: `forge_run` returns a CLI-command hint and asks the caller to run from the shell:

**Input**:
```json
{"all": true, "confirm": true}
```

**Output**:
```
forge_run via MCP is deferred to v2 (MutexGuard !Send across .await).
Please run it from the shell:

    said forge run --all --yes

All other forge MCP tools (list, get, status, load, reset) work via MCP.
```

The CLI path is fully proven — use it for generation. The MCP tools cover everything else in a session.

## forge_reset

**Input**:
```json
{"story_ids": ["post-pet"], "confirm": true}
```

**Output**:
```
tombstoned 10 frames across 1 slugs
```

## Tool count impact

- `said-mcp` default build: **25 tools**
- `said-mcp --features forge`: **31 tools** (25 + 6)

Verified end-to-end via stdio JSON-RPC probe:
```
initialize → 31 tools, forge_* present
tools/call forge_list → 20 stories
tools/call forge_list {"filter":"method:GET"} → 8 stories
tools/call forge_status {"story_ids":["post-pet"]} → "pending"
tools/call forge_run {"confirm":false} → refused (confirm gate)
tools/call forge_run {"confirm":true} → CLI hint (deferred)
```

## Source

- [`crates/said-forge/src/mcp_api.rs`](../../../crates/said-forge/src/mcp_api.rs) — `list_stories`, `get_bundled` (token-capped), `story_status`
- [`crates/said-mcp/src/tools.rs`](../../../crates/said-mcp/src/tools.rs) — 6 `#[mcp_tool]` structs
- [`crates/said-mcp/src/handler.rs`](../../../crates/said-mcp/src/handler.rs) — `handle_forge_*` dispatchers
