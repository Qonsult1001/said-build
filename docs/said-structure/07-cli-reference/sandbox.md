# said sandbox

Take the output of [`said snapshot`](snapshot.md) and boot it against a real database — a throwaway SQL Server container wired up with just the module(s) you asked for.

Sandbox is the second half of the "extract a module from a monolith" workflow. Snapshot tells you which objects belong to the module; sandbox proves those objects actually run in isolation (or with other modules you co-deploy) before you cut them out of the monolith for real.

> This command is specialized to SQL schema work today. The plumbing (compose + init.d + schema render) is generic enough that it could generalize to Postgres / MySQL / MongoDB later, but the shipped shape is SQL Server 2022.

## Usage

```
said [--path FILE] [--json] sandbox <MODULE> [+MODULE...] [--port N] [--compare LABELS] [--up]
```

- `<MODULE>` — primary module (must match a snapshot target name). Required.
- `+MODULE` — additional modules to co-deploy into the **same** container for cross-module interaction testing.
- `--port N` — SQL Server host port. Default `1433`. Use a unique port per sandbox you want running simultaneously.
- `--compare v1,v2` — A/B mode: spin up one sandbox per label on adjacent ports (same module, different labels).
- `--up` — also run `docker compose up -d` after generating files. Default: generate-only.

## The three modes at a glance

| Command | What you get | Why |
|---------|-------------|-----|
| `said sandbox card` | One container, only card procs | Prove card runs alone |
| `said sandbox card +billing +fee` | One container, ALL three modules' procs deployed | Prove cross-module calls work (card proc → billing table → fee function → …) |
| `said sandbox card --compare before,after` | Two containers (ports 1433 + 1434), same module, different labels | A/B test a refactor: does anything break? |

The `+module` form is what makes sandbox more than a test-db bootstrapper — it's how you test *negative* interactions (e.g. a billing trigger firing on card-owned tables because someone forgot to scope it).

## Output layout

```
.said-code/card+billing.vivere/
├── Exclusive/                  ← inherited from snapshot
├── Shared/                     ← inherited from snapshot
├── BOUNDARY.md                 ← inherited from snapshot
├── MODULE_MAP.md               ← inherited from snapshot
├── card.vivere.said            ← inherited from snapshot (lens)
└── sandbox/                    ← NEW, written by sandbox
    ├── docker-compose.yml
    ├── schema.sql              ← full deployable schema (dependency-sorted)
    ├── seed-data.sql           ← extracted INSERT statements
    └── run.sh                  ← "bash run.sh" = full bring-up
```

Snapshot creates everything up to (and including) the lens. Sandbox adds the `sandbox/` subfolder. This means you can re-run sandbox against a snapshot without re-running snapshot. It also means `git clean -fd .said-code/card+billing.vivere/sandbox` kills the compose output without losing the extracted source.

## What goes into schema.sql

This is the core workflow. The schema is **not** just a dump of the module's frames. It is a full, FK-sound, dependency-sorted schema for *every* table/function/view the module needs to run — plus procs/triggers scoped to the selected modules.

