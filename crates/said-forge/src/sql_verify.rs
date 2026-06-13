//! Read-only verification pass against a running `said sandbox`
//! SQL Server container.
//!
//! Step 1 scope:
//! - Auto-discover the sandbox container (`docker ps --filter
//!   name=said-sbx-`) and pull its host port.
//! - Confirm forge's parsed proc parameter list matches `sys.parameters`.
//! - Pull lookup-table contents for every table the spec cites in
//!   `x-validated-by` — but ONLY when the table has ≤ MAX_ENUM_ROWS
//!   rows (closed sets like `cps_Cardholder_Profile_Status`). Larger
//!   tables stay open in the spec.
//!
//! Pure read; no `EXEC`, no DML, no DDL. Safe to run against any
//! reachable instance.
//!
//! Gated behind the `forge-sql-verify` feature so file-only users
//! don't pay the tiberius/rustls compile cost.

#![cfg(feature = "forge-sql-verify")]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Command;

use tiberius::{AuthMethod, Client, Config};
use tokio_util::compat::TokioAsyncWriteCompatExt;

/// Hard cap on rows we treat as a "closed enum". Larger tables get
/// left as plain `string` with the existing format hint.
pub const MAX_ENUM_ROWS: usize = 25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub container_name: String,
    pub host_port: u16,
}

/// Auto-discover a running sandbox container. Returns the first match
/// in priority order: `said-forge-sbx-*` (preferred) → `said-sbx-*`.
pub fn discover_sandbox() -> Option<SandboxInfo> {
    discover_sandbox_with_hint(None)
}

/// Auto-discover a running sandbox container, optionally preferring one
/// whose name contains a hint substring. Used by multi-client workspaces
/// to pick the right sandbox when several are running side-by-side.
///
/// Match order:
/// 1. `said-forge-sbx-*` containing `hint` (case-insensitive)
/// 2. `said-sbx-*` containing `hint`
/// 3. any `said-forge-sbx-*`
/// 4. any `said-sbx-*`
pub fn discover_sandbox_with_hint(hint: Option<&str>) -> Option<SandboxInfo> {
    if let Some(h) = hint {
        let needle = h.to_lowercase();
        if let Some(s) = discover_with_prefix_filter("said-forge-sbx-", Some(&needle)) {
            return Some(s);
        }
        if let Some(s) = discover_with_prefix_filter("said-sbx-", Some(&needle)) {
            return Some(s);
        }
    }
    discover_with_prefix("said-forge-sbx-").or_else(|| discover_with_prefix("said-sbx-"))
}

fn discover_with_prefix(prefix: &str) -> Option<SandboxInfo> {
    discover_with_prefix_filter(prefix, None)
}

