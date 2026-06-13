//! Forge-owned snapshot + sandbox generator.
//!
//! This is the forge-side analogue of `said snapshot card` + `said sandbox card`,
//! built completely separately so it does NOT touch the existing vivere-tuned
//! sandbox path. Operates on `forge-sync:` frames (written by `said forge sync`)
//! and emits a self-contained docker-compose tree under
//! `<workspace_root>/.forge-sandbox/<module>/sandbox/`.
//!
//! Three classifications, all driven by frame **content** (not title):
//! - `CREATE TABLE` / `ALTER TABLE` → schema (FK-sorted, two-pass)
//! - `CREATE PROCEDURE` / `CREATE FUNCTION` / `CREATE VIEW` / `CREATE TRIGGER` → schema
//! - `MERGE INTO [lookups]` → `data.sql` (loaded as `03-data.sql`)
//! - `INSERT INTO` → `seed-data.sql` (loaded as `02-seed.sql`)
//!
//! The sandbox container name is `said-forge-sbx-<module>-<port>` so it never
//! collides with the existing `said-sbx-*` flow.

use sca_core::said_file::SaidFile;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Default port for forge sandboxes — picked one above the vivere default
/// (1433) so both can run side-by-side without `--port`.
pub const DEFAULT_FORGE_SANDBOX_PORT: u16 = 1434;

/// Hard cap on FK topo-sort rounds. Mirrors the vivere sandbox's `for _ in 0..50`
/// so circular FKs degrade gracefully instead of hanging.
const MAX_FK_PASSES: usize = 50;

/// Fixed sandbox password — local-only, not a secret. Same as vivere sandbox
/// to keep the verify path simple.
pub const SANDBOX_PASSWORD: &str = "Said_Test_2026!";

#[derive(Debug, Clone)]
pub struct ForgeSandboxOutput {
    pub sandbox_dir: PathBuf,
    pub container_name: String,
    pub host_port: u16,
    pub tables: usize,
    pub procs: usize,
    pub functions: usize,
    pub views: usize,
    pub triggers: usize,
    pub merge_files: usize,
    pub seed_inserts: usize,
}

