//! Wire-shape audit — proc-projection-vs-DTO consistency check.
//!
//! For every endpoint in a bundle:
//!   1. Parse the deployed proc via `proc_analysis::analyse_body` (sidecar
//!      preferred, tree-sitter-sequel fallback).
//!   2. Parse the rendered Response DTO via `cs_dto_audit::parse_dto_file`.
//!   3. Diff the proc's `FOR JSON PATH` alias set against the DTO's
//!      property set (case-insensitive).
//!
//! Mismatches surface as:
//!   - **missing**: alias in the proc that's not a DTO property (renderer
//!     dropped it, OR proc rendered before DTO).
//!   - **extra**: DTO property that has no proc alias (DTO was hand-edited
//!     or rendered from a stale Dev Spec).
//!
//! This is the IA-equivalent "deviations fail the build" gate at the wire
//! boundary: if a proc and its DTO disagree on field names, the audit fails
//! and the rendered C# DTO is the bit that needs regenerating.

use std::collections::HashSet;
use std::path::Path;

use crate::proc_framework::cs_dto_audit::parse_dto_file;

/// One row in the audit report — one endpoint's verdict.
#[derive(Debug, Clone)]
pub struct WireShapeRow {
    /// Endpoint id, e.g. `Account.GetAllAccounts`.
    pub id: String,
    /// Outcome: `clean` / `drift` / `proc-missing` / `dto-missing` /
    /// `proc-no-projection` / `no-response-dto`.
    pub status: &'static str,
    /// Proc projection aliases (lowercased) in source order.
    pub proc_aliases: Vec<String>,
    /// DTO property names (lowercased) in source order.
    pub dto_properties: Vec<String>,
    /// Aliases present in proc but missing from DTO (lowercased).
    pub missing_in_dto: Vec<String>,
    /// Properties present in DTO but missing from proc (lowercased).
    pub extra_in_dto: Vec<String>,
    /// Free-text note for non-drift outcomes (file-not-found, etc).
    pub note: Option<String>,
}

impl WireShapeRow {
    pub fn is_clean(&self) -> bool {
        self.status == "clean"
    }
    pub fn is_drift(&self) -> bool {
        self.status == "drift"
    }
}

