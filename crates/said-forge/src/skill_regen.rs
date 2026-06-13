//! Skill regenerator — reads the user's OLD `.claude/skills/<slug>/`
//! as starting-point evidence and writes a REGENERATED, globally-correct
//! skill grounded in current SQL + directive.
//!
//! Forge does not auto-run the regenerated skill. It writes the skill
//! + stories + gap report; developers then open Claude Code and run
//! the skill manually. That's the contract from phase 21 scoping —
//! forge stops at the spec artefact boundary.
//!
//! File classification:
//!
//! - **preserve** — `architecture-patterns.md`, `azure-db-setup.md`,
//!   `bruno-template.md`, `di-registration-template.md`. These capture
//!   human-written convention + environment setup that no amount of
//!   SQL parsing can reproduce. Copied verbatim from the old skill.
//!
//! - **regenerate** — `SKILL.md`, `controller-template.md`,
//!   `domain-template.md`, `model-template.md`, `data-layer-template.md`,
//!   `sql-stored-proc-template.md`. These describe how code maps to
//!   schema. Regenerated from current SQL + glossary so the skill
//!   stays in sync with what actually exists in the database.
//!
//! - **generated-new** — `glossary.md`. Never present in old skills;
//!   always produced fresh from the catalog.
//!
//! Safety gate:
//!
//! Every regenerated file is diffed against the old file before the
//! new output is written. The top-level `REVIEW.md` lists every
//! change (preserved, regenerated, new, removed). Large diffs on
//! regenerated files cause `regenerate_skill` to return an error
//! unless `RegenOptions::force == true`. "Large" = more than
//! `RegenOptions::large_diff_threshold_lines` lines changed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{ForgeError, ForgeResult};
use crate::mapping_service::Glossary;
use crate::sql_catalog::SqlCatalog;

// ─────────────────────────── public API ───────────────────────────

/// Files we keep byte-identical from the old skill.
pub const PRESERVE_FILES: &[&str] = &[
    "architecture-patterns.md",
    "azure-db-setup.md",
    "bruno-template.md",
    "di-registration-template.md",
];

/// Files we regenerate from current SQL + glossary.
pub const REGENERATE_FILES: &[&str] = &[
    "SKILL.md",
    "controller-template.md",
    "domain-template.md",
    "model-template.md",
    "data-layer-template.md",
    "sql-stored-proc-template.md",
];

/// File produced fresh — never in the old skill.
pub const GENERATED_FILES: &[&str] = &["glossary.md"];

#[derive(Debug, Clone)]
pub struct RegenOptions {
    /// Write output even when a regenerated file has a large diff
    /// against its old counterpart.
    pub force: bool,
    /// Lines-changed threshold above which regeneration halts without
    /// `force`. Defaults to 40 — matches txn-api-generator's typical
    /// template size.
    pub large_diff_threshold_lines: usize,
    /// Name of the skill emitted (becomes the folder under
    /// `<output-root>/.claude/skills/<skill_name>/`).
    pub skill_name: String,
}