/// Build the forge sandbox directory for a given module.
///
/// `workspace_root` is typically the dir containing the `.said` brain (e.g.
/// `dtcard/`). `module` is a substring matched against frame doc_ids
/// (case-insensitive) — e.g. `cardholder` for the dtcard project.
pub fn build_forge_sandbox(
    said: &mut SaidFile,
    workspace_root: &Path,
    module: &str,
    port: u16,
    project_name: &str,
) -> Result<ForgeSandboxOutput, String> {
    let module_lower = module.to_lowercase();

    // 1. Walk every active frame, classify by content prefix. Frames are
    //    forge-sync'd, so doc_id starts with `forge-sync:1-ground-truth/...`.
    let doc_ids: Vec<String> = said
        .frames
        .active_doc_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let mut tables: Vec<TableEntry> = Vec::new();
    let mut procs: Vec<(String, String)> = Vec::new();
    let mut functions: Vec<(String, String)> = Vec::new();
    let mut views: Vec<(String, String)> = Vec::new();
    let mut triggers: Vec<(String, String)> = Vec::new();
    let mut data_parts: Vec<String> = Vec::new();
    let mut seed_parts: Vec<String> = Vec::new();

    for did in &doc_ids {
        let did_lower = did.to_lowercase();

        let content = match said.get(did) {
            Some(c) => c,
            None => continue,
        };
        // Strip BOM that forge-sync left on UTF-8 conversions.
        let clean = content.trim_start_matches('\u{FEFF}').to_string();
        let upper = clean.to_uppercase();

        // MERGE-style lookup-data scripts come FIRST and are global —
        // they target the shared `lookups.*` schema referenced by every
        // module. Bypass the module gate so all lookup data ships into
        // every forge sandbox regardless of which module is being built.
        if upper.contains("MERGE INTO") && upper.contains("[LOOKUPS]") {
            data_parts.push(format!("-- {}\n{}\nGO\n", did, clean));
            continue;
        }

        // CREATE TABLE [lookups].* — also global. The module's procs
        // reference these tables and the MERGE data above writes into
        // them. Without the DDL, MERGE fails with "object does not exist".
        let is_global_lookup_ddl =
            upper.contains("CREATE TABLE") && upper.contains("[LOOKUPS]");

        // Module gate for everything else: doc_id must contain the
        // module token. `cardholder` matches
        // `forge-sync:1-ground-truth/cardholder/...`.
        if !is_global_lookup_ddl && !did_lower.contains(&module_lower) {
            continue;
        }

        if upper.contains("CREATE TABLE") {
            let tname = extract_object_name_after(&clean, "CREATE TABLE")
                .unwrap_or_else(|| did.clone());
            let fks = extract_fk_targets(&clean);
            tables.push(TableEntry {
                name: tname,
                content: clean,
                fk_refs: fks,
            });
            continue;
        }

        if upper.contains("CREATE PROCEDURE") || upper.contains("CREATE OR ALTER PROCEDURE") {
            procs.push((did.clone(), clean));
            continue;
        }

        if upper.contains("CREATE FUNCTION") || upper.contains("CREATE OR ALTER FUNCTION") {
            functions.push((did.clone(), clean));
            continue;
        }

        if upper.contains("CREATE VIEW") || upper.contains("CREATE OR ALTER VIEW") {
            views.push((did.clone(), clean));
            continue;
        }

        if upper.contains("CREATE TRIGGER") || upper.contains("CREATE OR ALTER TRIGGER") {
            triggers.push((did.clone(), clean));
            continue;
        }

        // Plain INSERT seed (no MERGE, no DDL).
        if upper.contains("INSERT INTO") {
            seed_parts.push(format!("-- {}\n{}\nGO\n", did, clean));
        }
    }

    // 2. FK-aware topo sort for tables — two-pass like the vivere sandbox.
    let table_names: HashSet<String> = tables.iter().map(|t| t.name.to_uppercase()).collect();
    let ordered_tables = topo_sort_tables(tables, &table_names);

    // 3. Render schema.sql.
    let schema_sql = render_schema(&ordered_tables, &functions, &views, &procs, &triggers, module);

    // 4. Build sandbox dir under workspace_root/.forge-sandbox/<module>/sandbox.
    let sandbox_root = workspace_root
        .join(".forge-sandbox")
        .join(module);
    let sandbox_dir = sandbox_root.join("sandbox");
    std::fs::create_dir_all(&sandbox_dir)
        .map_err(|e| format!("create sandbox dir {}: {}", sandbox_dir.display(), e))?;

    let container_name = format!("said-forge-sbx-{}-{}", module, port);

    write_file(&sandbox_dir.join("schema.sql"), &schema_sql)?;
    write_file(
        &sandbox_dir.join("seed-data.sql"),
        &if seed_parts.is_empty() {
            "-- No INSERT seed frames found\n".to_string()
        } else {
            format!(
                "-- INSERT seed for {} ({} statements)\n-- Auto-generated by said forge sandbox\n\n{}",
                module,
                seed_parts.len(),
                seed_parts.join("\n")
            )
        },
    )?;
    write_file(
        &sandbox_dir.join("data.sql"),
        &if data_parts.is_empty() {
            "-- No MERGE-style lookup-data frames found\n".to_string()
        } else {
            format!(
                "-- Lookup-data MERGE scripts for {} ({} files)\n-- Auto-generated by said forge sandbox\n\n{}",
                module,
                data_parts.len(),
                data_parts.join("\n")
            )
        },
    )?;
    write_file(
        &sandbox_dir.join("docker-compose.yml"),
        &render_compose(project_name, module, &container_name, port),
    )?;
    write_file(
        &sandbox_dir.join("run.sh"),
        &render_run_script(&container_name, port, module),
    )?;

    Ok(ForgeSandboxOutput {
        sandbox_dir,
        container_name,
        host_port: port,
        tables: ordered_tables.len(),
        procs: procs.len(),
        functions: functions.len(),
        views: views.len(),
        triggers: triggers.len(),
        merge_files: data_parts.len(),
        seed_inserts: seed_parts.len(),
    })
}