/// Audit one endpoint. Returns a `WireShapeRow` describing the verdict.
///
/// `proc_path` — deployed proc `.sql` to parse via `proc_analysis`.
/// `dto_path`  — rendered Response DTO `.cs` to parse via the C# AST tools.
pub fn audit_one(id: &str, proc_path: &Path, dto_path: &Path) -> WireShapeRow {
    if !proc_path.exists() {
        return WireShapeRow {
            id: id.to_string(),
            status: "proc-missing",
            proc_aliases: Vec::new(),
            dto_properties: Vec::new(),
            missing_in_dto: Vec::new(),
            extra_in_dto: Vec::new(),
            note: Some(format!("proc not found at {}", proc_path.display())),
        };
    }
    if !dto_path.exists() {
        return WireShapeRow {
            id: id.to_string(),
            status: "dto-missing",
            proc_aliases: Vec::new(),
            dto_properties: Vec::new(),
            missing_in_dto: Vec::new(),
            extra_in_dto: Vec::new(),
            note: Some(format!("DTO not found at {}", dto_path.display())),
        };
    }

    // Proc side.
    let body = match std::fs::read_to_string(proc_path) {
        Ok(s) => s,
        Err(e) => {
            return WireShapeRow {
                id: id.to_string(),
                status: "proc-missing",
                proc_aliases: Vec::new(),
                dto_properties: Vec::new(),
                missing_in_dto: Vec::new(),
                extra_in_dto: Vec::new(),
                note: Some(format!("read {}: {}", proc_path.display(), e)),
            };
        }
    };
    let analysis = crate::proc_analysis::analyse_body(id, &proc_path.to_string_lossy(), &body);
    if analysis.response_projection.is_empty() {
        return WireShapeRow {
            id: id.to_string(),
            status: "proc-no-projection",
            proc_aliases: Vec::new(),
            dto_properties: Vec::new(),
            missing_in_dto: Vec::new(),
            extra_in_dto: Vec::new(),
            note: Some(
                "proc has no FOR JSON PATH projection (PrcCode-only / non-API proc?)".to_string(),
            ),
        };
    }
    let proc_aliases: Vec<String> = analysis
        .response_projection
        .iter()
        .map(|p| p.alias.to_ascii_lowercase())
        .collect();

    // DTO side.
    let dto = match parse_dto_file(dto_path) {
        Ok(d) => d,
        Err(e) => {
            return WireShapeRow {
                id: id.to_string(),
                status: "dto-missing",
                proc_aliases,
                dto_properties: Vec::new(),
                missing_in_dto: Vec::new(),
                extra_in_dto: Vec::new(),
                note: Some(format!("parse {}: {}", dto_path.display(), e)),
            };
        }
    };
    let dto_properties: Vec<String> = dto
        .properties
        .iter()
        .map(|p| p.name.to_ascii_lowercase())
        .collect();

    // Diff. Set-based — order isn't part of the wire contract (JSON is
    // unordered). The auditor flags either side's extras.
    let proc_set: HashSet<&str> = proc_aliases.iter().map(String::as_str).collect();
    let dto_set: HashSet<&str> = dto_properties.iter().map(String::as_str).collect();
    let missing_in_dto: Vec<String> = proc_aliases
        .iter()
        .filter(|a| !dto_set.contains(a.as_str()))
        .cloned()
        .collect();
    let extra_in_dto: Vec<String> = dto_properties
        .iter()
        .filter(|p| !proc_set.contains(p.as_str()))
        .cloned()
        .collect();

    let status = if missing_in_dto.is_empty() && extra_in_dto.is_empty() {
        "clean"
    } else {
        "drift"
    };
    WireShapeRow {
        id: id.to_string(),
        status,
        proc_aliases,
        dto_properties,
        missing_in_dto,
        extra_in_dto,
        note: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("said-forge-wire-shape-{}-{}.tmp", std::process::id(), name));
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        p
    }

    #[test]
    fn clean_when_proc_alias_set_equals_dto_property_set() {
        let proc_sql = r#"
            CREATE PROCEDURE [a].[p_x]
                @uRequestId UNIQUEIDENTIFIER,
                @uAccountId UNIQUEIDENTIFIER
            AS
            BEGIN
                SET @sResponseObj = (
                    SELECT
                        @uAccountId AS [id],
                        @uAccountId AS [accountOwnerId]
                    FOR JSON PATH, WITHOUT_ARRAY_WRAPPER
                );
            END
        "#;
        let dto_cs = r#"
            using System.Diagnostics.CodeAnalysis;
            namespace X;
            [ExcludeFromCodeCoverage]
            public class FooResponse {
                public System.Guid? Id { get; set; }
                public System.Guid? AccountOwnerId { get; set; }
            }
        "#;
        let proc_path = write_tmp("clean.sql", proc_sql);
        let dto_path = write_tmp("clean.cs", dto_cs);
        let row = audit_one("X.Foo", &proc_path, &dto_path);
        let _ = fs::remove_file(&proc_path);
        let _ = fs::remove_file(&dto_path);
        assert!(row.is_clean(), "expected clean, got {:?}", row);
    }

    #[test]
    fn drift_when_dto_has_extra_property() {
        let proc_sql = r#"
            CREATE PROCEDURE [a].[p_x] @uX UNIQUEIDENTIFIER AS BEGIN
                SET @sResponseObj = (SELECT @uX AS [id] FOR JSON PATH, WITHOUT_ARRAY_WRAPPER);
            END
        "#;
        let dto_cs = r#"
            namespace X;
            public class FooResponse {
                public System.Guid? Id { get; set; }
                public string Stale { get; set; }
            }
        "#;
        let proc_path = write_tmp("drift.sql", proc_sql);
        let dto_path = write_tmp("drift.cs", dto_cs);
        let row = audit_one("X.Foo", &proc_path, &dto_path);
        let _ = fs::remove_file(&proc_path);
        let _ = fs::remove_file(&dto_path);
        assert!(row.is_drift(), "expected drift, got {:?}", row);
        assert_eq!(row.extra_in_dto, vec!["stale".to_string()]);
        assert!(row.missing_in_dto.is_empty());
    }
}
