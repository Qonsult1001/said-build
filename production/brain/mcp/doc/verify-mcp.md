# verify-mcp — `brain` variant · v0.11.5 acceptance record

**Variant:** `brain` = `--no-default-features --features brain` (`embed-model` only — no `code`, `lsp`, `docs`).
**Binary:** `production/brain/mcp/said-mcp.exe` — `said-mcp 0.11.5`.
**Transport:** MCP stdio JSON-RPC. Isolated brain, single session (state carried across calls).

> This is a per-release acceptance record — re-run it against the shipped binary each release. The tool
> surface below is the **exact** `tools/list` of this build; if a future build changes it, update this doc.

## Result: PASS ✅ — 13 tools, all memory, all clean

### The real tool surface (`tools/list` — 13 tools)

The brain bundle advertises **only** the memory tools it can actually deliver. No `search`, `sym`,
`init`, `ingest`, `overview`, `snapshot`, `lsp_*`, or blueprint/fix tools — those are code-tier and are
**absent** from this build's `tools/list`, not merely disabled.

| Tool | What it does | Verified |
|---|---|---|
| `remember` | save a note/fact/decision as a memory | ✅ saved (memory #0, pillar) |
| `ask` | recall memories by meaning (the main command) | ✅ "when does billing run" → the billing note |
| `get` | read one memory's exact text by id | ✅ returns the stored text |
| `delete` | remove a memory (tombstoned — recoverable) | ✅ "Deleted: billing (tombstoned — preserved in history)" |
| `history` | list a memory's past versions | ✅ |
| `checkout` | restore a memory to an earlier version | ✅ |
| `status` | brain health: memory count, index, plain-English Learning line | ✅ "Brain is POPULATED… Next steps: ask/remember/get" |
| `list_concepts` | the `[[wikilink]]` concept vocabulary + counts | ✅ (empty on a fresh brain — correct) |
| `list_tags` | the `tags` metadata vocabulary + per-tag counts | ✅ returns the tag list |
| `compact` | tidy the file + reclaim space from deleted memories | ✅ dry-run: "recycle bin holds 1 deleted memory frame (38 bytes)" |
| `admin` | recycle-bin recovery: `list-tombstones`, `restore`, `who-deleted` | ✅ |
| `create` / `open` | make or switch to a brain file | ✅ |

### Prompts (`prompts/list` — 1)

| Prompt | What it is | Verified |
|---|---|---|
| `onboard` | friendly first-connect welcome (memory-specific; empty-brain = the agent-voiced first-win copy, populated = "welcome back") | ✅ |

`answerer` (the agent system prompt) and `fix-template` (an internal orchestrator template) are **not**
advertised — `answerer` is auto-injected on connect; `fix-template` is code-tier plumbing. Both remain
retrievable by name for internal callers; neither belongs in a user-facing picker.

### State-across-calls (one session)

`remember` → `get` → `ask` → `list_tags` → `status` → `delete` → `compact(dry_run)` on a **single
session**: each call sees the prior effect (the saved memory recalls; delete tombstones it; compact's
dry-run reports the 1 tombstone). ✅

### Error paths

| Scenario | Result |
|---|---|
| `get`(nonexistent id) | clean "Document not found", no panic ✅ |
| `delete`(no criteria) | helpful "No deletion criteria specified…" ✅ |
| `compact` drop_history with no scope | rejected: "drop_history needs a scope (all or keep_per_doc)" ✅ |
| a code-tier tool name (e.g. `ingest`) | clean "Unknown tool" — absent from this build ✅ |

### Tier honesty (fixed since earlier builds)

- Pasting **code** into `remember` no longer falsely promises `search` — it says "stored as text — ask
  for it later", and when the content looks like code it adds "does NOT index code — use the coding build".
- `status` next-steps and the `onboard` welcome use **memory** vocabulary only (no code-tier commands).

## Resolved finding — the tool-leak the previous verify flagged is FIXED

An earlier verify (pre-v0.11.4) found the brain build advertised ~38 tools, ~10 of which needed
`code`/`lsp`/`docs` this bundle lacks, degrading inconsistently (a leaky "set SAID_CLI?" message, empty
"No symbol found", etc.). The recommendation — **hide the code/lsp/docs tools so `tools/list` is an
honest menu** — has **shipped**: this build advertises exactly the 13 memory tools above, and the
code-tier tools are compile-gated out (`#[cfg(feature = "code")]`). `tools/list` is now a truthful menu
of what works. See [docs/said-structure/40-build-tier-capability-matrix.md](../../../../docs/said-structure/40-build-tier-capability-matrix.md).

## Reproduce

    cargo build --release -p said-mcp --no-default-features --features brain
    # then drive tools/list + tools/call over stdio JSON-RPC against the binary
    # (production/brain/mcp/said-mcp.exe), or run the mcp harness in hard-eval/mcp-harness/.