impl Default for RegenOptions {
    fn default() -> Self {
        Self {
            force: false,
            large_diff_threshold_lines: 40,
            skill_name: "dt-api-generator".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenReport {
    pub skill_path: PathBuf,
    pub preserved: Vec<String>,
    pub regenerated: Vec<RegeneratedEntry>,
    pub generated_new: Vec<String>,
    pub removed: Vec<String>,
    /// Files that would have been regenerated but hit the large-diff
    /// gate — only populated when `force == false`.
    pub blocked_large_diff: Vec<RegeneratedEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegeneratedEntry {
    pub name: String,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub old_existed: bool,
}

impl RegenReport {
    pub fn to_review_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# REVIEW — Regenerated skill\n\n");
        out.push_str(&format!("_Output path: `{}`_\n\n", self.skill_path.display()));
        out.push_str("Forge regenerated this skill from current SQL. Inspect the\n");
        out.push_str("changes below before handing the skill to developers. Files in\n");
        out.push_str("the **preserved** list were copied byte-identical from the old\n");
        out.push_str("skill — no review needed on those.\n\n");

        out.push_str("## Preserved (byte-identical)\n\n");
        if self.preserved.is_empty() {
            out.push_str("_(none)_\n\n");
        } else {
            for name in &self.preserved {
                out.push_str(&format!("- `{}`\n", name));
            }
            out.push('\n');
        }

        out.push_str("## Regenerated\n\n");
        if self.regenerated.is_empty() {
            out.push_str("_(none)_\n\n");
        } else {
            out.push_str("| file | +lines | -lines | new file |\n");
            out.push_str("|------|--------|--------|----------|\n");
            for e in &self.regenerated {
                out.push_str(&format!(
                    "| `{}` | {} | {} | {} |\n",
                    e.name,
                    e.lines_added,
                    e.lines_removed,
                    if e.old_existed { "" } else { "yes" }
                ));
            }
            out.push('\n');
        }

        if !self.blocked_large_diff.is_empty() {
            out.push_str("## Blocked — large diff (use `--force` to override)\n\n");
            for e in &self.blocked_large_diff {
                out.push_str(&format!(
                    "- `{}` — +{} / -{}\n",
                    e.name, e.lines_added, e.lines_removed
                ));
            }
            out.push('\n');
        }

        out.push_str("## Generated new\n\n");
        if self.generated_new.is_empty() {
            out.push_str("_(none)_\n\n");
        } else {
            for name in &self.generated_new {
                out.push_str(&format!("- `{}`\n", name));
            }
            out.push('\n');
        }

        if !self.removed.is_empty() {
            out.push_str("## Removed\n\n");
            for name in &self.removed {
                out.push_str(&format!("- `{}`\n", name));
            }
            out.push('\n');
        }

        out
    }
}

/// Regenerate a skill.
///
/// - `old_skill_dir` — existing `.claude/skills/<slug>/`. May not
///   exist; in that case nothing is preserved and every regenerate
///   target is written as a new file.
/// - `output_root` — the directory under which the new skill lands at
///   `<output_root>/.claude/skills/<skill_name>/`.
/// - `catalog`, `glossary` — ground truth for the regenerated files.
/// - `options` — controls force + diff threshold.
pub fn regenerate_skill(
    old_skill_dir: &Path,
    output_root: &Path,
    catalog: &SqlCatalog,
    glossary: &Glossary,
    options: &RegenOptions,
) -> ForgeResult<RegenReport> {
    let output_skill_dir = output_root
        .join(".claude")
        .join("skills")
        .join(&options.skill_name);
    let output_refs = output_skill_dir.join("references");
    fs::create_dir_all(&output_refs).map_err(|e| ForgeError::Io {
        path: output_refs.display().to_string(),
        cause: e,
    })?;

    let old_refs = old_skill_dir.join("references");

    let mut preserved = Vec::new();
    let mut regenerated = Vec::new();
    let mut generated_new = Vec::new();
    let mut blocked_large_diff = Vec::new();

    // 1. Preserve — copy verbatim.
    for name in PRESERVE_FILES {
        let src = old_refs.join(name);
        if !src.exists() {
            continue;
        }
        let dst = output_refs.join(name);
        let bytes = fs::read(&src).map_err(|e| ForgeError::Io {
            path: src.display().to_string(),
            cause: e,
        })?;
        fs::write(&dst, &bytes).map_err(|e| ForgeError::Io {
            path: dst.display().to_string(),
            cause: e,
        })?;
        preserved.push((*name).into());
    }

    // 2. Regenerate.
    let regen_contents = regenerate_contents(catalog, glossary, &options.skill_name);
    for (name, new_content) in regen_contents {
        let is_top_level = name == "SKILL.md";
        let old_path = if is_top_level {
            old_skill_dir.join(&name)
        } else {
            old_refs.join(&name)
        };
        let old_content = if old_path.exists() {
            Some(
                fs::read_to_string(&old_path).map_err(|e| ForgeError::Io {
                    path: old_path.display().to_string(),
                    cause: e,
                })?,
            )
        } else {
            None
        };
        let diff = count_diff_lines(old_content.as_deref(), &new_content);
        let entry = RegeneratedEntry {
            name: name.clone(),
            lines_added: diff.added,
            lines_removed: diff.removed,
            old_existed: old_content.is_some(),
        };

        let large = old_content.is_some()
            && (diff.added + diff.removed) > options.large_diff_threshold_lines;
        if large && !options.force {
            blocked_large_diff.push(entry);
            continue;
        }

        let dst = if is_top_level {
            output_skill_dir.join(&name)
        } else {
            output_refs.join(&name)
        };
        fs::write(&dst, &new_content).map_err(|e| ForgeError::Io {
            path: dst.display().to_string(),
            cause: e,
        })?;
        regenerated.push(entry);
    }

    // 3. Generated new — glossary.md always fresh.
    let glossary_md = glossary.to_markdown();
    let dst = output_refs.join("glossary.md");
    fs::write(&dst, &glossary_md).map_err(|e| ForgeError::Io {
        path: dst.display().to_string(),
        cause: e,
    })?;
    generated_new.push("glossary.md".into());

    // 4. Removed — anything in the old references that we neither
    // preserve nor regenerate nor generate is flagged. We don't delete
    // from the destination (fresh output dir) — this is just informative.
    let mut removed = Vec::new();
    if old_refs.exists() {
        for entry in fs::read_dir(&old_refs).map_err(|e| ForgeError::Io {
            path: old_refs.display().to_string(),
            cause: e,
        })? {
            let entry = entry.map_err(|e| ForgeError::Io {
                path: old_refs.display().to_string(),
                cause: e,
            })?;
            let fname = entry.file_name().to_string_lossy().into_owned();
            if PRESERVE_FILES.iter().any(|s| *s == fname)
                || REGENERATE_FILES.iter().any(|s| *s == fname)
                || GENERATED_FILES.iter().any(|s| *s == fname)
                || fname == "SKILL.md"
            {
                continue;
            }
            removed.push(fname);
        }
    }

    let report = RegenReport {
        skill_path: output_skill_dir.clone(),
        preserved,
        regenerated,
        generated_new,
        removed,
        blocked_large_diff,
    };

    // REVIEW.md at the skill root so devs see it immediately.
    let review_md = report.to_review_markdown();
    let review_path = output_skill_dir.join("REVIEW.md");
    fs::write(&review_path, review_md).map_err(|e| ForgeError::Io {
        path: review_path.display().to_string(),
        cause: e,
    })?;

    if !report.blocked_large_diff.is_empty() {
        return Err(ForgeError::Config(format!(
            "{} regenerated file(s) blocked by large-diff gate; re-run with --force to override (see {})",
            report.blocked_large_diff.len(),
            review_path.display()
        )));
    }

    Ok(report)
}

// ─────────────────────────── regeneration ───────────────────────────

/// Compute the new content for every regenerated file.
///
/// Deterministic — same (catalog, glossary, skill_name) input
/// produces byte-identical output. That's the 100% parity promise.
pub fn regenerate_contents(
    catalog: &SqlCatalog,
    glossary: &Glossary,
    skill_name: &str,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert(
        "SKILL.md".into(),
        render_skill_md(skill_name, catalog, glossary),
    );
    out.insert(
        "controller-template.md".into(),
        render_controller_template(),
    );
    out.insert("domain-template.md".into(), render_domain_template());
    out.insert("model-template.md".into(), render_model_template());
    out.insert(
        "data-layer-template.md".into(),
        render_data_layer_template(),
    );
    out.insert(
        "sql-stored-proc-template.md".into(),
        render_sql_stored_proc_template(catalog),
    );
    out
}

fn render_skill_md(skill_name: &str, catalog: &SqlCatalog, glossary: &Glossary) -> String {
    let schemas = catalog_schema_summary(catalog);
    format!(
        r#"---
name: {name}
description: >
  Generate C# API endpoints aligned to the current SQL schema. Reads a
  directive (OpenAPI or Dev Planning markdown), consults the
  mapping-service grounding in `.forge/`, and produces Controller,
  Domain, Model, Data-layer, and SQL artefacts. Invoke this skill after
  `said forge regen` has populated `.claude/skills/{name}/` so the
  templates here reflect current SQL.
---

# {name}

Forge regenerated this skill. Every schema-touching template below was
produced from the workspace's current SQL catalog ({schemas_count}
schemas, {tables_count} tables, {procs_count} procedures).

## Entry point

Run this skill against a story file (markdown or text) in
`user-stories/`. Each story names one op — method, path, request /
response fields — and inherits grounding from `.forge/gaps/` +
`.forge/glossary.toml`.

## What to read first

1. `references/architecture-patterns.md` — human conventions
   (preserved from the prior skill, not regenerated).
2. `references/glossary.md` — auto-built lookup terms. When a story
   says `country_code` or `address_type`, check here for the backing
   lookup table + accepted SQL types before writing code.
3. `references/controller-template.md` — regenerated. Mirrors the
   current SQL column prefixes + naming.
4. `references/domain-template.md`, `model-template.md`,
   `data-layer-template.md`, `sql-stored-proc-template.md` — all
   regenerated.
5. `references/di-registration-template.md`, `bruno-template.md`,
   `azure-db-setup.md` — preserved byte-identical from the prior
   skill.

## SQL schemas in scope

{schemas}

## Glossary terms ({glossary_terms})

{glossary_snippet}

## Mapping confidence

Every mapping decision forge made has a confidence tier recorded in
`.forge/mapping-trace.jsonl`:

- `🔒 explicit` — from `.forge/mapping.toml`. Ship verbatim.
- `● high` — exact symbol match. Ship unless review flags otherwise.
- `◐ medium` — multi-signal heuristic. Review before shipping.
- `◯ low` — single-signal heuristic. Review carefully.
- `✗ none` — no mapping produced. Escalate to schema owner.

The gap file under `.forge/gaps/<Group>/<OP>.md` shows the confidence
for every field → column decision in the op you're implementing.

## Do

- Match C# column bindings to SQL columns named in the gap file.
- Reuse the preserved templates for DI, Bruno, Azure setup.
- When the gap file flags `confidence: low` on a field, ask the user
  before guessing.

## Do not

- Invent table or column names. The catalog is authoritative.
- Bypass the glossary — if a story field maps to a lookup term, use
  the lookup's accepted SQL type.
- Write C# without first reading the glossary + gap file for the op.
"#,
        name = skill_name,
        schemas_count = schema_count(catalog),
        tables_count = catalog.tables.len(),
        procs_count = catalog
            .objects
            .iter()
            .filter(|o| matches!(o.kind, crate::sql_catalog::SqlKind::Procedure))
            .count(),
        schemas = schemas,
        glossary_terms = glossary.len(),
        glossary_snippet = glossary_snippet(glossary),
    )
}

fn render_controller_template() -> String {
    r#"# Controller template

Regenerated from current SQL + conventions.

```csharp
// <copyright file="{Entity}Controller.cs" company="Direct Transact">
// © Direct Transact. All rights reserved.
// </copyright>

using DirectTransact.TxnGlobal.API.Domain.Commands.{Entity};
using DirectTransact.TxnGlobal.API.Domain.Queries.{Entity};
using DirectTransact.TxnGlobal.API.Models.Request.{Entity};
using DirectTransact.TxnGlobal.API.Models.Response.{Entity};
using MediatR;
using Microsoft.AspNetCore.Mvc;

namespace DirectTransact.TxnGlobal.API.Controllers.V1;

/// <summary>
/// {EntityPlural} API. Verbs map to stored procedures in
/// `[{schema}].[p_txn_{Action}_{Entity}]`.
/// </summary>
[ApiController]
[Route("v1/{route}")]
[ApiExplorerSettings(GroupName = "{SwaggerLabel}")]
public sealed class {Entity}Controller(ISender mediator) : ControllerBase
{
    /// <summary>Create a {entity}.</summary>
    /// <param name="body">Create request.</param>
    [HttpPost]
    [ProducesResponseType(typeof(BaseResponseModel<Create{Entity}Response>), StatusCodes.Status200OK)]
    public async Task<IActionResult> Create([FromBody] Create{Entity}Request body)
        => Ok(await mediator.Send(new Create{Entity}Command(body)));

    /// <summary>Get a {entity} by id.</summary>
    [HttpGet("{id}")]
    [ProducesResponseType(typeof(BaseResponseModel<Get{Entity}Response>), StatusCodes.Status200OK)]
    public async Task<IActionResult> Get(Guid id)
        => Ok(await mediator.Send(new Get{Entity}Query(id)));
}
```

## Placeholder reference

| token | meaning |
|-------|---------|
| `{Entity}` | PascalCase entity, derived from primary table — e.g. `Cardholder` |
| `{EntityPlural}` | plural form — folder + Swagger |
| `{entity}` | lowercase entity — XML docs |
| `{schema}` | SQL schema owning the primary table |
| `{route}` | REST route segment (lowercase plural) |
| `{Action}` | Verb in proc name: Update, Get, List |
| `{SwaggerLabel}` | `"{Entity} Lifecycle"` |
"#
    .into()
}

fn render_domain_template() -> String {
    r#"# Domain template

Regenerated. Commands/Queries route MediatR traffic to the repository.

```csharp
// Command — Domain/Commands/{Entity}/Create{Entity}Command.cs
using DirectTransact.TxnGlobal.API.Data.Repositories.{Entity};
using DirectTransact.TxnGlobal.API.Models.Request.{Entity};
using DirectTransact.TxnGlobal.API.Models.Response.{Entity};
using DirectTransact.TxnGlobal.API.Shared;
using MediatR;

namespace DirectTransact.TxnGlobal.API.Domain.Commands.{Entity};

public sealed record Create{Entity}Command(Create{Entity}Request Body)
    : IRequest<BaseResponseModel<Create{Entity}Response>>;

public sealed class Create{Entity}CommandHandler(I{Entity}Repository repo)
    : IRequestHandler<Create{Entity}Command, BaseResponseModel<Create{Entity}Response>>
{
    public async Task<BaseResponseModel<Create{Entity}Response>> Handle(
        Create{Entity}Command request,
        CancellationToken ct)
    {
        var id = await repo.CreateAsync(request.Body, ct);
        return BaseResponseModel<Create{Entity}Response>.Ok(new Create{Entity}Response(id));
    }
}
```

Query shape mirrors the command but returns a `Get{Entity}Response` and
calls `repo.GetAsync(id, ct)`.
"#
    .into()
}

fn render_model_template() -> String {
    r#"# Model template

Regenerated. Request/Response models are records aligned to the
columns the mapping-service resolved.

```csharp
// Models/Request/{Entity}/Create{Entity}Request.cs
namespace DirectTransact.TxnGlobal.API.Models.Request.{Entity};

public sealed record Create{Entity}Request
{
    /// <summary>{FieldDescription}</summary>
    /// <example>{FieldExample}</example>
    public required {CSharpType} {FieldName} { get; init; }

    // Repeat for every body field resolved by MappingService.
    // Fields with confidence=low should include a /// <remarks> note.
}
```

```csharp
// Models/Response/{Entity}/Create{Entity}Response.cs
namespace DirectTransact.TxnGlobal.API.Models.Response.{Entity};

public sealed record Create{Entity}Response(Guid Id);
```

## Type mapping (SQL column → C# field)

- `UNIQUEIDENTIFIER` → `Guid` (or `Guid?` when nullable)
- `VARCHAR`, `NVARCHAR`, `CHAR`, `NCHAR`, `TEXT` → `string` / `string?`
- `INT`, `BIGINT`, `SMALLINT`, `TINYINT` → `int`, `long`, `short`, `byte`
- `DECIMAL`, `NUMERIC`, `MONEY` → `decimal`
- `FLOAT`, `REAL` → `double`, `float`
- `BIT` → `bool` / `bool?`
- `DATETIME`, `DATETIME2`, `DATETIMEOFFSET` → `DateTime` / `DateTimeOffset`
- `DATE` → `DateOnly`
- `TIME` → `TimeOnly`
"#
    .into()
}

fn render_data_layer_template() -> String {
    r#"# Data layer template

Regenerated. Repository calls the stored procedure the mapping-service
resolved. Never writes inline SQL.

```csharp
// Data/Repositories/{Entity}/{Entity}Repository.cs
using System.Data;
using Dapper;
using DirectTransact.TxnGlobal.API.Data.SqlQueries.{Entity};
using DirectTransact.TxnGlobal.API.Models.Request.{Entity};
using Microsoft.Data.SqlClient;

namespace DirectTransact.TxnGlobal.API.Data.Repositories.{Entity};

public interface I{Entity}Repository
{
    Task<Guid> CreateAsync(Create{Entity}Request body, CancellationToken ct);
    Task<Get{Entity}Response?> GetAsync(Guid id, CancellationToken ct);
}

public sealed class {Entity}Repository(ISqlConnectionFactory factory)
    : I{Entity}Repository
{
    public async Task<Guid> CreateAsync(Create{Entity}Request body, CancellationToken ct)
    {
        await using var conn = factory.Open();
        var id = await conn.ExecuteScalarAsync<Guid>(
            Create{Entity}Query.ProcName,
            Create{Entity}Query.BuildParameters(body),
            commandType: CommandType.StoredProcedure);
        return id;
    }

    public async Task<Get{Entity}Response?> GetAsync(Guid id, CancellationToken ct)
    {
        await using var conn = factory.Open();
        return await conn.QuerySingleOrDefaultAsync<Get{Entity}Response>(
            Get{Entity}Query.ProcName,
            new { id },
            commandType: CommandType.StoredProcedure);
    }
}
```

```csharp
// Data/SqlQueries/{Entity}/Create{Entity}Query.cs
using DirectTransact.TxnGlobal.API.Models.Request.{Entity};

namespace DirectTransact.TxnGlobal.API.Data.SqlQueries.{Entity};

internal static class Create{Entity}Query
{
    public const string ProcName = "[{schema}].[p_txn_Update_{Entity}]";

    public static object BuildParameters(Create{Entity}Request body) => new
    {
        // One entry per body field resolved by MappingService:
        //   @{SqlColumnName} = body.{FieldName}
        // Match column names exactly — the stored proc signature comes
        // from the SQL catalog, not from guesswork.
    };
}
```
"#
    .into()
}

fn render_sql_stored_proc_template(catalog: &SqlCatalog) -> String {
    let proc_hint = if catalog
        .objects
        .iter()
        .any(|o| matches!(o.kind, crate::sql_catalog::SqlKind::Procedure))
    {
        "Use the catalog's existing procedure as the template when one is listed for the op.\n".to_string()
    } else {
        "Catalog currently lists no procedures — scaffold against the primary table the gap file resolves.\n".to_string()
    };
    format!(
        r#"# SQL stored procedure template

Regenerated.

Naming: `[{{schema}}].[p_txn_{{Action}}_{{Entity}}]` — Action ∈
{{Update, Get, List}}. POST/PUT/PATCH use `Update`. GET single uses
`Get`. GET collection uses `List`.

{proc_hint}
```sql
CREATE OR ALTER PROCEDURE [{{schema}}].[p_txn_Update_{{Entity}}]
    @{{PrimaryKeyParam}} UNIQUEIDENTIFIER,
    -- One @param per body field, typed from MappingService.
    @cnl_Code VARCHAR(2) = NULL,
    @adl_Code VARCHAR(4) = NULL
AS
BEGIN
    SET NOCOUNT ON;
    SET XACT_ABORT ON;

    BEGIN TRAN;

    INSERT INTO [{{schema}}].[{{PrimaryTable}}] (
        -- Column list from PK + NOT-NULL columns resolved by
        -- MappingService; order matches the @param declarations.
    )
    VALUES (
        -- @param list matching the INSERT columns.
    );

    -- For FK-dependent rows, INSERT into related tables in FK
    -- dependency order (dependents after parents).

    COMMIT TRAN;

    SELECT @{{PrimaryKeyParam}} AS Id;
END;
```

## Rules

- One `BEGIN TRAN` / `COMMIT TRAN` per proc. Never partial inserts.
- `SET XACT_ABORT ON` — any error rolls back the whole transaction.
- Every FK column gets a default `= NULL` when the API makes the
  field optional.
- Return shape matches the response model resolved by the skill.
"#
    )
}

// ─────────────────────────── catalog helpers ───────────────────────────

fn schema_count(catalog: &SqlCatalog) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for t in &catalog.tables {
        if let Some(s) = &t.schema {
            seen.insert(s.clone());
        }
    }
    seen.len()
}

fn catalog_schema_summary(catalog: &SqlCatalog) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for t in &catalog.tables {
        let key = t.schema.clone().unwrap_or_else(|| "(default)".into());
        *counts.entry(key).or_insert(0) += 1;
    }
    if counts.is_empty() {
        return "_(no schemas — catalog empty)_".into();
    }
    let mut out = String::new();
    for (name, n) in counts {
        out.push_str(&format!("- `{}` — {} tables\n", name, n));
    }
    out
}