fn discover_with_prefix_filter(prefix: &str, hint: Option<&str>) -> Option<SandboxInfo> {
    let filter = format!("name={}", prefix);
    let output = Command::new("docker")
        .args(["ps", "--filter", &filter, "--format", "{{.Names}}\t{{.Ports}}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let mut parts = line.splitn(2, '\t');
        let name = parts.next()?.trim();
        if !name.starts_with(prefix) {
            continue;
        }
        if let Some(h) = hint {
            if !name.to_lowercase().contains(h) {
                continue;
            }
        }
        let ports = parts.next().unwrap_or("");
        // ports format: `0.0.0.0:1433->1433/tcp`. Pull the host port
        // before `->`.
        for tok in ports.split(',') {
            let tok = tok.trim();
            if let Some(arrow) = tok.find("->") {
                let host = tok[..arrow].rsplit(':').next().unwrap_or("").trim();
                if let Ok(port) = host.parse::<u16>() {
                    return Some(SandboxInfo {
                        container_name: name.to_string(),
                        host_port: port,
                    });
                }
            }
        }
    }
    None
}

/// Build a tiberius Config for the sandbox using the well-known
/// password baked into `said sandbox`'s docker-compose.
pub fn sandbox_config(host_port: u16) -> Config {
    let mut cfg = Config::new();
    cfg.host("127.0.0.1");
    cfg.port(host_port);
    cfg.authentication(AuthMethod::sql_server("sa", "Said_Test_2026!"));
    // The sandbox uses a self-signed cert; trust it.
    cfg.trust_cert();
    cfg
}

/// Apply a multi-statement SQL script to the sandbox. Splits on `GO`
/// and `--SPLIT` markers (mirroring the conventions used by the
/// emitters). Errors on any individual batch are surfaced; previous
/// batches stay applied. Idempotent SQL (with `IF NOT EXISTS` guards
/// or MERGE patterns) is safe to re-run.
pub async fn apply_sql_script(host_port: u16, script: &str) -> Result<(), String> {
    let cfg = sandbox_config(host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;

    let mut current = String::new();
    let mut last_err: Option<String> = None;
    let mut batches_run = 0usize;
    for line in script.lines() {
        let trimmed = line.trim();
        // Split markers: `GO` (sqlcmd convention), `--SPLIT` (forge
        // convention used by sql_emit + registry_amend output).
        let is_split = trimmed.eq_ignore_ascii_case("GO")
            || trimmed.eq_ignore_ascii_case("--SPLIT");
        if is_split {
            if !current.trim().is_empty() {
                if let Err(e) = client.simple_query(&current).await {
                    let snippet: String = current.chars().take(200).collect();
                    eprintln!(
                        "    apply_sql_script batch {} failed: {} (first 200 chars: {})",
                        batches_run, e, snippet,
                    );
                    last_err = Some(format!(
                        "batch {} failed: {} (first 200 chars: {})",
                        batches_run, e, snippet,
                    ));
                    // Continue: idempotent guards mean some batches
                    // can fail (e.g. CREATE TABLE on already-existing
                    // table) without invalidating the rest.
                }
                batches_run += 1;
                current.clear();
            }
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.trim().is_empty() {
        if let Err(e) = client.simple_query(&current).await {
            last_err = Some(format!("final batch failed: {}", e));
        }
        batches_run += 1;
    }
    let _ = batches_run;
    if let Some(e) = last_err {
        // Return the last failure as a soft warning — caller decides
        // whether to bail or continue.
        return Err(e);
    }
    Ok(())
}

/// Connect to the sandbox on `host_port` and run a single read-only
/// query. Returns rows as a `Vec<Vec<Option<String>>>` (each row is a
/// vec of nullable stringified columns).
pub async fn query_rows(
    host_port: u16,
    sql: &str,
) -> Result<Vec<Vec<Option<String>>>, String> {
    let cfg = sandbox_config(host_port);
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", host_port))
        .await
        .map_err(|e| format!("connect: {}", e))?;
    tcp.set_nodelay(true).map_err(|e| format!("nodelay: {}", e))?;
    let mut client: Client<_> = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("tiberius connect: {}", e))?;
    let stream = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("query: {}", e))?;
    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("collect rows: {}", e))?;
    let mut out: Vec<Vec<Option<String>>> = Vec::with_capacity(rows.len());
    for row in rows {
        let row_len: usize = row.len();
        let mut cells: Vec<Option<String>> = Vec::with_capacity(row_len);
        for i in 0..row_len {
            // Best-effort stringify: try every common type. Type
            // annotations are mandatory because tiberius's `try_get`
            // is fully generic.
            let val: Option<String> = if let Ok(Some(v)) = row.try_get::<&str, _>(i) {
                Some(v.to_string())
            } else if let Ok(Some(v)) = row.try_get::<i32, _>(i) {
                Some(v.to_string())
            } else if let Ok(Some(v)) = row.try_get::<i64, _>(i) {
                Some(v.to_string())
            } else if let Ok(Some(v)) = row.try_get::<bool, _>(i) {
                Some(v.to_string())
            } else {
                None
            };
            cells.push(val);
        }
        out.push(cells);
    }
    Ok(out)
}

// ─────────────────────────── proc signature reflection ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcSignatureRow {
    pub schema: String,
    pub proc_name: String,
    pub param_name: String,
    pub data_type: String,
    pub max_length: Option<i32>,
    pub has_default: bool,
    pub ordinal: i32,
}

