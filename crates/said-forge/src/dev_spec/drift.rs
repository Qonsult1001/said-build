//! ERD drift detection — compares the **Expected ERD** (derived from
//! Dev Spec markdown + borrow decisions) against the **Live ERD**
//! (read from the running sandbox database via `sys.tables`,
//! `sys.columns`, `sys.foreign_keys`, and `ars_Api_Rule_Settings`).
//!
//! Output is observation-only:
//!
//! - `erd-drift.md` — human-readable diff report
//! - `erd-drift-alter.sql` — proposed `ALTER TABLE ADD COLUMN` /
//!    `CREATE TABLE` statements (additive only — never DROP/DELETE)
//! - `2-progress/<client>-erd.json` updated to reflect what's
//!    actually in the live database
//!
//! Strict mode (`--strict-erd-drift`) additionally reports extras:
//! tables in DB but not in Expected, columns in entity but not in
//! Expected, registry rows not in Dev Spec. Even in strict mode no
//! DROP/DELETE SQL is emitted — the report flags them for the
//! operator to handle manually.

#![cfg(feature = "forge-sql-verify")]

use crate::dev_spec::types::{Erd, BorrowDecision};
use crate::sql_verify::{sandbox_config, SandboxInfo};
use futures_util::TryStreamExt;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tiberius::{Client, QueryItem};
use tokio_util::compat::TokioAsyncWriteCompatExt;

// ─────────────────────────── types ───────────────────────────

/// One row from `sys.tables` + `sys.columns` for a table that's in
/// the drift-check scope.
#[derive(Debug, Clone)]
pub struct LiveTable {
    pub schema: String,
    pub name: String,
    pub columns: Vec<LiveColumn>,
    pub foreign_keys: Vec<LiveForeignKey>,
}

#[derive(Debug, Clone)]
pub struct LiveColumn {
    pub name: String,
    pub ty: String,                 // e.g. "uniqueidentifier", "varchar(60)"
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub struct LiveForeignKey {
    pub column: String,
    pub references_schema: String,
    pub references_table: String,
    pub references_column: String,
}

/// Snapshot of the live database, scoped to the entity-set of
/// interest (entities the Expected ERD declares + borrow targets).
#[derive(Debug, Clone, Default)]
pub struct LiveErd {
    pub tables: BTreeMap<String, LiveTable>,   // key: "schema.name"
    pub registry_paths: BTreeSet<(String, String)>, // (path, method)
}

/// Per-entity drift — anything the Expected ERD says exists, the Live
/// DB doesn't, AND (in strict mode) anything the Live DB has that the
/// Expected doesn't.
#[derive(Debug, Clone, Default)]
pub struct DriftReport {
    pub missing_tables: Vec<String>,       // expected but not in DB
    pub missing_columns: Vec<MissingColumn>,
    pub missing_fks: Vec<MissingFk>,
    pub missing_registry_rows: Vec<(String, String)>, // (path, method)
    // Strict-mode extras (default mode leaves these empty)
    pub extra_tables: Vec<String>,
    pub extra_columns: Vec<ExtraColumn>,
    pub extra_registry_rows: Vec<(String, String)>,
    /// Total live tables read (diagnostic).
    pub live_table_count: usize,
    pub live_registry_count: usize,
}

#[derive(Debug, Clone)]
pub struct MissingColumn {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub ty: String,                        // "uniqueidentifier", "varchar(60)", etc.
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub struct MissingFk {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub references_schema: String,
    pub references_table: String,
    pub references_column: String,
}

#[derive(Debug, Clone)]
pub struct ExtraColumn {
    pub schema: String,
    pub table: String,
    pub column: String,
}

impl DriftReport {
    pub fn has_missing(&self) -> bool {
        !self.missing_tables.is_empty()
            || !self.missing_columns.is_empty()
            || !self.missing_fks.is_empty()
            || !self.missing_registry_rows.is_empty()
    }