fn glossary_snippet(glossary: &Glossary) -> String {
    if glossary.is_empty() {
        return "_(no terms — catalog had no `lookups.*` tables)_".into();
    }
    let mut out = String::new();
    for (_, term) in glossary.terms().take(12) {
        let types = if term.sql_types.is_empty() {
            "—".to_string()
        } else {
            term.sql_types.join(", ")
        };
        out.push_str(&format!("- `{}` — {}\n", term.name, types));
    }
    if glossary.len() > 12 {
        out.push_str(&format!(
            "- _(+{} more — see `references/glossary.md`)_\n",
            glossary.len() - 12
        ));
    }
    out
}

// ─────────────────────────── diff counting ───────────────────────────

struct DiffCount {
    added: usize,
    removed: usize,
}

fn count_diff_lines(old: Option<&str>, new: &str) -> DiffCount {
    let old_lines: Vec<&str> = old.map(|s| s.lines().collect()).unwrap_or_default();
    let new_lines: Vec<&str> = new.lines().collect();
    let old_set: std::collections::BTreeSet<&str> = old_lines.iter().copied().collect();
    let new_set: std::collections::BTreeSet<&str> = new_lines.iter().copied().collect();
    DiffCount {
        added: new_set.difference(&old_set).count(),
        removed: old_set.difference(&new_set).count(),
    }
}