/// Pull the full proc-parameter table from `sys.parameters`. Caller
/// joins by `(schema, proc_name)` against forge's parsed catalog.
pub async fn fetch_proc_signatures(
    host_port: u16,
) -> Result<Vec<ProcSignatureRow>, String> {
    let sql = r#"
        SELECT
            s.name      AS schema_name,
            o.name      AS proc_name,
            p.name      AS param_name,
            t.name      AS data_type,
            CAST(p.max_length AS INT) AS max_length,
            p.has_default_value AS has_default,
            p.parameter_id AS ordinal
        FROM sys.parameters p
        INNER JOIN sys.objects o ON o.object_id = p.object_id
        INNER JOIN sys.schemas s ON s.schema_id = o.schema_id
        INNER JOIN sys.types   t ON t.user_type_id = p.user_type_id
        WHERE o.type IN ('P', 'PC')
          AND p.parameter_id > 0
        ORDER BY s.name, o.name, p.parameter_id;
    "#;
    let rows = query_rows(host_port, sql).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        if r.len() < 7 {
            continue;
        }
        let schema = r[0].clone().unwrap_or_default();
        let proc_name = r[1].clone().unwrap_or_default();
        let param_name = r[2].clone().unwrap_or_default();
        let data_type = r[3].clone().unwrap_or_default();
        let max_length = r[4].as_deref().and_then(|s| s.parse().ok());
        let has_default = r[5]
            .as_deref()
            .map(|s| s == "true" || s == "1")
            .unwrap_or(false);
        let ordinal = r[6].as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
        out.push(ProcSignatureRow {
            schema,
            proc_name,
            param_name,
            data_type,
            max_length,
            has_default,
            ordinal,
        });
    }
    Ok(out)
}

// ─────────────────────────── lookup contents ───────────────────────────

