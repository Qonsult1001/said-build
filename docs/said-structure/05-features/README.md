# Feature catalogue

One page per shipped feature from [`SAID_MVP_PLAN.md`](../../SAID_MVP_PLAN.md)'s success-criteria status table. Each page follows the same template:

1. **What it does** — one paragraph
2. **Where it lives** — files, modules, APIs
3. **Inputs** — required args, formats, preconditions
4. **Outputs** — return types, tags, persisted state, stdout shape
5. **How to test** — fixtures, commands, expected behaviour
6. **How to extend** — what to change to add a new variant / action
7. **Known limitations** — shipped-but-imperfect bits

## Index (rows 30–50)

### Pillar + memory model (rows 30-35)
- [Row 30 — Pillar enum on FrameMeta](row-30-pillar-enum.md) (Decision 1)
- [Row 31 — Per-pillar retrieval scope](row-31-per-pillar-retrieval.md) (Decision 2)
- [Row 32 — Episodic writer + tool hooks](row-32-episodic-writer.md) (Decision 3)
- [Row 33 — Salience scorer v0](row-33-salience.md) (Decision 4 v0)
- [Row 34 — Dream pipeline v1 (deprecated)](row-34-dream-v1.md) (Decision 5 v1)
- [Row 35 — Brain-state auto-dream (Decision 5 v2)](row-35-brain-state-dream.md)

### External pillar + deployment mode (rows 36-37)
- [Row 36 — Enterprise pointer mode](row-36-external-pointer.md)
- [Row 37 — Immutable brain deployment mode](row-37-brain-mode.md)

### Planned roadmap items (rows 38-40 — documented, not yet shipped)
- [Row 38 — Migration adapters (spec)](row-38-migration-spec.md)
- [Row 39 — Competitor benchmark sweep (spec)](row-39-competitor-sweep-spec.md)
- [Row 40 — Plugin ecosystem (spec)](row-40-plugin-spec.md)

### Surprise + admin (rows 41-44)
- [Row 41 — Surprise / reconsolidation detector](row-41-surprise.md)
- [Row 42 — Admin CLI](row-42-admin.md)
- [Row 43 — User-management UI (planned)](row-43-admin-ui.md)
- [Row 44 — MCP admin tool](row-44-mcp-admin.md)

### Audit + pillar writers + migration + bench + plugins (rows 45-50)
- [Row 45 — Audit section (AUDT) + AppGrant](row-45-audit.md)
- [Row 46 — Procedural pillar writer](row-46-procedural.md)
- [Row 47 — Code pillar writer](row-47-code.md)
- [Row 48 — Migration adapters (shipped)](row-48-migration.md)
- [Row 49 — Competitor benchmark harness](row-49-competitor-bench.md)
- [Row 50 — Plugin ecosystem trait](row-50-plugin-trait.md)

## Rows 1-28

The historical success criteria (MTEB scores, chamber tests, `said init` timing, etc.) are grouped thematically in themed pages:

- [MTEB + retrieval criteria](../10-benchmarks/mteb.md) — rows 1-6, 19, 24-27
- [Ingestion criteria](../06-ingestion-plugins/) — rows 7, 13, 21-23
- [CLI + MCP criteria](../07-cli-reference/) + [MCP reference](../08-mcp-reference/) — rows 8, 9, 10, 28
- [Operational criteria](../11-known-limitations.md) — rows 20 (single binary), 25 (relative cutoff), 29 (said watch — deferred)

(See [`SAID_MVP_PLAN.md`](../../SAID_MVP_PLAN.md) for the full row-by-row list.)
