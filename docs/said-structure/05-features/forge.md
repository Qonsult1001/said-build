# said-forge — Spec-driven workspace generator

**Status:** ✅ shipped 2026-04-23

## What it does

Turns a grounded `.said` brain plus a directive document (OpenAPI or Markdown in MVP) into a per-story feature folder (`.forge/<slug>/`) and a native Claude Code skill file (`.claude/skills/<slug>/SKILL.md`). The spiritual sibling of `said snapshot card` — both materialize a folder on disk from brain frames.

`said forge load` ingests a directive (OpenAPI URL/file or Markdown checklist). `said forge run` iterates its stories, retrieves grounding from the brain, calls a BYO LLM, and projects the result to disk. Each story's artifacts are stored as `.said` frames with `forge:<type>:<hash>:<slug>` tags — so `said history` / `said checkout` / tombstones all work on the generated spec/plan/tasks.

## Where it lives

- [`crates/said-forge/`](../../../crates/said-forge/) — library + integration tests
- [`crates/said-cli/src/main.rs`](../../../crates/said-cli/src/main.rs) — `Commands::Forge` + `forge_cli` module (feature-gated)
- [`crates/said-mcp/src/tools.rs`](../../../crates/said-mcp/src/tools.rs) — 6 `Forge*Tool` structs (feature-gated)
- [`crates/said-mcp/src/handler.rs`](../../../crates/said-mcp/src/handler.rs) — 6 `handle_forge_*` dispatchers

Feature flag: `forge` on both `said-cli` and `said-mcp` (not a direct sca-core feature — see `09-cargo-features.md`).

## CLI (6 verbs)

| Verb | Purpose |
|---|---|
| `load <path-or-url> [--source <name>]` | Import a directive (OpenAPI, Markdown) — writes `forge:directive` + one `forge:story` frame per item |
| `list [--filter <expr>]` | Preview stories; filter by `method:`, `path:`, `kind:`, `tag:`, `text:` |
| `show <slug>` | Print bundled spec + plan + tasks + brain markdown for one story |
| `run [--all \| --ids <csv> \| --filter <expr>] [--force] [--yes]` | Generate per-story artifacts against configured BYO LLM |
| `status [--story <slug>]` | Inspect run state (pending / incomplete / completed) |
| `reset <slug> [--yes]` | Tombstone a story's frames + remove `.forge/<slug>/` + `.claude/skills/<slug>/` |

See `07-cli-reference/forge-*.md` for per-verb details.

## MCP (6 tools)

- **Read-only** (tagged `read_only_hint`): `forge_list`, `forge_get` (γ bundled markdown, 25k-token cap), `forge_status`.
- **Write, require `confirm: true`**: `forge_load`, `forge_run` (deferred to CLI — see below), `forge_reset`.

The `confirm: true` gate is a contract between the skill file and the AI — the skill tells Claude to ask the user first. MCP server refuses the call if confirm is false.

**`forge_run` over MCP is deferred to v2.** `std::sync::MutexGuard<SaidFile>` isn't `Send` across `.await`, and `run_one()` awaits the LLM inside the generation loop. MCP's `forge_run` returns a `said forge run …` CLI hint and asks the caller to execute from the shell. The CLI path is proven (smoke-tested with petstore).

## Frame tags written

| Tag | Purpose | Pillar |
|---|---|---|
| `forge:directive:<hash>` | Raw directive bytes + metadata | External |
| `forge:story:<hash>:<slug>` | One per extracted story | External |
| `forge:request:<hash>:<slug>:rN` | Run envelope per attempt | Memory |
| `forge:run:<hash>:<slug>:rN:input` | Audit — grounding frames retrieved | Memory |
| `forge:run:<hash>:<slug>:rN:prompt` | Audit — exact LLM prompt | Memory |
| `forge:run:<hash>:<slug>:rN:output` | Audit — raw LLM response | Memory |
| `forge:run:<hash>:<slug>:rN:meta` | Audit — token usage + validation report | Memory |
| `forge:spec:<hash>:<slug>` | Generated specification | External |
| `forge:plan:<hash>:<slug>` | Generated implementation plan | External |
| `forge:tasks:<hash>:<slug>` | Generated task list | External |
| `forge:brain:<hash>:<slug>` | Generated brain-map markdown | External |