    pub fn has_anything(&self) -> bool {
        self.has_missing()
            || !self.extra_tables.is_empty()
            || !self.extra_columns.is_empty()
            || !self.extra_registry_rows.is_empty()
    }
}

// ─────────────────────── live DB reader ──────────────────────

/// Read live tables (in scope) + registry rows from the sandbox.
///
/// `scope` is the set of `(schema, table)` pairs the caller cares
/// about — Expected ERD entities + borrow targets. Tables outside
/// scope are skipped entirely (audit, lookups, internal tooling).
pub async fn read_live_erd(
    sandbox: &SandboxInfo,
    scope: &BTreeSet<(String, String)>,
) -> Result<LiveErd, String> {
    let cfg = sandbox_config(sandbox.host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", sandbox.host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;

    let tables = read_live_tables(&mut client, scope).await?;
    let registry_paths = read_registry_paths(&mut client).await?;
    Ok(LiveErd { tables, registry_paths })
}

async fn read_live_tables(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
    scope: &BTreeSet<(String, String)>,
) -> Result<BTreeMap<String, LiveTable>, String> {
    // Pull every (schema, table, column) tuple in one query, then
    // bucket client-side. Filtering server-side per-table costs many
    // round-trips for hundreds of tables.
    let columns_sql = "\
        SELECT s.name AS schema_name, t.name AS table_name, \
               c.name AS column_name, ty.name AS type_name, \
               c.max_length AS max_length, c.is_nullable AS is_nullable \
        FROM sys.tables t \
        INNER JOIN sys.schemas s ON s.schema_id = t.schema_id \
        INNER JOIN sys.columns c ON c.object_id = t.object_id \
        INNER JOIN sys.types ty ON ty.user_type_id = c.user_type_id \
        ORDER BY s.name, t.name, c.column_id;";
    let mut tables: BTreeMap<String, LiveTable> = BTreeMap::new();
    {
        let mut stream = client
            .simple_query(columns_sql)
            .await
            .map_err(|e| format!("sys.columns: {}", e))?;
        while let Some(item) = stream.try_next().await.map_err(|e| format!("stream: {}", e))? {
            if let QueryItem::Row(row) = item {
                let schema: Option<&str> = row.try_get(0).ok().flatten();
                let table: Option<&str> = row.try_get(1).ok().flatten();
                let col: Option<&str> = row.try_get(2).ok().flatten();
                let ty: Option<&str> = row.try_get(3).ok().flatten();
                let max_len: Option<i16> = row.try_get(4).ok().flatten();
                let nullable: Option<bool> = row.try_get(5).ok().flatten();
                if let (Some(s), Some(t), Some(c), Some(ty)) = (schema, table, col, ty) {
                    let key = (s.to_lowercase(), t.to_lowercase());
                    if !scope_contains(scope, &key) {
                        continue;
                    }
                    let canonical_key = format!("{}.{}", s, t);
                    let entry = tables
                        .entry(canonical_key.clone())
                        .or_insert_with(|| LiveTable {
                            schema: s.to_string(),
                            name: t.to_string(),
                            columns: Vec::new(),
                            foreign_keys: Vec::new(),
                        });
                    let formatted = format_sql_type(ty, max_len);
                    entry.columns.push(LiveColumn {
                        name: c.to_string(),
                        ty: formatted,
                        nullable: nullable.unwrap_or(false),
                    });
                }
            }
        }
    }

    // Fetch FKs in a second query for the same scope.
    let fk_sql = "\
        SELECT \
            fs.name AS schema_name, ft.name AS table_name, fc.name AS column_name, \
            rs.name AS ref_schema, rt.name AS ref_table, rc.name AS ref_column \
        FROM sys.foreign_keys fk \
        INNER JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
        INNER JOIN sys.tables ft ON ft.object_id = fk.parent_object_id \
        INNER JOIN sys.schemas fs ON fs.schema_id = ft.schema_id \
        INNER JOIN sys.columns fc ON fc.object_id = fk.parent_object_id AND fc.column_id = fkc.parent_column_id \
        INNER JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id \
        INNER JOIN sys.schemas rs ON rs.schema_id = rt.schema_id \
        INNER JOIN sys.columns rc ON rc.object_id = fk.referenced_object_id AND rc.column_id = fkc.referenced_column_id;";
    let mut stream = client
        .simple_query(fk_sql)
        .await
        .map_err(|e| format!("sys.foreign_keys: {}", e))?;
    while let Some(item) = stream.try_next().await.map_err(|e| format!("stream: {}", e))? {
        if let QueryItem::Row(row) = item {
            let s: Option<&str> = row.try_get(0).ok().flatten();
            let t: Option<&str> = row.try_get(1).ok().flatten();
            let c: Option<&str> = row.try_get(2).ok().flatten();
            let rs: Option<&str> = row.try_get(3).ok().flatten();
            let rt: Option<&str> = row.try_get(4).ok().flatten();
            let rc: Option<&str> = row.try_get(5).ok().flatten();
            if let (Some(s), Some(t), Some(c), Some(rs), Some(rt), Some(rc)) =
                (s, t, c, rs, rt, rc)
            {
                let key = (s.to_lowercase(), t.to_lowercase());
                if !scope_contains(scope, &key) {
                    continue;
                }
                let canonical = format!("{}.{}", s, t);
                if let Some(entry) = tables.get_mut(&canonical) {
                    entry.foreign_keys.push(LiveForeignKey {
                        column: c.to_string(),
                        references_schema: rs.to_string(),
                        references_table: rt.to_string(),
                        references_column: rc.to_string(),
                    });
                }
            }
        }
    }
    Ok(tables)
}

async fn read_registry_paths(
    client: &mut Client<tokio_util::compat::Compat<tokio::net::TcpStream>>,
) -> Result<BTreeSet<(String, String)>, String> {
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    // Tries the same shapes as fitter::fetch_registry but only pulls
    // (path, method) — the drift detector doesn't need uuids.
    let candidates = [
        ("lookups.ars_Api_Rule_Settings",
         "SELECT ars_Path, aml_Code AS method FROM lookups.ars_Api_Rule_Settings;"),
        ("dbo.ars_Api_Rule_Settings",
         "SELECT ars_Path, aml_Code AS method FROM dbo.ars_Api_Rule_Settings;"),
    ];
    for (label, sql) in candidates {
        let stream_result = client.simple_query(sql).await;
        let mut stream = match stream_result {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut found_any = false;
        while let Some(item_res) = stream.try_next().await.transpose() {
            let item = match item_res {
                Ok(i) => i,
                Err(_) => break,
            };
            if let QueryItem::Row(row) = item {
                let path: Option<&str> = row.try_get(0).ok().flatten();
                let method: Option<&str> = row.try_get(1).ok().flatten();
                if let (Some(p), Some(m)) = (path, method) {
                    found_any = true;
                    out.insert((p.to_string(), m.to_uppercase()));
                }
            }
        }
        let _ = label;
        if found_any {
            break;
        }
    }
    Ok(out)
}

fn scope_contains(scope: &BTreeSet<(String, String)>, key: &(String, String)) -> bool {
    // Empty scope = include everything (test/diagnostic mode).
    if scope.is_empty() {
        return true;
    }
    scope.contains(key)
}

fn format_sql_type(ty: &str, max_length: Option<i16>) -> String {
    let lower = ty.to_lowercase();
    match lower.as_str() {
        "varchar" | "char" | "nvarchar" | "nchar" | "varbinary" | "binary" => {
            let len = max_length.unwrap_or(0);
            if len < 0 {
                format!("{}(max)", lower)
            } else if len > 0 {
                // nvarchar / nchar bytes are 2x — `max_length` already
                // counts bytes, so we report what `sys.columns` says.
                let chars = if matches!(lower.as_str(), "nvarchar" | "nchar") {
                    len / 2
                } else {
                    len
                };
                format!("{}({})", lower, chars)
            } else {
                lower
            }
        }
        _ => lower,
    }
}

// ──────────────────────── detector ───────────────────────────

/// Compare Expected ERD against Live DB. Asymmetric by default —
/// only flags missing-from-DB. Strict mode also flags extras.
///
/// `borrows` maps `Expected entity name → (schema, table_name)` so
/// we know where the entity actually lives in the database.
pub fn compute_drift(
    expected: &Erd,
    borrows: &[BorrowDecision],
    live: &LiveErd,
    expected_paths: &BTreeSet<(String, String)>,
    strict: bool,
) -> DriftReport {
    let mut report = DriftReport::default();
    report.live_table_count = live.tables.len();
    report.live_registry_count = live.registry_paths.len();

    let borrow_map: BTreeMap<&str, &BorrowDecision> = borrows
        .iter()
        .map(|b| (b.entity.as_str(), b))
        .collect();

    // Build the expected scope: each Expected entity → (schema, table_name)
    let mut expected_scope: BTreeMap<(String, String), &str> = BTreeMap::new();
    for (entity_name, _entity) in &expected.entities {
        let (schema, table_name) = match borrow_map.get(entity_name.as_str()) {
            Some(b) => (b.schema.clone(), b.table_name.clone()),
            None => ("dbo".into(), entity_name.clone()),
        };
        expected_scope.insert(
            (schema.to_lowercase(), table_name.to_lowercase()),
            entity_name.as_str(),
        );
    }

    // Per-entity comparison.
    for ((schema_lc, table_lc), entity_name) in &expected_scope {
        let expected_entity = &expected.entities[*entity_name];
        let live_key = format!("{}.{}", proper_case(schema_lc, &live.tables), proper_case(table_lc, &live.tables));
        let live_table = live.tables.iter()
            .find(|(_, t)| t.schema.eq_ignore_ascii_case(schema_lc)
                && t.name.eq_ignore_ascii_case(table_lc))
            .map(|(_, t)| t);

        match live_table {
            None => {
                report.missing_tables.push(live_key.clone());
                // Every column of the expected entity is also missing
                // — surface them so the ALTER SQL emitter has work.
                for col in &expected_entity.columns {
                    report.missing_columns.push(MissingColumn {
                        schema: schema_lc.clone(),
                        table: table_lc.clone(),
                        column: col.name.clone(),
                        ty: erd_type_to_sql(&col.ty),
                        nullable: col.nullable,
                    });
                }
            }
            Some(lt) => {
                let live_col_set: BTreeSet<String> = lt
                    .columns.iter().map(|c| c.name.to_lowercase()).collect();
                let expected_col_names: BTreeSet<String> = expected_entity
                    .columns.iter().map(|c| c.name.to_lowercase()).collect();
                // Missing columns
                for col in &expected_entity.columns {
                    if !live_col_set.contains(&col.name.to_lowercase()) {
                        report.missing_columns.push(MissingColumn {
                            schema: lt.schema.clone(),
                            table: lt.name.clone(),
                            column: col.name.clone(),
                            ty: erd_type_to_sql(&col.ty),
                            nullable: col.nullable,
                        });
                    }
                }
                // Strict mode: extras
                if strict {
                    for col in &lt.columns {
                        if !expected_col_names.contains(&col.name.to_lowercase()) {
                            report.extra_columns.push(ExtraColumn {
                                schema: lt.schema.clone(),
                                table: lt.name.clone(),
                                column: col.name.clone(),
                            });
                        }
                    }
                }
                // Missing FKs
                let live_fk_set: BTreeSet<(String, String)> = lt
                    .foreign_keys
                    .iter()
                    .map(|fk| (fk.column.to_lowercase(),
                               fk.references_table.to_lowercase()))
                    .collect();
                for fk in &expected_entity.foreign_keys {
                    let target_entity = fk.references_entity.as_str();
                    let target_table = match borrow_map.get(target_entity) {
                        Some(b) => b.table_name.clone(),
                        None => target_entity.to_string(),
                    };
                    let key = (fk.column.to_lowercase(), target_table.to_lowercase());
                    if !live_fk_set.contains(&key) {
                        let target_schema = match borrow_map.get(target_entity) {
                            Some(b) => b.schema.clone(),
                            None => "dbo".into(),
                        };
                        report.missing_fks.push(MissingFk {
                            schema: lt.schema.clone(),
                            table: lt.name.clone(),
                            column: fk.column.clone(),
                            references_schema: target_schema,
                            references_table: target_table,
                            references_column: fk.references_column.clone(),
                        });
                    }
                }
            }
        }
    }

    // Strict-mode extras: tables in live but NOT in expected scope
    // (yet still part of the borrow universe — we don't surface random
    // out-of-scope tables, only ones we'd otherwise touch).
    if strict {
        let expected_keys: BTreeSet<(String, String)> = expected_scope.keys().cloned().collect();
        for (canonical_key, lt) in &live.tables {
            let key = (lt.schema.to_lowercase(), lt.name.to_lowercase());
            if !expected_keys.contains(&key) {
                report.extra_tables.push(canonical_key.clone());
            }
        }
    }

    // Registry: missing rows (expected but not in DB).
    for (path, method) in expected_paths {
        let key = (path.clone(), method.to_uppercase());
        if !live.registry_paths.contains(&key) {
            report.missing_registry_rows.push(key);
        }
    }
    // Strict mode: extra registry rows.
    if strict {
        for (path, method) in &live.registry_paths {
            let key = (path.clone(), method.to_uppercase());
            if !expected_paths.contains(&key) {
                report.extra_registry_rows.push(key);
            }
        }
    }

    report
}

fn proper_case(lc: &str, live: &BTreeMap<String, LiveTable>) -> String {
    for (_, t) in live {
        if t.schema.eq_ignore_ascii_case(lc) {
            return t.schema.clone();
        }
        if t.name.eq_ignore_ascii_case(lc) {
            return t.name.clone();
        }
    }
    lc.to_string()
}

/// Map ERD canonical type tokens to SQL Server types for ALTER SQL.
fn erd_type_to_sql(erd_ty: &str) -> String {
    match erd_ty.to_lowercase().as_str() {
        "uuid" => "UNIQUEIDENTIFIER".into(),
        "datetime" | "date-time" => "DATETIME2".into(),
        "int" | "integer" => "INT".into(),
        "json" => "NVARCHAR(MAX)".into(),
        "boolean" | "bool" => "BIT".into(),
        s if s.starts_with("string(") && s.ends_with(')') => {
            let inner = &s["string(".len()..s.len() - 1];
            format!("NVARCHAR({})", inner)
        }
        "string" => "NVARCHAR(255)".into(),
        other => format!("/* unmapped: {} */ NVARCHAR(255)", other),
    }
}

// ──────────────────────── emitter ────────────────────────────

/// Render a human-readable Markdown report.
pub fn render_report(report: &DriftReport, client: &str, strict: bool) -> String {
    if !report.has_anything() {
        return format!(
            "# ERD drift — {client}\n\n\
             No drift detected. Live database schema matches the Dev Spec contract.\n\n\
             - Live tables in scope: {}\n\
             - Live registry rows: {}\n",
            report.live_table_count, report.live_registry_count,
        );
    }
    let mut out = format!("# ERD drift — {client}\n\n");
    out.push_str(&format!(
        "Mode: {}\n\n\
         Compared the **Expected ERD** (Dev Spec markdown + borrow decisions)\n\
         against the **Live database** ({} tables, {} registry rows).\n\n\
         Drift is **observation-only** — no SQL is executed against the\n\
         database. The companion `erd-drift-alter.sql` carries proposed\n\
         additive ALTER statements you can review and apply manually.\n\n",
        if strict { "strict (reports extras)" } else { "default (additive only)" },
        report.live_table_count,
        report.live_registry_count,
    ));

    if !report.missing_tables.is_empty() {
        out.push_str("## Missing tables (Expected → not in DB)\n\n");
        for t in &report.missing_tables {
            out.push_str(&format!("- `{t}`\n"));
        }
        out.push('\n');
    }
    if !report.missing_columns.is_empty() {
        out.push_str("## Missing columns\n\n");
        out.push_str("| Schema | Table | Column | Type | Nullable |\n");
        out.push_str("| --- | --- | --- | --- | --- |\n");
        for c in &report.missing_columns {
            out.push_str(&format!(
                "| {} | {} | `{}` | `{}` | {} |\n",
                c.schema, c.table, c.column, c.ty, if c.nullable { "yes" } else { "no" },
            ));
        }
        out.push('\n');
    }
    if !report.missing_fks.is_empty() {
        out.push_str("## Missing foreign keys\n\n");
        out.push_str("| From | References |\n| --- | --- |\n");
        for fk in &report.missing_fks {
            out.push_str(&format!(
                "| `{}.{}.{}` | `{}.{}.{}` |\n",
                fk.schema, fk.table, fk.column,
                fk.references_schema, fk.references_table, fk.references_column,
            ));
        }
        out.push('\n');
    }
    if !report.missing_registry_rows.is_empty() {
        out.push_str("## Missing registry rows (`ars_Api_Rule_Settings`)\n\n");
        out.push_str("| Path | Method |\n| --- | --- |\n");
        for (path, method) in &report.missing_registry_rows {
            out.push_str(&format!("| `{path}` | {method} |\n"));
        }
        out.push('\n');
    }
    if strict {
        if !report.extra_tables.is_empty() {
            out.push_str("## Extra tables (DB → not in Expected, strict mode)\n\n\
                          *Reported only — no DROP SQL is emitted.*\n\n");
            for t in &report.extra_tables {
                out.push_str(&format!("- `{t}`\n"));
            }
            out.push('\n');
        }
        if !report.extra_columns.is_empty() {
            out.push_str("## Extra columns (DB → not in Expected, strict mode)\n\n\
                          *Reported only — no DROP SQL is emitted.*\n\n");
            for c in &report.extra_columns {
                out.push_str(&format!(
                    "- `{}.{}.{}`\n",
                    c.schema, c.table, c.column,
                ));
            }
            out.push('\n');
        }
        if !report.extra_registry_rows.is_empty() {
            out.push_str("## Extra registry rows (DB → not in Dev Spec, strict mode)\n\n\
                          *Reported only — no DELETE SQL is emitted.*\n\n");
            out.push_str("| Path | Method |\n| --- | --- |\n");
            for (path, method) in &report.extra_registry_rows {
                out.push_str(&format!("| `{path}` | {method} |\n"));
            }
            out.push('\n');
        }
    }
    out
}

/// Render proposed ALTER SQL — additive only.
pub fn render_alter_sql(report: &DriftReport, client: &str) -> String {
    if !report.has_missing() {
        return format!(
            "-- ERD drift — {client}\n\
             -- No drift; nothing to ALTER.\n"
        );
    }
    let mut out = format!(
        "-- ERD drift — proposed ALTER statements for {client}\n\
         -- Additive only: ADD COLUMN, ADD CONSTRAINT FK, CREATE TABLE.\n\
         -- Review before applying. Never auto-applied by the toolchain.\n\n"
    );

    // CREATE TABLE for missing tables (skeleton — caller can extend
    // to include all columns + PK).
    for table in &report.missing_tables {
        out.push_str(&format!(
            "-- TODO: CREATE TABLE [{}] — add columns + PK manually or rerun\n\
             --       sql_emit::emit_all_tables to scaffold.\n",
            table,
        ));
    }
    if !report.missing_tables.is_empty() {
        out.push('\n');
    }

    // ALTER TABLE ADD COLUMN — skips columns already covered by a
    // CREATE TABLE entry (those tables don't exist yet).
    let missing_table_set: BTreeSet<String> = report.missing_tables
        .iter()
        .map(|t| t.to_lowercase())
        .collect();
    let in_missing_table = |schema: &str, table: &str| -> bool {
        let key = format!("{}.{}", schema, table).to_lowercase();
        missing_table_set.contains(&key)
    };

    let mut by_table: BTreeMap<(String, String), Vec<&MissingColumn>> = BTreeMap::new();
    for c in &report.missing_columns {
        if in_missing_table(&c.schema, &c.table) {
            continue;
        }
        by_table
            .entry((c.schema.clone(), c.table.clone()))
            .or_default()
            .push(c);
    }
    for ((schema, table), cols) in &by_table {
        out.push_str(&format!(
            "-- ADD COLUMNS to [{schema}].[{table}]\n"
        ));
        for c in cols {
            let null = if c.nullable { "NULL" } else { "NOT NULL" };
            out.push_str(&format!(
                "ALTER TABLE [{schema}].[{table}] ADD [{col}] {ty} {null};\n",
                col = c.column, ty = c.ty,
            ));
        }
        out.push('\n');
    }

    // FOREIGN KEYS
    if !report.missing_fks.is_empty() {
        out.push_str("-- ADD FOREIGN KEYS\n");
        for fk in &report.missing_fks {
            if in_missing_table(&fk.schema, &fk.table) {
                continue;
            }
            let constraint = format!(
                "FK_{}_{}_{}",
                fk.table, fk.column, fk.references_table,
            );
            out.push_str(&format!(
                "ALTER TABLE [{}].[{}] ADD CONSTRAINT [{}] FOREIGN KEY ([{}]) REFERENCES [{}].[{}]([{}]);\n",
                fk.schema, fk.table, constraint, fk.column,
                fk.references_schema, fk.references_table, fk.references_column,
            ));
        }
        out.push('\n');
    }

    if !report.missing_registry_rows.is_empty() {
        out.push_str(&format!(
            "-- {} missing registry rows; rerun `said dev-spec amend-registry --client {}`\n\
             -- to regenerate the MERGE script that upserts them.\n",
            report.missing_registry_rows.len(), client,
        ));
    }

    out
}

/// Persist drift report + ALTER SQL to disk.
pub fn write_drift_artifacts(
    report: &DriftReport,
    client: &str,
    deliverables_root: &Path,
    strict: bool,
) -> Result<(), String> {
    let report_path = deliverables_root.join("erd-drift.md");
    let report_md = render_report(report, client, strict);
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent: {}", e))?;
    }
    std::fs::write(&report_path, report_md)
        .map_err(|e| format!("write {}: {}", report_path.display(), e))?;

    let sql_path = deliverables_root.join("sql").join("erd-drift-alter.sql");
    if let Some(parent) = sql_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent: {}", e))?;
    }
    let sql = render_alter_sql(report, client);
    std::fs::write(&sql_path, sql)
        .map_err(|e| format!("write {}: {}", sql_path.display(), e))?;
    Ok(())
}

/// Convert a `LiveErd` into the canonical `Erd` JSON shape so the
/// progress dir's `<client>-erd.json` reflects what's actually in the DB.
pub fn live_erd_to_erd_json(
    live: &LiveErd,
    expected: &Erd,
) -> crate::dev_spec::types::Erd {
    use crate::dev_spec::types::{Entity, Column, ForeignKey};
    let mut out = crate::dev_spec::types::Erd::default();
    // Carry endpoints from expected (they're contract-only — DB has no concept of them).
    out.endpoints = expected.endpoints.clone();
    for (canonical_key, lt) in &live.tables {
        let entity_name = pascal_table_name(&lt.name);
        let pk = lt.columns.iter()
            .find(|c| c.name.to_lowercase() == "id"
                || c.name.to_lowercase().ends_with("_id")
                || c.name.to_lowercase() == format!("{}_id", lt.name.to_lowercase()))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| lt.columns.first().map(|c| c.name.clone()).unwrap_or_default());
        let entity = Entity {
            name: entity_name.clone(),
            columns: lt.columns.iter().map(|c| Column {
                name: c.name.clone(),
                ty: c.ty.clone(),
                nullable: c.nullable,
                description: None,
            }).collect(),
            primary_key: pk,
            foreign_keys: lt.foreign_keys.iter().map(|fk| ForeignKey {
                column: fk.column.clone(),
                references_entity: pascal_table_name(&fk.references_table),
                references_column: fk.references_column.clone(),
            }).collect(),
            introduced_by: vec![format!("live:{}", canonical_key)],
        };
        out.entities.insert(entity_name, entity);
    }
    out
}

fn pascal_table_name(s: &str) -> String {
    // Simple PascalCase: split on `_` boundaries, capitalise each.
    s.split('_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
            }
        })
        .collect()
}
