# verify-cli — `brain` variant · v0.11.9 acceptance record

**Variant:** `brain` = `--no-default-features --features brain` (`embed-model` only — no `code`, `lsp`, `docs`).
**Binary:** `production/brain/cli/said.exe` — `said 0.11.9`.
**How:** run each command against an isolated brain file and check the observable output.

> Per-release acceptance record — re-run against the shipped binary each release. This is the CLI
> counterpart to [`../../mcp/doc/verify-mcp.md`](../../mcp/doc/verify-mcp.md); the two surfaces share a
> core (`sca_core::ask::ask`) but differ in argv/stdout vs JSON-RPC, so both are verified.

## Result: PASS ✅ — the memory command surface, all clean

### The real command surface (`said --help`)

The brain CLI ships **only** memory commands. No `sym`, `init`, `sync`, `lsp-*`, `overview`, `snapshot`,
`sandbox` — those are code-tier and are **absent** from this build (compile-gated).

| Command | What it does | Verified |
|---|---|---|
| `create <file>.said` | Make a new brain file | ✅ `Created: … (mode: portable, immutable)` |
| `add "<note>" [--id --title --tag]` | Store a memory (aka `remember`) | ✅ `Added 'billing' (26 bytes)` |
| `get <id>` | Read one memory verbatim | ✅ returns the exact text |
| `ask "<q>" [--tag --pillar]` | Recall by meaning — the main command | ✅ ranked results, tags shown |
| `delete <id>` | Tombstone a memory (recoverable) | ✅ `Deleted: billing` |
| `history <id>` | Past versions of a memory | ✅ `versions: 2` after a same-id update |
| `checkout <id> --version N` | Roll a memory back | ✅ |
| `stats [--verbose]` | Memory count + index + plain-English Learning line | ✅ |
| `list-concepts [--prefix]` | The `[[wikilink]]` concept vocabulary | ✅ (footer points to `list-tags`) |
| `list-tags [--prefix]` | The `tags` metadata vocabulary + counts | ✅ `1  topic:ops` |
| `compact [--drop-history --all/--keep N]` | Tidy the file / reclaim space from deleted memories | ✅ `Compacted: … bytes saved` |
| `admin <restore/list-tombstones/who-deleted>` | Recycle-bin recovery | ✅ `✓ Recovered memory 'billing'.` |
| `import` | Bring memories in from mem0 / others | ✅ |
| `use <file>.said` | Set the default brain (skip `--path`) | ✅ |
| `save-memory` / `recall-memory` / `memory-manifest` | Claim→evidence memory (doc-31) | ✅ |
| `hook` / `setup` | Agent-steering hook (opt-in) | ✅ |

### Recall UX (v0.11.9 — CLI/MCP parity)

| Behavior | Verified |
|---|---|
| `--tag ns:value` on `add` attaches a browsable/filterable tag | ✅ `list-tags` → `topic:ops` |
| `ask` result lines show user-facing `tags:` (internals filtered) | ✅ |
| Vague query, flat cluster (no clear leader) → tie footer with distinguishing tag counts | ✅ `3 close matches … scope to a tag` |
| Clear leader (gap to #2 > 0.03) → NO footer (not chatty on obvious queries) | ✅ |
| `ask "<q>" --tag quarter:Q2` narrows the pool before scoring | ✅ |

### Tier honesty

| Behavior | Verified |
|---|---|
| `add` says "Stored as text — recall with `said ask`" (not code-search) | ✅ |
| `add` of a code snippet appends the honesty note | ✅ `Note: memory brain only — snippet kept as text, not indexed as code. Use the coding build…` |
| Second-brain `create` on the free build is refused (one brain per PC; multi = Enterprise) | ✅ Enterprise upsell |
| `stats` "Search index" reports the semantic index honestly (not "102 of 36"); "Learning" line in plain English | ✅ |

### Error paths

| Scenario | Result |
|---|---|
| `get <nonexistent>` | clean not-found, no panic ✅ |
| `delete` with no id/criteria | helpful message ✅ |
| `compact --drop-history` with no scope | rejected: requires `--all` or `--keep N` ✅ |
| a code-tier subcommand (e.g. `sym`) | `error: unrecognized subcommand` — absent from this build ✅ |

## Reproduce

    cargo build --release -p said-cli --no-default-features --features brain
    # then run the commands above against an isolated .said file and check the observables.

See [`../../mcp/doc/verify-mcp.md`](../../mcp/doc/verify-mcp.md) for the MCP-surface equivalent.