#[derive(Debug, Clone)]
struct TableEntry {
    name: String,
    content: String,
    fk_refs: Vec<String>,
}

fn topo_sort_tables(
    tables: Vec<TableEntry>,
    known: &HashSet<String>,
) -> Vec<TableEntry> {
    let mut placed: HashSet<String> = HashSet::new();
    let mut ordered: Vec<TableEntry> = Vec::new();
    let mut remaining: Vec<TableEntry> = tables;

    for _ in 0..MAX_FK_PASSES {
        if remaining.is_empty() {
            break;
        }
        let prev = remaining.len();
        let mut still: Vec<TableEntry> = Vec::new();
        for t in remaining.drain(..) {
            let satisfied = t.fk_refs.iter().all(|r| {
                let r_up = r.to_uppercase();
                placed.contains(&r_up) || !known.contains(&r_up) || r_up == t.name.to_uppercase()
            });
            if satisfied {
                placed.insert(t.name.to_uppercase());
                ordered.push(t);
            } else {
                still.push(t);
            }
        }
        if still.len() == prev {
            // Circular FKs — emit the rest as-is.
            ordered.extend(still);
            return ordered;
        }
        remaining = still;
    }
    ordered.extend(remaining);
    ordered
}

fn extract_object_name_after(src: &str, marker: &str) -> Option<String> {
    let upper = src.to_uppercase();
    let pos = upper.find(marker)?;
    let after = &src[pos + marker.len()..];
    let after = after.trim_start();
    let mut name = String::new();
    for ch in after.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '[' || ch == ']' {
            name.push(ch);
        } else {
            break;
        }
    }
    let no_brackets = name.replace(['[', ']'], "");
    let last = no_brackets.rsplit('.').next().unwrap_or(&no_brackets);
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

fn extract_fk_targets(content: &str) -> Vec<String> {
    let mut refs: BTreeSet<String> = BTreeSet::new();
    for line in content.lines() {
        let upper = line.to_uppercase();
        if let Some(pos) = upper.find("REFERENCES") {
            let after = upper[pos + "REFERENCES".len()..].trim_start();
            let mut ident = String::new();
            for ch in after.chars() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '[' || ch == ']' {
                    ident.push(ch);
                } else {
                    break;
                }
            }
            let no_brackets = ident.replace(['[', ']'], "");
            if let Some(last) = no_brackets.rsplit('.').next() {
                if !last.is_empty() {
                    refs.insert(last.to_string());
                }
            }
        }
    }
    refs.into_iter().collect()
}

fn render_schema(
    tables: &[TableEntry],
    functions: &[(String, String)],
    views: &[(String, String)],
    procs: &[(String, String)],
    triggers: &[(String, String)],
    module: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "-- Forge sandbox schema for module: {}\n\
         -- Tables: {}, Functions: {}, Views: {}, Procs: {}, Triggers: {}\n\
         -- Auto-generated by said forge sandbox\n\n",
        module,
        tables.len(),
        functions.len(),
        views.len(),
        procs.len(),
        triggers.len(),
    ));
    out.push_str("SET QUOTED_IDENTIFIER ON;\nGO\nSET ANSI_NULLS ON;\nGO\n\n");

    // Order: schemas → functions → tables → views → procs → triggers.
    // Schema CREATE for any non-dbo schemas referenced by table/proc names.
    let schemas = collect_schemas(tables, procs, functions, views, triggers);
    if !schemas.is_empty() {
        out.push_str("-- ═══ Schemas ═══\n");
        for s in &schemas {
            out.push_str(&format!(
                "IF SCHEMA_ID('{s}') IS NULL EXEC('CREATE SCHEMA [{s}]');\nGO\n",
                s = s
            ));
        }
        out.push('\n');
    }

    if !functions.is_empty() {
        out.push_str("-- ═══ Functions ═══\n");
        for (did, body) in functions {
            out.push_str(&format!("-- {}\n{}\nGO\n\n", did, body));
        }
    }

    out.push_str("-- ═══ Tables (FK-sorted) ═══\n");
    for t in tables {
        out.push_str(&format!("-- {}\n{}\nGO\n\n", t.name, t.content));
    }

    if !views.is_empty() {
        out.push_str("-- ═══ Views ═══\n");
        for (did, body) in views {
            out.push_str(&format!("-- {}\n{}\nGO\n\n", did, body));
        }
    }

    if !procs.is_empty() {
        out.push_str("-- ═══ Stored Procedures ═══\n");
        for (did, body) in procs {
            out.push_str(&format!("-- {}\n{}\nGO\n\n", did, body));
        }
    }

    if !triggers.is_empty() {
        out.push_str("-- ═══ Triggers ═══\n");
        for (did, body) in triggers {
            out.push_str(&format!("-- {}\n{}\nGO\n\n", did, body));
        }
    }

    out
}

