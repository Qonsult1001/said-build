# .said

A portable, single-file **memory brain** — and a model-agnostic **coding orchestrator** built on top of it that makes *any* LLM behave like Claude Code.

Two things live in this repo:

1. **`.said` — the brain.** One file holds a project's memory: code, decisions, fixes, conversations. Semantic search is 1-bit hierarchical fingerprints (Hamming distance, ~0.3 ms/query, 500× smaller than float embeddings), fused with BM25 + symbol + grep. Four memory pillars (Episodic, Semantic, Procedural, Code). Works offline, on a USB stick, with zero external files — the encoder is baked into the binary.

2. **The orchestrator — the loop.** A separate process that drives the full Claude-Code lifecycle (plan → design → code → test → repair → learn) against *your* configured LLM, gating every change with the project's real build/test commands, and transferring verified *learning* from the brain so a weak/cheap model succeeds where it fails alone.

## The moat (measured, not claimed)

> A fix is never byte-for-byte exact across codebases — but the **learning** is. We transfer understanding; the model adapts it to *this* file; the gate verifies.

On the hard-eval suite ([`hard-eval/RESULTS.md`](hard-eval/RESULTS.md)): **gpt-oss-120b cannot solve an LRU cache cold** (RED, 5 attempts). After a verified solution + its learning is in the brain, the **same weak model solves it warm — GREEN, 1 attempt, ~13 s — on a *perturbed* repo where a verbatim paste is impossible.** Memory flips fail → pass by transferring learning, not pasting a diff. Portable memory ≈ RL-in-weights, but as a file, over any model.

## Build

The static encoder is embedded by default (`embed-model`), so semantic search works with zero external files.

```bash
# The brain CLI  -> target/release/said
cargo build --release -p said-cli

# The MCP server -> target/release/said-mcp   (docs/PDF support on by default)
cargo build --release -p said-mcp --features "code"

# The coding orchestrator -> target/release/said-orchestrate
cargo build --release -p said-orchestration
```

Optional features: `code` (tree-sitter AST chunking, all languages), `docs` (PDF/DOCX ingest — needs cmake, clang, openssl dev headers), `lsp`, `ocr`, `whisper`. See [docs/said-structure/09-cargo-features.md](docs/said-structure/09-cargo-features.md).

## Use the brain

```bash
said init                      # index the current directory into a .said brain
said ask "how does retrieval route queries?"   # the smart 3-engine router
said grep "fn build_index"     # exact text search
said sym LRUCache              # symbol lookup
said stats                     # frame count, SCA docs indexed, sizes
```

`said stats` should show `SCA docs indexed > 0` — that confirms the semantic index is built. If it's 0, the static encoder didn't load (see [docs/said-structure/03-core-subsystems/3.1-sca-engine.md](docs/said-structure/03-core-subsystems/3.1-sca-engine.md)).

## Run the orchestrator

```bash
said-orchestrate \
  --brain project.said \
  --repo  /path/to/project \
  --task  "Implement the LRUCache class per its spec; make node test/lru.test.js pass" \
  --files "src/lru.js" \
  --build "node test/lru.test.js" \
  [--test "..."] [--max-attempts 3]
```

Bring your own LLM (model-agnostic), configured via env:

- **Groq:** `GROQ_API_KEY` (+ optional `GROQ_MODEL`)
- **Any OpenAI-compatible:** `OPENAI_API_KEY` + `SAID_LLM_BASE_URL` + `SAID_LLM_MODEL`
- **Anthropic:** `ANTHROPIC_API_KEY` (+ `ANTHROPIC_MODEL`)

The build/test gate is the sole judge — the orchestrator exits non-zero unless it goes green. Full reference: [docs/said-structure/15-orchestration.md](docs/said-structure/15-orchestration.md).

## What makes it work

- **Claude-faithful apply.** Edits are anchored on exact, unique substrings — a bad anchor *fails cleanly* into the repair loop rather than silently corrupting a file (mirrors Claude Code's Edit tool). No fuzzy matching, no verbatim diff replay.
- **Semantic coding-fix recall.** One shared scorer (CLI + orchestrator) rides the `ask` chain for the candidate neighborhood, then discriminates with `.said`'s 1-bit semantic + intent fingerprints — separating near-twins (e.g. an LRU fix from an LFU fix whose text mentions "least-recently-used") that lexical overlap cannot. Measured 7/7 + 5/5 on a 9-decoy harness.
- **Learning quality is the lever.** The `learn` step stores the *non-obvious invariant*, not a generic summary — that is what lets a weak model adapt a solution correctly.

## Crates

| Crate | What it is |
|---|---|
| [`sca-core`](crates/sca-core/) | The brain: SCA engine, static encoder, FrameStore, retrieval pipeline |
| [`said-cli`](crates/said-cli/) | The `said` command-line tool |
| [`said-mcp`](crates/said-mcp/) | MCP server (tools for agents) |
| [`said-prompts`](crates/said-prompts/) | The orchestrator's phase prompts, in Claude Code's voice |
| [`said-llm`](crates/said-llm/) | BYO-LLM provider plugin (Groq / OpenAI-compatible / Anthropic / Claude-CLI) |
| [`said-orchestration`](crates/said-orchestration/) | The coding loop (plan→design→code→test→repair→learn) |
| [`said-forge`](crates/said-forge/) | Spec-driven workspace generator |
| [`said-vault`](crates/said-vault/) | Document vault (ingest, dedupe, restore) |

The `.said` library **never calls an LLM** — the orchestrator does, as a separate process (the BYO-LLM rule; see [docs/said-structure/13-integrations.md](docs/said-structure/13-integrations.md)).

## Documentation

Full blueprint: [docs/said-structure/](docs/said-structure/) — file format, retrieval engine, four pillars, features, CLI/MCP reference, benchmarks, and [orchestration](docs/said-structure/15-orchestration.md). Measured results: [hard-eval/RESULTS.md](hard-eval/RESULTS.md).

## Cloud builds

Push a `v*` tag (or run the **build-binaries** workflow manually) to get Linux / Windows / macOS binaries as release artifacts. No secrets needed — the encoder is committed here.