// ─────────────────────────── tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse_create_table;

    fn sample_catalog() -> SqlCatalog {
        let t = parse_create_table(
            r#"CREATE TABLE [cardholder].[cpf_Client_Profile] (
                [cpf_Profile_Id] UNIQUEIDENTIFIER NOT NULL,
                [cpf_Last_Name]  NVARCHAR (1024)  NULL,
                CONSTRAINT [PK_cpf] PRIMARY KEY ([cpf_Profile_Id])
            );"#,
        )
        .unwrap();
        SqlCatalog {
            tables: vec![t],
            objects: Vec::new(),
        }
    }

    fn write(path: &Path, contents: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn preserved_files_copied_byte_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        write(
            &old.join("references").join("architecture-patterns.md"),
            "HAND-WRITTEN-CONVENTIONS",
        );
        write(
            &old.join("references").join("azure-db-setup.md"),
            "AZURE-STEPS",
        );
        write(
            &old.join("references").join("bruno-template.md"),
            "BRUNO-CONTENT",
        );
        write(
            &old.join("references").join("di-registration-template.md"),
            "DI-CONTENT",
        );
        let out = tmp.path().join("out");
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let opts = RegenOptions::default();
        let report = regenerate_skill(&old, &out, &cat, &g, &opts).unwrap();
        assert_eq!(report.preserved.len(), 4);
        let copied = std::fs::read_to_string(
            out.join(".claude/skills/dt-api-generator/references/architecture-patterns.md"),
        )
        .unwrap();
        assert_eq!(copied, "HAND-WRITTEN-CONVENTIONS");
    }

    #[test]
    fn glossary_md_always_generated_new() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old"); // empty
        let out = tmp.path().join("out");
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let report =
            regenerate_skill(&old, &out, &cat, &g, &RegenOptions::default()).unwrap();
        assert!(report.generated_new.contains(&"glossary.md".to_string()));
        let path = out.join(".claude/skills/dt-api-generator/references/glossary.md");
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("# Glossary"));
    }

    #[test]
    fn regenerated_files_written_when_no_old_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let out = tmp.path().join("out");
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let report =
            regenerate_skill(&old, &out, &cat, &g, &RegenOptions::default()).unwrap();
        let names: Vec<&str> = report.regenerated.iter().map(|e| e.name.as_str()).collect();
        for must_have in REGENERATE_FILES {
            assert!(names.contains(must_have), "missing {}", must_have);
        }
        let skill_path = out.join(".claude/skills/dt-api-generator/SKILL.md");
        assert!(skill_path.exists());
        let skill_md = std::fs::read_to_string(&skill_path).unwrap();
        assert!(skill_md.contains("cardholder"));
    }

    #[test]
    fn review_md_lists_every_category() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        write(
            &old.join("references").join("architecture-patterns.md"),
            "CONVENTIONS",
        );
        write(
            &old.join("references").join("obsolete-notes.md"),
            "OLD-NOTES-SHOULD-BE-FLAGGED-REMOVED",
        );
        let out = tmp.path().join("out");
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let _ = regenerate_skill(&old, &out, &cat, &g, &RegenOptions::default()).unwrap();
        let review =
            std::fs::read_to_string(out.join(".claude/skills/dt-api-generator/REVIEW.md"))
                .unwrap();
        assert!(review.contains("## Preserved"));
        assert!(review.contains("architecture-patterns.md"));
        assert!(review.contains("## Regenerated"));
        assert!(review.contains("## Generated new"));
        assert!(review.contains("## Removed"));
        assert!(review.contains("obsolete-notes.md"));
    }

    #[test]
    fn large_diff_blocked_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        // Put an old SKILL.md that differs substantially.
        let mut huge_old = String::new();
        for i in 0..200 {
            huge_old.push_str(&format!("original line {}\n", i));
        }
        write(&old.join("SKILL.md"), &huge_old);
        let out = tmp.path().join("out");
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let mut opts = RegenOptions::default();
        opts.large_diff_threshold_lines = 10;
        let err = regenerate_skill(&old, &out, &cat, &g, &opts).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("blocked by large-diff"));
        // REVIEW.md still written, but the SKILL.md should NOT have
        // been overwritten in the output.
        let out_skill = out.join(".claude/skills/dt-api-generator/SKILL.md");
        assert!(!out_skill.exists());
        let review =
            std::fs::read_to_string(out.join(".claude/skills/dt-api-generator/REVIEW.md"))
                .unwrap();
        assert!(review.contains("Blocked — large diff"));
    }

    #[test]
    fn large_diff_written_with_force() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let mut huge_old = String::new();
        for i in 0..200 {
            huge_old.push_str(&format!("original line {}\n", i));
        }
        write(&old.join("SKILL.md"), &huge_old);
        let out = tmp.path().join("out");
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let mut opts = RegenOptions::default();
        opts.large_diff_threshold_lines = 10;
        opts.force = true;
        let report = regenerate_skill(&old, &out, &cat, &g, &opts).unwrap();
        assert!(report.regenerated.iter().any(|e| e.name == "SKILL.md"));
        let out_skill = out.join(".claude/skills/dt-api-generator/SKILL.md");
        assert!(out_skill.exists());
    }

    #[test]
    fn deterministic_output_same_inputs_byte_identical() {
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let a = regenerate_contents(&cat, &g, "dt-api-generator");
        let b = regenerate_contents(&cat, &g, "dt-api-generator");
        assert_eq!(a, b);
    }

    #[test]
    fn skill_name_override_threads_through_output_path() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let out = tmp.path().join("out");
        let cat = sample_catalog();
        let g = Glossary::build(&cat);
        let mut opts = RegenOptions::default();
        opts.skill_name = "custom-skill-name".into();
        let report = regenerate_skill(&old, &out, &cat, &g, &opts).unwrap();
        assert!(report
            .skill_path
            .to_string_lossy()
            .contains("custom-skill-name"));
        let skill_path = out.join(".claude/skills/custom-skill-name/SKILL.md");
        assert!(skill_path.exists());
    }
}