`<hash>` is `blake3(directive_source + "v1")[..5]` in lowercase hex. `<slug>` is kebab-case, max 64 chars.

## Projection folder

```
.forge/<slug>/
├── story.md         — Spec-Kit-compatible, frontmatter: slug, kind, directive_hash
├── plan.md          — Step table with grounding frame refs
├── tasks.md         — Checklist items with per-task grounding
├── brain.md         — Navigation map: frame pointers + CLI/MCP commands
└── .forge-meta      — JSON: run_id, generated_at_utc, llm_provider, llm_model
```

## LLM provider contract

Two built-in providers (BYO key):

- **`anthropic`** — Messages API with tools-based structured output + `cache_control: ephemeral` on cacheable grounding prelude (90% prompt-cache discount across a batch).
- **`openai-compatible`** — Chat Completions with `response_format: { type: "json_schema", strict: true }`. Works against OpenAI, Azure, OpenRouter, LiteLLM, Ollama's OpenAI shim.

Config: `~/.said/config.toml` or project-local `.said/config.toml` under `[forge.llm]`.

```toml
[forge.llm]
provider = "anthropic"
model    = "claude-opus-4-7"
api_key  = "${ANTHROPIC_API_KEY}"
```

## mem0 output principles (baked into the system prompt + validators)

1. **Atomic** — each acceptance criterion / plan step / task is 1–2 sentences, 15–80 words (up to 100w/3 sent for detail-rich items with multiple proper nouns).
2. **Preserve proper nouns verbatim** — column names, endpoint paths, status codes, type names.
3. **No fabrication** — every `grounding_frame_ids` entry must exist; fabricated IDs are flagged in `forge:run:*:meta.validation`.
4. **No duplication** — Jaccard > 0.85 between peer items is a warning.
5. **`NEEDS-INPUT:` flags** — when grounding is absent for an expected pillar, the generator emits a `NEEDS-INPUT:` acceptance criterion rather than inventing.

Post-validators run after every LLM response. Retry-once on parse failure with the parse error fed back into the prompt (per spec §13.1).

## How to test

```bash
# Create brain + ingest grounding material (optional for the smoke)
said init .said
said ingest ./specs --features docs   # requirement PDFs, notes
said init  ./legacy-sql --features code   # DDD tables/procs

# Load directive + preview
said forge load crates/said-forge/fixtures/petstore.yaml
said forge list | head
said forge list --filter method:GET | head

# Run (with ANTHROPIC_API_KEY set in config.toml or env)
said forge run --ids post-pet --yes

# Results appear live
ls .forge/post-pet/               # story.md, plan.md, tasks.md, brain.md, .forge-meta
ls .claude/skills/post-pet/       # SKILL.md (Claude Code picks it up without restart)

# MVP acceptance suite
cargo test -p said-forge --features stub-llm    # 135 lib + 16 E2E = 151 tests
```

## Extension

- **Additional source adapters** (Word, Excel, CSV, plain text) — impl `DirectiveSource` in `src/source/<name>.rs`, register in `SourceRegistry::default()`.
- **Additional editor adapters** (Cursor, Copilot) — impl `EditorAdapter`, add a follow-up page here.
- **Sandbox runtime (Milestone C)** — consumes `.forge/<slug>/` + SKILL.md, spawns a worktree, runs the plan in isolation. Separate spec.

## Source

- Crate: [`crates/said-forge/`](../../../crates/said-forge/)
- Spec: [`../../superpowers/specs/2026-04-22-said-forge-design.md`](../../superpowers/specs/2026-04-22-said-forge-design.md)
- Implementation plan: [`../../superpowers/plans/2026-04-23-said-forge/`](../../superpowers/plans/2026-04-23-said-forge/)
- Fixtures: `crates/said-forge/fixtures/petstore.yaml` (20 endpoints), `requirements.md` (5 checklist items), `requirements-headings.md` (3 H2 headings)
