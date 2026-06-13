//! C# auditor — text-level region comparison against deployed Query classes.
//!
//! Mirrors `audit.rs` (SQL auditor) but uses the `// [SaidFully]` / `// [SaidEnd]`
//! grammar from `cs_markers.rs`. Same Option 3 contract:
//!   - `framework-managed`: deployed file has `[SaidFully]` markers.
//!     Fully regions must match canonical. Drift = defect.
//!   - `legacy`: no markers. Informational only (touch-it-migrate-it).
//!
//! Phase 1 scope: audit only. Generation (writing rendered .cs files) is
//! Phase 2. This module reads deployed .cs, parses regions, and reports —
//! it never writes.

use std::path::{Path, PathBuf};

use super::cs_markers::{parse_cs_regions, CsRegion};
use super::manifest::EndpointRow;
use super::markers::ManagedMode;

#[derive(Debug, Clone)]
pub struct CsAuditResult {
    pub row_id: String,
    pub query_path: PathBuf,
    pub deployed_exists: bool,
    pub is_legacy: bool,
    pub fully_ok: usize,
    pub fully_drift: Vec<String>,
    pub ignore_present: usize,
    pub extra_blocks: Vec<String>,
    /// Region names parsed from the deployed file (for verbose reporting).
    pub deployed_region_names: Vec<String>,
}

impl CsAuditResult {
    pub fn compliant(&self) -> bool {
        if self.is_legacy {
            return true;
        }
        self.deployed_exists && self.fully_drift.is_empty()
    }
}

/// Resolve the deployed C# Query class path for an endpoint. Two layouts
/// exist in the TXN repo:
///   - bare:    `SqlQueries/{ClassName}Query.cs`               (e.g. CreateAccountQuery.cs)
///   - nested:  `SqlQueries/{Bundle}/{ClassName}Query.cs`      (e.g. Account/GetAccountByIdQuery.cs)
///
/// The class name comes from the part of `row.id` after the dot
/// (`Account.GetAccountById` → `GetAccountById`). This is the PascalCase
/// C# class name, distinct from the snake-style `verb`/`entity` fields
/// which map to SQL proc names. The bundle name (before the dot) is
/// the nested folder.
pub fn candidate_query_paths(cs_query_root: &Path, row: &EndpointRow) -> Vec<PathBuf> {
    // Use the row's own resolution helpers so `cs_class_override` is
    // honoured (e.g. `Account.UpdateAccount` whose deployed Query class
    // is `UpdateAccountByIdQuery.cs`, not `UpdateAccountQuery.cs`).
    let bundle_name = row.cs_bundle_folder();
    let class_stem = row.cs_class_stem();
    let file_name = format!("{}Query.cs", class_stem);
    vec![
        cs_query_root.join(&file_name),
        cs_query_root.join(&bundle_name).join(&file_name),
    ]
}

/// Resolve the actual deployed path (first candidate that exists), or the
/// nested form as the "expected" path when none exist yet.
pub fn deployed_query_path(cs_query_root: &Path, row: &EndpointRow) -> PathBuf {
    let candidates = candidate_query_paths(cs_query_root, row);
    for p in &candidates {
        if p.exists() {
            return p.clone();
        }
    }
    // Default to nested layout when neither exists — that's the convention
    // for newly-generated classes.
    candidates.into_iter().nth(1).unwrap()
}

/// Audit one endpoint's deployed Query class. Phase 1: parse + classify
/// only (legacy vs framework-managed). Drift detection against a rendered
/// canonical is Phase 2 — for now `fully_drift` stays empty when the file
/// is framework-managed but we have no canonical to diff against.
pub fn audit_cs_endpoint(
    cs_query_root: &Path,
    row: &EndpointRow,
) -> Result<CsAuditResult, String> {
    let path = deployed_query_path(cs_query_root, row);
    let mut res = CsAuditResult {
        row_id: row.id.clone(),
        query_path: path.clone(),
        deployed_exists: path.exists(),
        is_legacy: false,
        fully_ok: 0,
        fully_drift: Vec::new(),
        ignore_present: 0,
        extra_blocks: Vec::new(),
        deployed_region_names: Vec::new(),
    };
    if !res.deployed_exists {
        return Ok(res);
    }

    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let regions = parse_cs_regions(&text)?;

    if regions.is_empty() {
        res.is_legacy = true;
        return Ok(res);
    }

    classify_regions(&regions, &mut res);
    Ok(res)
}

fn classify_regions(regions: &[CsRegion], res: &mut CsAuditResult) {
    for r in regions {
        res.deployed_region_names.push(r.name.clone());
        match r.mode {
            ManagedMode::Fully => res.fully_ok += 1,
            ManagedMode::Ignore => res.ignore_present += 1,
            ManagedMode::Merge => res.fully_ok += 1,
        }
    }
}

/// Roll-up across endpoints.
#[derive(Debug, Clone, Default)]
pub struct CsAuditSummary {
    pub total: usize,
    pub framework_managed: usize,
    pub framework_compliant: usize,
    pub framework_defects: usize,
    pub legacy: usize,
    pub missing_files: usize,
}

impl CsAuditSummary {
    pub fn from_results(results: &[CsAuditResult]) -> Self {
        let mut s = Self::default();
        s.total = results.len();
        for r in results {
            if !r.deployed_exists {
                s.missing_files += 1;
            } else if r.is_legacy {
                s.legacy += 1;
            } else {
                s.framework_managed += 1;
                if r.compliant() {
                    s.framework_compliant += 1;
                } else {
                    s.framework_defects += 1;
                }
            }
        }
        s
    }

    pub fn real_defects(&self) -> usize {
        self.framework_defects + self.missing_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_row(id: &str, verb: &str, entity: &str) -> EndpointRow {
        EndpointRow {
            id: id.into(),
            shape: "command".into(),
            schema: Some("accounthosting".into()),
            entity: entity.into(),
            verb: verb.into(),
            entity_plural: None,
            method: None,
            path: "/accounts".into(),
            api_id: "00000000-0000-0000-0000-000000000000".into(),
            primary_table: "ach_Account".into(),
            action_type: "WRITE".into(),
            route_params: vec![],
            request_dto: None,
            response_dto: None,
            has_validation: false,
            has_child_tables: false,
            proc_name_override: None,
            description: None,
            author: None,
            create_date: None,
            change_log_rows: vec![],
            persistence_by_profile: vec![],
        }
    }

    #[test]
    fn candidate_paths_both_layouts() {
        let root = Path::new("/tmp/cs");
        let row = mk_row("Account.CreateAccount", "Create", "Account");
        let paths = candidate_query_paths(root, &row);
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("CreateAccountQuery.cs"));
        assert!(
            paths[1].ends_with("Account/CreateAccountQuery.cs")
                || paths[1].ends_with("Account\\CreateAccountQuery.cs")
        );
    }

    #[test]
    fn candidate_paths_use_id_not_verb_entity() {
        // Bundle uses snake-style entity (`Account_By_Id`) for SQL proc names,
        // but C# class name comes from the dotted id ("Account.GetAccountById").
        let root = Path::new("/tmp/cs");
        let row = mk_row("Account.GetAccountById", "Get", "Account_By_Id");
        let paths = candidate_query_paths(root, &row);
        assert!(paths[0].ends_with("GetAccountByIdQuery.cs"));
        assert!(
            paths[1].ends_with("Account/GetAccountByIdQuery.cs")
                || paths[1].ends_with("Account\\GetAccountByIdQuery.cs")
        );
    }
}
