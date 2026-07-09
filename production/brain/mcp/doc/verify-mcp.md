# verify-mcp — `brain` variant (MVP launch gate)

**Variant:** `brain` = `--no-default-features --features brain` (`embed-model` only — no `code`, `lsp`, `docs`).
**Binary:** `production/brain/said-mcp.exe` (21.5 MB, production profile).
**Build:** `cargo build --profile production -p said-mcp --no-default-features --features brain` → exit 0, 288 s.
**Transport:** MCP stdio JSON-RPC, protocol `2025-11-25`. Isolated brain (`verify-brain.said`), single session for state-across-calls.

## Result: 25 / 25 PASS ✅

### (A) Positive — the memory surface the brain bundle exists to deliver
Every memory tool works, on one session, with state carried across calls (remember → get → ask → recall_fix).

| Tool | Expected | Result |
|---|---|---|
| remember (Episodic/Semantic/Procedural) | saved | ✅ |
| get | recalls the stored note by id | ✅ |
| ask | semantic recall ("what owns the ledger" → billing/ledger) | ✅ |
| search | lexical recall | ✅ |
| salience | scores a note high/med/low | ✅ |
| list_concepts | lists (empty on a fresh brain — correct) | ✅ |
| list_tags | lists the tag vocabulary with per-tag counts (empty on a fresh brain — correct) | ✅ |
| history | version list for a memory | ✅ |
| journal | session note saved | ✅ |
| learn_fix / recall_fix | learn a fix, recall it by a differently-worded query | ✅ |
| status | shows memory count | ✅ |
| dream | consolidation runs | ✅ |
| delete (dry-run) | reports what it would remove | ✅ |

### (B) Negative-gating — tools advertised but not backed by this bundle
All respond **cleanly, no panic**. `lsp_*` give an exact "rebuild with --features lsp" message.

| Tool | Response | Clean? |
|---|---|---|
| lsp_def / refs / hover / symbols | `"said-mcp was built without the 'lsp' feature. Rebuild with cargo build -p said-mcp --features lsp"` | ✅ ideal |
| sym | `"No symbol found: Account"` (empty symbol table) | ✅ no panic |
| init (source dir) | `"Could not run said init — the CLI binary wasn't found…"` | ✅ no panic |
| discover | `"Module Discovery — Total objects: 0"` | ✅ no panic |
| harvest_blueprints | `"harvest: could not run the said CLI (set SAID_CLI?)"` | ✅ no panic |

### (C) Error paths
| Scenario | Result |
|---|---|
| get(nonexistent id) | clean not-found, no panic ✅ |
| ask(no query) | clean, no panic ✅ |
| delete(missing criteria) | `"No deletion criteria specified. Use doc_id, older_than_days, or before_date."` — helpful ✅ |
| lsp_symbols(wrong field) | `"missing field 'query'"` — clean schema rejection ✅ |

## Verdict: **GO for MVP** (memory surface), with one pre-launch polish item

The brain bundle's **memory product is solid** — every memory tool works and errors are clean. Nothing
panics. As a portable-memory MVP it passes.

## Finding — degrade quality is inconsistent (fix before a polished launch)

`brain` **advertises 38 tools** (`tools/list`), but ~10 of them need `code`/`lsp`/`docs` this bundle
lacks. They degrade three *different* ways:

1. **Ideal** — `lsp_*`: a precise "rebuild with `--features lsp`" message. A user knows exactly what to do.
2. **Confusing** — `sym` "No symbol found", `discover` "Total objects: 0": these look like *empty results*,
   not *disabled features*. A user thinks their data is missing, not that the bundle can't do this.
3. **Leaky** — `init`, `harvest_blueprints`: "could not run the said CLI (set SAID_CLI?)" — this exposes
   an *implementation detail* (it shells out to a CLI) and reads like a broken install, not a feature gate.

### Recommendation: for the brain MVP, **HIDE the code/lsp/docs tools from `tools/list`**
A memory-only bundle advertising `lsp_def`, `sym`, `harvest_blueprints` is misleading — a developer
evaluating it sees code-intelligence tools that then fail. For a clean MVP the brain surface should be
**only the memory tools** (~28), so `tools/list` is an honest menu of what actually works. Degrade-in-place
is acceptable *only if* every message is as good as the `lsp_*` one (option 1); today two of the three
classes aren't, so hiding is the safer, more sellable choice. (Deferred as a code change — this run
verifies + files the finding, per verify-mcp's boundary.)

## Reproduce
```
cargo build --profile production -p said-mcp --no-default-features --features brain
SAID_MCP_BIN=.../production/brain/said-mcp.exe node hard-eval/mcp-harness/verify-brain.js
```
Raw results: `production/brain/verify-mcp-result.json`.
