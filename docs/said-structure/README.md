# .said — Complete Blueprint

This directory is the authoritative documentation for everything `.said` is and does. Everything a contributor, integrator, auditor, or curious user needs to understand the file format, retrieval engine, four-pillar memory model, feature set, plugins, and operational surface lives here.

It's split into twelve sections so pages stay focused and cross-linkable.

## Sections

1. [Overview](01-overview/) — what `.said` is, the elevator pitch, how it compares to a vector DB + SQLite combo
2. [File format blueprint](02-file-format/) — v7_1 byte layout, every section marker, frame structure, block compression, version history
3. [Core subsystems](03-core-subsystems/) — SCA engine, static encoder, brain, FrameStore, retrieval pipeline, trigram + symbol index, audit log, **latent space** (the math + measured speed)
4. [Four-pillar memory](04-four-pillars/) — Episodic, Semantic, Procedural, External, Code, Memory (what each pillar is for, how retrieval weighs them, where they're written)
5. [Feature catalogue](05-features/) — one page per shipped feature (rows 30-50 of the MVP plan) with APIs, inputs, outputs, tests, extension hooks; themed pages for rows 1-28 (historical success criteria). Newest: [forge](05-features/forge.md) — spec-driven workspace generator (shipped 2026-04-23).
6. [Ingestion plugins](06-ingestion-plugins/) — docs (PDF/DOCX/TXT/MD), OCR (PaddleOCR), whisper (video/audio), code (tree-sitter), LSP, [openapi](06-ingestion-plugins/openapi.md) (OpenAPI → forge stories)
7. [CLI reference](07-cli-reference/) — every `said <cmd>` subcommand, flags, example sessions. Forge verbs: [forge-load](07-cli-reference/forge-load.md), [forge-list](07-cli-reference/forge-list.md), [forge-show](07-cli-reference/forge-show.md), [forge-status](07-cli-reference/forge-status.md), [forge-run](07-cli-reference/forge-run.md), [forge-reset](07-cli-reference/forge-reset.md).
8. [MCP reference](08-mcp-reference/) — every MCP tool with JSON schema, example agent calls, return shapes. Forge tools: [forge-tools](08-mcp-reference/forge-tools.md) (6 tools added by `--features forge`).
9. [Cargo feature flags](09-cargo-features.md) — what each feature pulls in, default vs opt-in
10. [Benchmarks we run](10-benchmarks/) — MTEB, LoCoMo, BEIR, chambers, real-world probes
11. [Known limitations + stubs](11-known-limitations.md) — shipped-but-imperfect features with honest remediation paths
12. [Roadmap](12-roadmap.md) — planned work, gating metrics, deferred research
13. [Integrations architecture](13-integrations.md) — the two rules (offline-first; LLM in a separate process), crate inventory, `said-think`, and the LEANN-inspired Q2/Q3/Q4 plan
14. [Novel mechanisms](14-novel-mechanisms/) — 8 shipped + 5 horizon, with math, formulas, proof-sketches, and literature positioning

## Maintenance rules

- When shipping a new feature, add a row to [`../SAID_MVP_PLAN.md`](../SAID_MVP_PLAN.md) AND a page here in the same commit.
- When behaviour changes materially, update this blueprint. Row summaries in the MVP plan point here; pages here point to source files with line-precise links.
- Every feature page must have a testable "How to test" section. If you can't write one, the feature isn't done.
- Extension notes are required. A future contributor should be able to add a new pillar / adapter / plugin by reading one page.

## Related docs

- [`../SAID_MVP_PLAN.md`](../SAID_MVP_PLAN.md) — running status table (whether things exist)
- [`../SAID_FEATURE_CATALOGUE.md`](../SAID_FEATURE_CATALOGUE.md) — earlier monolithic catalogue (being migrated into this tree)
- [`../../README.md`](../../README.md) — repo root
