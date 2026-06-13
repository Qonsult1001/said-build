//! Auditor — re-renders each endpoint's expected SQL and diffs it against
//! the deployed file by region.
//!
//! Two modes (the Option 3 contract):
//!   - `framework-managed`: deployed file has `@said-managed` markers.
//!     Every Fully region must match canonical. Audit failure = defect.
//!   - `legacy`: deployed file has NO markers. Reported informationally;
//!     not a defect. Touch-it-migrate-it from there.
//!
//! Text-level diff for parity with Python. The hooks are here for future
//! `sca-core::ast_chunk` AST-aware checks when a case forces it.

use std::collections::BTreeMap;
use std::path::Path;

use super::manifest::EndpointRow;
use super::markers::{parse_regions, ManagedMode};
use super::render::render_endpoint;

#[derive(Debug, Clone)]
pub struct AuditResult {
    pub row_id: String,
    pub deployed_exists: bool,
    pub is_legacy: bool,
    pub fully_ok: usize,
    pub fully_drift: Vec<String>,
    pub ignore_present: usize,
    pub ignore_missing: Vec<String>,
    pub extra_blocks: Vec<String>,
    pub missing_blocks: Vec<String>,
}

impl AuditResult {
    pub fn compliant(&self) -> bool {
        if self.is_legacy {
            return true; // legacy is informational, not a defect
        }
        self.deployed_exists
            && self.fully_drift.is_empty()
            && self.ignore_missing.is_empty()
            && self.missing_blocks.is_empty()
    }
}

/// Audit one endpoint against its deployed file. `framework_root` is the
/// path to `dtcard/.forge/proc-framework/`. `deployed_root` is the path
/// where deployed procs live (e.g. `dtcard/1-ground-truth/TXN/.../TxnMasterSQL`).
pub fn audit_endpoint(
    framework_root: &Path,
    profile: &str,
    row: &EndpointRow,
    deployed_root: &Path,
) -> Result<AuditResult, String> {
    let deployed_path = row.deployed_path(deployed_root);
    let mut res = AuditResult {
        row_id: row.id.clone(),
        deployed_exists: deployed_path.exists(),
        is_legacy: false,
        fully_ok: 0,
        fully_drift: Vec::new(),
        ignore_present: 0,
        ignore_missing: Vec::new(),
        extra_blocks: Vec::new(),
        missing_blocks: Vec::new(),
    };
    if !res.deployed_exists {
        return Ok(res);
    }

    let deployed_text = std::fs::read_to_string(&deployed_path)
        .map_err(|e| format!("read {}: {}", deployed_path.display(), e))?;
    let rendered_text = render_endpoint(framework_root, profile, row, Some(&deployed_path))?;

    let deployed_regions = parse_regions(&deployed_text)?;
    let rendered_regions = parse_regions(&rendered_text)?;

    // No markers in deployed = legacy. Informational only.
    if deployed_regions.is_empty() {
        res.is_legacy = true;
        return Ok(res);
    }

    let deployed_by_name: BTreeMap<&str, &super::markers::Region> = deployed_regions
        .iter()
        .map(|r| (r.name.as_str(), r))
        .collect();

    let rendered_by_name: BTreeMap<&str, &super::markers::Region> = rendered_regions
        .iter()
        .map(|r| (r.name.as_str(), r))
        .collect();

    // Walk rendered regions: check each against deployed.
    for (name, rr) in &rendered_by_name {
        match deployed_by_name.get(name) {
            None => {
                res.missing_blocks
                    .push(format!("{}:{}", rr.mode.as_str(), name));
            }
            Some(dr) => match rr.mode {
                ManagedMode::Fully => {
                    if normalize_for_diff(&dr.body) == normalize_for_diff(&rr.body) {
                        res.fully_ok += 1;
                    } else {
                        res.fully_drift.push((*name).to_string());
                    }
                }
                ManagedMode::Ignore => {
                    res.ignore_present += 1;
                }
                ManagedMode::Merge => {
                    // Merge audit is not yet defined — record as ok if names match.
                    res.fully_ok += 1;
                }
            },
        }
    }

    // Extra regions in deployed that the framework doesn't render — author
    // additions; informational, not a defect.
    for name in deployed_by_name.keys() {
        if !rendered_by_name.contains_key(name) {
            res.extra_blocks.push((*name).to_string());
        }
    }

    Ok(res)
}

/// Trailing-whitespace + final-newline insensitive equality. Matches
/// Python's `normalize_for_diff`.
fn normalize_for_diff(s: &str) -> String {
    let mut lines: Vec<String> = s.lines().map(|l| l.trim_end().to_string()).collect();
    while let Some(last) = lines.last() {
        if last.is_empty() {
            lines.pop();
        } else {
            break;
        }
    }
    lines.join("\n")
}

/// Roll-up summary across multiple endpoints.
#[derive(Debug, Clone, Default)]
pub struct AuditSummary {
    pub total: usize,
    pub framework_managed: usize,
    pub framework_compliant: usize,
    pub framework_defects: usize,
    pub legacy: usize,
    pub missing_files: usize,
}

impl AuditSummary {
    pub fn from_results(results: &[AuditResult]) -> Self {
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