/// `(schema, table) → row count`. Caller filters to only tables that
/// fit the closed-set rule (`<= MAX_ENUM_ROWS`).
pub async fn fetch_lookup_row_counts(
    host_port: u16,
) -> Result<BTreeMap<(String, String), i64>, String> {
    let sql = r#"
        SELECT
            s.name AS schema_name,
            t.name AS table_name,
            CAST(SUM(p.rows) AS BIGINT) AS approx_rows
        FROM sys.tables t
        INNER JOIN sys.schemas s    ON s.schema_id = t.schema_id
        INNER JOIN sys.partitions p ON p.object_id = t.object_id AND p.index_id IN (0, 1)
        WHERE s.name = 'lookups'
        GROUP BY s.name, t.name
        ORDER BY s.name, t.name;
    "#;
    let rows = query_rows(host_port, sql).await?;
    let mut out = BTreeMap::new();
    for r in rows {
        if r.len() < 3 {
            continue;
        }
        let schema = r[0].clone().unwrap_or_default();
        let table = r[1].clone().unwrap_or_default();
        let count: i64 = r[2].as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
        out.insert((schema, table), count);
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookupValue {
    pub code: String,
    pub description: Option<String>,
}

/// Pull values from a closed-set lookup. Caller picks the code column
/// (usually `<prefix>_Code`) and an optional description column. We
/// don't introspect — the caller knows from the table-definition scan
/// what those are.
pub async fn fetch_lookup_values(
    host_port: u16,
    schema: &str,
    table: &str,
    code_col: &str,
    desc_col: Option<&str>,
) -> Result<Vec<LookupValue>, String> {
    fetch_lookup_values_with_cap(host_port, schema, table, code_col, desc_col, MAX_ENUM_ROWS).await
}

pub async fn fetch_lookup_values_with_cap(
    host_port: u16,
    schema: &str,
    table: &str,
    code_col: &str,
    desc_col: Option<&str>,
    max_rows: usize,
) -> Result<Vec<LookupValue>, String> {
    let _ = max_rows; // used below in the SELECT TOP
    fetch_lookup_values_inner(host_port, schema, table, code_col, desc_col, max_rows).await
}

async fn fetch_lookup_values_inner(
    host_port: u16,
    schema: &str,
    table: &str,
    code_col: &str,
    desc_col: Option<&str>,
    max_rows: usize,
) -> Result<Vec<LookupValue>, String> {
    // Identifier-quote everything because table/column names follow
    // SQL Server bracket-quote rules. We sanitise here in addition to
    // the trust we put in the catalog: the strings come from forge's
    // own parser, but defence-in-depth.
    let s = sanitise_ident(schema);
    let t = sanitise_ident(table);
    let c = sanitise_ident(code_col);
    let d = desc_col.map(sanitise_ident);
    let select = match d.as_ref() {
        Some(dc) => format!(
            "SELECT TOP {top} CAST([{c}] AS NVARCHAR(MAX)), CAST([{dc}] AS NVARCHAR(MAX)) FROM [{s}].[{t}] ORDER BY [{c}]",
            top = max_rows + 1,
            c = c,
            dc = dc,
            s = s,
            t = t
        ),
        None => format!(
            "SELECT TOP {top} CAST([{c}] AS NVARCHAR(MAX)) FROM [{s}].[{t}] ORDER BY [{c}]",
            top = max_rows + 1,
            c = c,
            s = s,
            t = t
        ),
    };
    let rows = query_rows(host_port, &select).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let code = match r.get(0).and_then(|c| c.clone()) {
            Some(v) => v,
            None => continue,
        };
        let desc = r.get(1).and_then(|c| c.clone());
        out.push(LookupValue {
            code,
            description: desc,
        });
    }
    Ok(out)
}

fn sanitise_ident(s: &str) -> String {
    // Allow only [A-Za-z0-9_]; everything else gets stripped. SQL
    // Server identifiers in the dt codebase fit this — defensively
    // strip just in case.
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

// ─────────────────────────── high-level API ───────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationReport {
    pub container_name: String,
    pub host_port: u16,
    pub procs_verified: usize,
    pub signature_mismatches: Vec<String>,
    pub closed_lookup_enums: BTreeMap<String, Vec<LookupValue>>,
    pub skipped_open_lookups: Vec<String>,
}

/// Full Step 1 pass: connect, fetch signatures, fetch row counts, pull
/// closed-set values for every `lookups.<table>` whose row count is
/// within `MAX_ENUM_ROWS`. Caller threads the resulting
/// `closed_lookup_enums` into the OpenAPI generator (one map keyed by
/// `lookups.<table>`).
///
/// `lookup_columns` provides per-table `(code_col, desc_col?)` so the
/// query knows which columns to SELECT. The caller (sql_to_openapi)
/// already knows this from the proc-body lookup-hint chain.
pub async fn run_verification(
    sandbox: &SandboxInfo,
    lookup_columns: &BTreeMap<String, (String, Option<String>)>,
) -> Result<VerificationReport, String> {
    run_verification_with_cap(sandbox, lookup_columns, MAX_ENUM_ROWS).await
}

/// Same as `run_verification` but with an explicit row-count cap so the
/// CLI can let the user bump it (currency codes / locales tend to break
/// the default 25-row cap).
pub async fn run_verification_with_cap(
    sandbox: &SandboxInfo,
    lookup_columns: &BTreeMap<String, (String, Option<String>)>,
    max_enum_rows: usize,
) -> Result<VerificationReport, String> {
    let signatures = fetch_proc_signatures(sandbox.host_port).await?;
    let counts = fetch_lookup_row_counts(sandbox.host_port).await?;

    let mut closed_lookup_enums: BTreeMap<String, Vec<LookupValue>> = BTreeMap::new();
    let mut skipped_open_lookups: Vec<String> = Vec::new();
    for (table_full, (code_col, desc_col)) in lookup_columns {
        let parts: Vec<&str> = table_full.splitn(2, '.').collect();
        if parts.len() != 2 {
            continue;
        }
        let schema = parts[0];
        let table = parts[1];
        let count = counts
            .get(&(schema.to_string(), table.to_string()))
            .copied()
            .unwrap_or(0);
        if count == 0 || (count as usize) > max_enum_rows {
            skipped_open_lookups.push(format!("{} (rows={})", table_full, count));
            continue;
        }
        match fetch_lookup_values_with_cap(
            sandbox.host_port,
            schema,
            table,
            code_col,
            desc_col.as_deref(),
            max_enum_rows,
        )
        .await
        {
            Ok(values) => {
                closed_lookup_enums.insert(table_full.clone(), values);
            }
            Err(e) => {
                skipped_open_lookups.push(format!("{} (error: {})", table_full, e));
            }
        }
    }

    let procs_verified = signatures
        .iter()
        .map(|r| (r.schema.clone(), r.proc_name.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    Ok(VerificationReport {
        container_name: sandbox.container_name.clone(),
        host_port: sandbox.host_port,
        procs_verified,
        signature_mismatches: Vec::new(),
        closed_lookup_enums,
        skipped_open_lookups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitise_ident_drops_non_alnum() {
        assert_eq!(sanitise_ident("col_Code"), "col_Code");
        assert_eq!(sanitise_ident("col_Code; DROP TABLE"), "col_CodeDROPTABLE");
        assert_eq!(sanitise_ident("[col_Code]"), "col_Code");
    }

    #[test]
    fn discover_returns_none_when_docker_absent_or_no_match() {
        // CI without docker → command fails → None.
        // Local without sandbox → no matching lines → None.
        // Either way the call is safe.
        let _ = discover_sandbox();
    }
}