fn collect_schemas(
    tables: &[TableEntry],
    procs: &[(String, String)],
    functions: &[(String, String)],
    views: &[(String, String)],
    triggers: &[(String, String)],
) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let push = |s: &mut BTreeSet<String>, body: &str, marker: &str| {
        let upper = body.to_uppercase();
        if let Some(pos) = upper.find(marker) {
            let after = body[pos + marker.len()..].trim_start();
            let mut ident = String::new();
            for ch in after.chars() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '[' || ch == ']' {
                    ident.push(ch);
                } else {
                    break;
                }
            }
            let no_brackets = ident.replace(['[', ']'], "");
            if let Some(dot) = no_brackets.find('.') {
                let schema_part = &no_brackets[..dot];
                if !schema_part.is_empty() && schema_part.to_lowercase() != "dbo" {
                    s.insert(schema_part.to_string());
                }
            }
        }
    };
    for t in tables {
        push(&mut out, &t.content, "CREATE TABLE");
    }
    for (_, body) in procs {
        push(&mut out, body, "CREATE PROCEDURE");
        push(&mut out, body, "CREATE OR ALTER PROCEDURE");
    }
    for (_, body) in functions {
        push(&mut out, body, "CREATE FUNCTION");
        push(&mut out, body, "CREATE OR ALTER FUNCTION");
    }
    for (_, body) in views {
        push(&mut out, body, "CREATE VIEW");
        push(&mut out, body, "CREATE OR ALTER VIEW");
    }
    for (_, body) in triggers {
        push(&mut out, body, "CREATE TRIGGER");
        push(&mut out, body, "CREATE OR ALTER TRIGGER");
    }
    // Always ensure `lookups` exists — MERGE scripts target it.
    out.insert("lookups".to_string());
    out.into_iter().collect()
}

fn render_compose(project: &str, module: &str, container: &str, port: u16) -> String {
    format!(
"name: said-forge-sbx-{module}-{port}
services:
  sqlserver:
    image: mcr.microsoft.com/mssql/server:2025-latest
    container_name: {container}
    environment:
      SA_PASSWORD: \"{password}\"
      ACCEPT_EULA: \"Y\"
      MSSQL_PID: \"Developer\"
    ports:
      - \"{port}:1433\"
    volumes:
      - ./schema.sql:/docker-entrypoint-initdb.d/01-schema.sql
      - ./seed-data.sql:/docker-entrypoint-initdb.d/02-seed.sql
      - ./data.sql:/docker-entrypoint-initdb.d/03-data.sql
    healthcheck:
      test: /opt/mssql-tools18/bin/sqlcmd -S localhost -U sa -P \"{password}\" -C -Q \"SELECT 1\"
      interval: 10s
      timeout: 5s
      retries: 5

# Project: {project}
# Module:  {module}
# Connection: Server=localhost,{port};User Id=sa;Password={password};TrustServerCertificate=True
",
        project = project,
        module = module,
        container = container,
        port = port,
        password = SANDBOX_PASSWORD,
    )
}