Written by [`handle_sandbox`](../../../crates/said-mcp/src/handler.rs#L2411) in the MCP handler. The order matters:

### 1. Functions — all of them, dep-sorted

Tables have DEFAULT constraints that call functions (`DEFAULT dbo.NEXT_ID()`). If the function doesn't exist at CREATE TABLE time, the DDL fails. So functions go first.

Within the function block, functions are **topologically sorted** by their references to each other. For every function body we grep for whole-word references to any *other* function name (ignoring string literals and comments via `strip_sql_noise`). That gives `(F_PARENT → [F_CHILD1, F_CHILD2])` edges, and a Kahn-style topological walk linearises them.

### 2. Tables — all of them, FK-sorted

Every table — not just the module's — because a module's procs may reference a table from another module. FK edges come from two sources:

1. `fk:<target_table>` tags set on each frame at init time by the SQL docs plugin
2. Parse-fallback: scan the table body for `REFERENCES <ident>` and pick out the target

Combined list is deduped then topologically sorted. Tables with unsatisfied FK targets at the end of the first pass get retried in a second pass that ignores optional FKs (nullable columns) — that's the "two-pass FK" note in recent commits.

### 3. Views — dependency-sorted

Views that query other views ship in the right order.

### 4. Stored procedures — MODULE-SCOPED only

Only procs whose `doc_id` or title contain a keyword derived from the selected module list. For module `card`, keywords are `card`, `_CARD_`, `CARD_`, `_CARD` (lowercased). This is why `+billing` meaningfully expands the set: it adds `_BILLING_`/`BILLING_`/`_BILLING` to the keyword set, pulling billing procs in.

Why: this is the "dependency reduction" — if you ship every proc in the brain, the schema file is huge and it's hard to reason about what actually belongs to the sandbox.

### 5. Triggers — all of them

Triggers fire implicitly and can chain across modules (billing trigger → card-owned table). We ship them all so the sandbox reproduces the full DB behavior.

## What goes into seed-data.sql

Every doc whose content contains `INSERT INTO …` or `INSERT …` gets harvested, top-of-file to end. Meant to populate lookup tables (enums, codes, fixed rows) so procs can actually run without the full prod data.

If no INSERT frames are found, a minimal placeholder is written (`-- no seed data in brain`).

## docker-compose.yml

One-container Compose file:

```yaml
services:
  mssql:
    image: mcr.microsoft.com/mssql/server:2022-latest
    container_name: said-sbx-<combo>-<port>
    ports: [<port>:1433]
    environment:
      - ACCEPT_EULA=Y
      - MSSQL_SA_PASSWORD=Said_Test_2026!
    volumes:
      - ./schema.sql:/docker-entrypoint-initdb.d/01-schema.sql
      - ./seed-data.sql:/docker-entrypoint-initdb.d/02-seed.sql
    healthcheck:
      test: sqlcmd -S localhost -U sa -P "Said_Test_2026!" -C -Q "SELECT 1"
```

- **Container naming** — `said-sbx-<combo>-<port>` (e.g. `said-sbx-card-billing-fee-1433`). This is the key [`said clean`](other-commands.md) uses to find what to tear down.
- **Init.d volumes** — SQL Server runs scripts in `/docker-entrypoint-initdb.d/` alphabetically on first start. `01-schema.sql` before `02-seed.sql`.
- **Password is fixed** — `Said_Test_2026!`. Sandboxes are ephemeral local containers; this is not a secret.

## run.sh — idempotent bring-up

```bash
#!/bin/bash
set -e
docker compose up -d
# wait for healthcheck
until docker exec <container> sqlcmd -S … -Q 'SELECT 1' | grep -q '1 rows'; do
  sleep 2
done
# load schema
docker exec -i <container> sqlcmd … -i /docker-entrypoint-initdb.d/01-schema.sql
docker exec -i <container> sqlcmd … -i /docker-entrypoint-initdb.d/02-seed.sql
echo "Sandbox ready — port <port>, user=sa, password=Said_Test_2026!"
```

`run.sh` is safe to re-run: `docker compose up -d` is a no-op if the container is healthy; the sqlcmd loads are idempotent enough for iteration (tables come back with errors but the seed still works; re-creation of procs via `CREATE OR ALTER` where the source uses it).

Note: `--up` internally runs the equivalent of `run.sh` *but also polls health for up to 90 seconds* and counts Level-16 errors during schema load so it can tell you "N non-cascade errors from pre-existing source bugs" at the end. That's the number worth paying attention to — it's the count of objects that the monolith itself can't deploy cleanly.

## Primary ↔ secondary .said sync

A sandbox run starts by **re-opening the parent brain from disk**:

```rust
// handle_sandbox
if let Ok(fresh) = SaidFile::open(&said_path_owned) {
    *brain = fresh;
    // reload static encoder
}
```

This matters because the MCP process may have cached brain state before you ran `said snapshot card` in another terminal. Re-opening guarantees the sandbox sees the latest frames. In particular:

- A fresh ingest of `card_new_proc.sql` lands in the parent.
- You run `said snapshot card` — lens frame_ids update.
- You run `said sandbox card +billing` — MCP re-reads `vivere.said`, picks up `card_new_proc`, emits it into `schema.sql`.

No explicit "sync" step is needed. The parent `.said` is always the source of truth; the sandbox derives from it at generation time.

## Cross-module mode (`+module`)

When `all_modules.len() > 1`, every module in the list contributes its procs and seeds to the same schema. The output includes a banner:

```
CROSS-MODULE MODE — modules [card, billing, fee] are deployed into the SAME database.
Any proc from any listed module can INSERT into or trigger any other's tables.
Use this to catch negative interactions.
```

Why this matters: SQL Server's trigger firing is implicit. A `billing_insert_after` trigger fires on inserts into `billing_event` even if the caller is a `card_activate` proc. In a module-isolated sandbox (no `+billing`) that trigger never fires, so the test is misleading. `+billing` restores reality: the trigger ships, the insert path traverses it, and you see the same cascade the monolith does.

The combined container is named with `-` separators — `said-sbx-card-billing-fee-1433` — so `said clean card` still matches it via substring search.

## A/B compare mode

```
said sandbox card --compare before,after
```

Writes two adjacent sandbox directories:
```
.said-code/card-before.vivere/sandbox/
.said-code/card-after.vivere/sandbox/
```

on adjacent ports (1433 + 1434). Both contain the **same** schema (because both point at the same brain at the same moment). The comparison is interactive: you bring up `before` on 1433, apply a diff to the schema file in `after`, bring up `after` on 1434, then run identical workloads against both and compare. Outside the CLI's scope — but the adjacent-port layout is built to make it ergonomic.

`--compare` is incompatible with `+module` (ambiguous which module the label applies to).

## Export for compilation / handoff

The `sandbox/` folder is **fully self-contained**. It does **not** reference the parent brain — everything needed to reconstruct the DB is in the three files. So:

```bash
# Zip just the sandbox for another engineer
cd .said-code/card+billing.vivere/
tar czf card-billing-sandbox.tgz sandbox/
# They unpack + bring up:
tar xzf card-billing-sandbox.tgz
cd sandbox
docker compose up -d
bash run.sh
# → running SQL Server with card+billing schema, no `said` required
```

This is the workflow for handing a module to a build-engineer / DBA who doesn't use SAID. They get a deployable artifact; the brain stays with you.

## Dropbox / shared-drive workflow

Unlike snapshots (which contain a lens pointing relatively into the parent brain), sandboxes are **path-independent**. `schema.sql` and `seed-data.sql` are pure SQL; `docker-compose.yml` mounts them by relative path. So:

```
~/Dropbox/vivere-sandboxes/
├── card+billing.vivere/sandbox/
└── card.vivere/sandbox/
```

Syncs anywhere. The receiver just needs Docker + a free port. No `said` binary, no parent brain.

Caveat: the sandbox dir lives **under** the snapshot output dir, which includes the lens. If Dropbox syncs the whole `.said-code/…` tree, the lens rides along but only resolves if the parent brain is reachable at the stored path. Engineers who only need the DB can ignore the lens; engineers who want to `said ask` through the lens need the parent too.

## Brain reload + encoder re-load

Every sandbox invocation runs this preamble:

```rust
let fresh = SaidFile::open(&said_path_owned)?;
*brain = fresh;
for p in ["said-lam-static", "../said-lam-static", "SAID-LAM-private/said-lam-static"] {
    if Path::new(p).exists() { brain.load_encoder(p); break; }
}
```

— because the brain lock held by the MCP may be stale (user just re-ingested) and because loss of the encoder means `brain.get(did)` calls that need to do decode-on-read could fail.

## How to test

```bash
# Prereq: Docker Desktop running
said init ./sql-monolith --output mono.said
said --path mono.said snapshot card
said --path mono.said sandbox card --up
# → running container, schema loaded, stdout: "✓ Schema loaded (N non-cascade errors …)"

sqlcmd -S localhost,1433 -U sa -P 'Said_Test_2026!' -C -Q "SELECT COUNT(*) FROM sys.tables"
# → matches the number reported by the sandbox output

# Multi-module:
said --path mono.said sandbox card +billing +fee --up
# → one container with all three modules' procs

# Teardown:
said clean                      # stops all said-sbx-* containers
said clean card+billing+fee     # removes that one folder
```

## Known limitations

- **SQL Server-only today.** The dep-sort + compose scaffold is generic but the shipped templates are SQL Server 2022. Postgres/MySQL would need parallel templates.
- **Can't run procs without seed data.** If your monolith's seed data is in a separate `.dat` file (bcp export) and never ingested as INSERTs, the sandbox boots but dynamic procs fail on empty lookup tables. Workaround: ingest the seed as `said ingest seed.sql` first.
- **Windows volume unmount.** `said clean` retries `remove_dir_all` up to 3 times with 500 ms pacing because Docker Desktop on Windows holds the volume for a beat after `docker rm`.
- **Dependency sort is best-effort.** Comments-inside-strings and schema-qualified names (`dbo.F_X` vs `F_X`) are normalized by stripping brackets + taking the last dotted segment — works for the 99% SQL Server case but can miscategorize exotic identifiers.
- **Non-cascade errors in schema load.** The `N non-cascade errors from pre-existing source bugs` count reflects DDL that fails in a way that doesn't abort the batch. These are usually monolith-source bugs (duplicate index name, ALTER on missing object). The sandbox surfaces them rather than hiding them — acting on them is an explicit step, not part of sandbox.

## Matching MCP tool

[`sandbox`](../08-mcp-reference/other-tools.md#sandbox) — args `{modules, port?, compare?, up?, label?}`. The CLI is a thin wrapper that RPCs into the MCP handler so there's one codepath.

## See also

- [said snapshot](snapshot.md) — the prerequisite step
- [said clean](other-commands.md) — tear down containers + folders
- [`handle_sandbox`](../../../crates/said-mcp/src/handler.rs#L2411) — implementation
- [Row 46 — Procedural pillar](../05-features/row-46-procedural.md)
- Recent progress commits: `548c0c5` (functions-first + triggers = 98%+), `52eaa7f` (hybrid brain+disk + two-pass FK), `66401b7` (snapshot disk integration), `3e12fb4` (lock conflict fix + 97% deployment)