fn render_run_script(container: &str, port: u16, module: &str) -> String {
    format!(
"#!/bin/bash
set -e
echo \"Starting forge {module} sandbox on port {port}...\"
docker compose up -d
echo \"Waiting for SQL Server...\"
until docker exec {container} /opt/mssql-tools18/bin/sqlcmd \\
    -S localhost -U sa -P '{password}' -C -Q 'SELECT 1' 2>/dev/null | grep -q '1 rows'; do
  sleep 2
done
echo \"Loading schema...\"
docker exec -i {container} /opt/mssql-tools18/bin/sqlcmd \\
  -S localhost -U sa -P '{password}' -C -i /docker-entrypoint-initdb.d/01-schema.sql
echo \"Loading INSERT seed data...\"
docker exec -i {container} /opt/mssql-tools18/bin/sqlcmd \\
  -S localhost -U sa -P '{password}' -C -i /docker-entrypoint-initdb.d/02-seed.sql
echo \"Loading lookup-data MERGE scripts...\"
docker exec -i {container} /opt/mssql-tools18/bin/sqlcmd \\
  -S localhost -U sa -P '{password}' -C -i /docker-entrypoint-initdb.d/03-data.sql
echo
echo \"Forge sandbox ready on port {port}!\"
echo \"Connection: Server=localhost,{port};User=sa;Password={password}\"
",
        container = container,
        port = port,
        module = module,
        password = SANDBOX_PASSWORD,
    )
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|e| format!("write {}: {}", path.display(), e))
}

/// Run `docker compose up -d` against the generated sandbox dir, then poll
/// the healthcheck for up to 90 seconds and execute the three init scripts
/// via `sqlcmd` exec. Returns `(error_count_per_script, ready)`.
pub fn bring_up_sandbox(out: &ForgeSandboxOutput) -> Result<BringUpReport, String> {
    use std::process::Command;
    let mut report = BringUpReport::default();

    let compose = Command::new("docker")
        .args(["compose", "up", "-d"])
        .current_dir(&out.sandbox_dir)
        .output()
        .map_err(|e| format!("docker compose up: {}", e))?;
    if !compose.status.success() {
        return Err(format!(
            "docker compose up failed:\n{}",
            String::from_utf8_lossy(&compose.stderr)
        ));
    }
    report.compose_started = true;

    // Poll healthcheck up to 90s (30 * 3s).
    for _ in 0..30 {
        let probe = Command::new("docker")
            .args([
                "exec",
                &out.container_name,
                "/opt/mssql-tools18/bin/sqlcmd",
                "-S", "localhost",
                "-U", "sa",
                "-P", SANDBOX_PASSWORD,
                "-C",
                "-Q", "SELECT 1",
            ])
            .output();
        if let Ok(r) = probe {
            if r.status.success()
                && String::from_utf8_lossy(&r.stdout).contains("1 rows")
            {
                report.healthy = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    if !report.healthy {
        return Err(format!(
            "SQL Server did not become healthy within 90s. Check `docker logs {}`",
            out.container_name
        ));
    }

    // Run each init script in order so we can count errors per script.
    for (label, script) in [
        ("schema", "/docker-entrypoint-initdb.d/01-schema.sql"),
        ("seed",   "/docker-entrypoint-initdb.d/02-seed.sql"),
        ("data",   "/docker-entrypoint-initdb.d/03-data.sql"),
    ] {
        let r = Command::new("docker")
            .args([
                "exec", "-i", &out.container_name,
                "/opt/mssql-tools18/bin/sqlcmd",
                "-S", "localhost",
                "-U", "sa",
                "-P", SANDBOX_PASSWORD,
                "-C", "-i", script,
            ])
            .output()
            .map_err(|e| format!("exec {}: {}", label, e))?;
        let stdout = String::from_utf8_lossy(&r.stdout);
        let err_count = stdout
            .lines()
            .filter(|l| l.contains("Msg ") && l.contains("Level 16"))
            .count();
        report.script_errors.insert(label.to_string(), err_count);
    }

    Ok(report)
}

#[derive(Debug, Clone, Default)]
pub struct BringUpReport {
    pub compose_started: bool,
    pub healthy: bool,
    /// Per-script Level-16 error count (`schema` / `seed` / `data`).
    pub script_errors: BTreeMap<String, usize>,
}
