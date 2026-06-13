//! Four-signal coverage emitter. Combines the four artefacts that
//! determine whether an endpoint is "Phase A — testable end-to-end"
//! or "Phase B — needs generation":
//!
//!   1. Dev Spec markdown exists for the (method, path) pair
//!   2. Registry row in `lookups.ars_Api_Rule_Settings`
//!   3. Bruno fixture in either collection root
//!   4. Stored proc bound (6-gate matcher → Single)
//!
//! Output: `5-deliverables/<CLIENT>/coverage.md` with per-row
//! classification driving the rest of the Phase A / Phase B workflow.

use crate::dev_spec::types::DevSpecEndpoint;
use crate::fitter::{FitReport, MissingOpAdd, MissingOpLog, RegistryEntry};
use crate::test_harness::bru_parse::{self, BruRequest};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Status assigned to one (method, path) endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageStatus {
    /// All four signals present — eligible for the test harness lifecycle.
    PhaseATestable,
    /// 1, 2, 4 ✓ but 3 ✗ — Bruno fixture missing or non-compliant.
    /// Action: hand-author one `.bru` file (the fixer can't help if the
    /// file doesn't exist).
    PhaseAFixBruno,
    /// 1, 2 ✓ but proc binding ambiguous (multiple candidates). Action:
    /// add an `ambiguity-resolutions.toml` entry.
    PhaseAResolveAmbiguity,
    /// 1, 2 ✓ but 3 ✗ AND 4 ✗ → run the txn-api-generator skill.
    PhaseBGenerateApi,
    /// 1, 2, 3 ✓ but 4 ✗ — proc missing. Action: txn-sql-generator skill.
    PhaseBGenerateSp,
    /// 1 ✗ — Dev Spec markdown missing. Action: human authors Dev Spec
    /// before any generator can be run.
    PhaseBAuthorDevSpec,
    /// 2 ✗ but 1 ✓ — Dev Spec drafted but not registered. Informational.
    SpecOnly,
}

impl CoverageStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PhaseATestable => "Phase A — testable",
            Self::PhaseAFixBruno => "Phase A — fix Bruno",
            Self::PhaseAResolveAmbiguity => "Phase A — resolve ambiguity",
            Self::PhaseBGenerateApi => "Phase B — generate C# + Bruno",
            Self::PhaseBGenerateSp => "Phase B — generate SP",
            Self::PhaseBAuthorDevSpec => "Phase B — author Dev Spec",
            Self::SpecOnly => "Spec-only (informational)",
        }
    }
    pub fn action(&self) -> &'static str {
        match self {
            Self::PhaseATestable => "run lifecycle",
            Self::PhaseAFixBruno => "hand-author .bru fixture",
            Self::PhaseAResolveAmbiguity => "add ambiguity-resolutions.toml entry",
            Self::PhaseBGenerateApi => "run txn-api-generator skill",
            Self::PhaseBGenerateSp => "run txn-sql-generator skill",
            Self::PhaseBAuthorDevSpec => "author Dev Spec markdown first",
            Self::SpecOnly => "register in ars_Api_Rule_Settings",
        }
    }
}

/// One row in the coverage table.
#[derive(Debug, Clone)]
pub struct CoverageRow {
    pub method: String,
    pub path: String,
    pub dev_spec: bool,
    pub registry: bool,
    pub bruno: bool,
    pub proc_bound: bool,
    pub proc_ambiguous: bool,
    pub status: CoverageStatus,
}

/// Build the coverage table from the four input artefacts.
///
/// `bruno_roots` is the list of Bruno collection root directories to
/// scan (typically two: `dtcard/4-expectations/BRU_Files` and
/// `dt/<CLIENT>/feapiTxnGlobal/.bruno/<CLIENT>-Global/1. Local`).
/// Missing roots are skipped silently.
pub fn build_coverage(
    dev_spec: &[DevSpecEndpoint],
    registry: &[RegistryEntry],
    fit_report: &FitReport,
    bruno_roots: &[PathBuf],
) -> Vec<CoverageRow> {
    // Index Dev Spec endpoints by (METHOD, normalised-path).
    let dev_keys: BTreeSet<(String, String)> = dev_spec
        .iter()
        .map(|e| (e.method.to_uppercase(), e.path.clone()))
        .collect();

    // Index registry rows by (METHOD, path).
    let registry_keys: BTreeSet<(String, String)> = registry
        .iter()
        .map(|r| {
            let m = r.method.as_deref().unwrap_or("GET").to_uppercase();
            (m, r.path.clone())
        })
        .collect();

    // Index Bruno fixtures from every collection root by (METHOD, path).
    // Strip the `{{baseURL}}` template prefix and `?query` so we compare
    // path-only to the spec.
    let mut bruno_keys: BTreeSet<(String, String)> = BTreeSet::new();
    for root in bruno_roots {
        let fixtures = bru_parse::load_collection(root).unwrap_or_default();
        for b in &fixtures {
            if let Some(path) = bruno_path_only(&b.url) {
                bruno_keys.insert((b.method.to_uppercase(), path));
            }
        }
    }

    // Index proc bindings from FitReport.
    // Single-bound: missing_ops_added.
    let bound: BTreeSet<(String, String)> = fit_report
        .missing_ops_added
        .iter()
        .map(|m: &MissingOpAdd| (m.method.to_uppercase(), m.path.clone()))
        .collect();
    // Ambiguous and Unbound both live in missing_ops_logged with reasons
    // distinguishing them. The reason starts with "AMBIGUOUS" for the
    // ambiguous bucket.
    let mut ambiguous: BTreeSet<(String, String)> = BTreeSet::new();
    for m in &fit_report.missing_ops_logged {
        if m.reason.starts_with("AMBIGUOUS") {
            let method = m.method.as_deref().unwrap_or("GET").to_uppercase();
            ambiguous.insert((method, m.path.clone()));
        }
    }

    // Union of every (method, path) seen across Dev Spec ∪ Registry —
    // these are the universe of endpoints we report on. Bruno fixtures
    // outside this universe are coverage-irrelevant (they'd be testing
    // endpoints nobody promised). Proc rows ditto — only matter when
    // there's a Dev Spec or registry row pointing at them.
    let mut universe: BTreeSet<(String, String)> = BTreeSet::new();
    universe.extend(dev_keys.iter().cloned());
    universe.extend(registry_keys.iter().cloned());

    let mut rows = Vec::with_capacity(universe.len());
    for (method, path) in &universe {
        let key = (method.clone(), path.clone());
        let dev = dev_keys.contains(&key);
        let reg = registry_keys.contains(&key);
        let bru = bruno_keys.contains(&key);
        let bound_ok = bound.contains(&key);
        let ambig = ambiguous.contains(&key);

        let status = classify(dev, reg, bru, bound_ok, ambig);

        rows.push(CoverageRow {
            method: method.clone(),
            path: path.clone(),
            dev_spec: dev,
            registry: reg,
            bruno: bru,
            proc_bound: bound_ok,
            proc_ambiguous: ambig,
            status,
        });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path).then(a.method.cmp(&b.method)));
    rows
}

fn classify(dev: bool, reg: bool, bru: bool, bound: bool, ambig: bool) -> CoverageStatus {
    if !dev {
        return CoverageStatus::PhaseBAuthorDevSpec;
    }
    if !reg {
        return CoverageStatus::SpecOnly;
    }
    if ambig {
        return CoverageStatus::PhaseAResolveAmbiguity;
    }
    match (bru, bound) {
        (true, true) => CoverageStatus::PhaseATestable,
        (false, true) => CoverageStatus::PhaseAFixBruno,
        (true, false) => CoverageStatus::PhaseBGenerateSp,
        (false, false) => CoverageStatus::PhaseBGenerateApi,
    }
}

/// Strip the `{{templateVar}}` base-URL prefix and `?query` suffix from
/// a Bruno URL value so what's left is a spec-comparable path. Returns
/// `None` if no path could be extracted.
fn bruno_path_only(url: &str) -> Option<String> {
    let after_base = if url.starts_with("{{") {
        let end = url.find("}}")? + 2;
        &url[end..]
    } else {
        url
    };
    let path = match after_base.find('?') {
        Some(q) => &after_base[..q],
        None => after_base,
    };
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Write the coverage report to `out_path`. The format is one summary
/// block + one table per status (so engineers see the work-list grouped
/// by required action).
pub fn write_report(
    rows: &[CoverageRow],
    client_hint: &str,
    out_path: &Path,
) -> Result<(), String> {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "# {} — endpoint coverage", client_hint).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "Endpoints assessed: {}", rows.len()).unwrap();
    writeln!(s).unwrap();
    writeln!(
        s,
        "Each row carries a 4-signal status: ✓/✗ for Dev Spec, Registry, \
         Bruno fixture, and stored proc binding. The classification then \
         routes the endpoint to the right next step (Phase A test, fix Bruno, \
         resolve ambiguity, run a generator, or author Dev Spec)."
    )
    .unwrap();
    writeln!(s).unwrap();

    // Summary by status.
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for r in rows {
        *counts.entry(r.status.label()).or_default() += 1;
    }
    writeln!(s, "## Summary").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "| Status | Count | Action |").unwrap();
    writeln!(s, "| --- | --- | --- |").unwrap();
    // Print in the natural workflow order, not alphabetical.
    let order = [
        CoverageStatus::PhaseATestable,
        CoverageStatus::PhaseAFixBruno,
        CoverageStatus::PhaseAResolveAmbiguity,
        CoverageStatus::PhaseBGenerateApi,
        CoverageStatus::PhaseBGenerateSp,
        CoverageStatus::PhaseBAuthorDevSpec,
        CoverageStatus::SpecOnly,
    ];
    for status in &order {
        let label = status.label();
        let count = counts.get(label).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        writeln!(s, "| {} | {} | {} |", label, count, status.action()).unwrap();
    }
    writeln!(s).unwrap();

    // Table per status with the actual endpoints.
    for status in &order {
        let label = status.label();
        let group: Vec<&CoverageRow> = rows.iter().filter(|r| r.status == *status).collect();
        if group.is_empty() {
            continue;
        }
        writeln!(s, "## {} ({})", label, group.len()).unwrap();
        writeln!(s).unwrap();
        writeln!(s, "Action: {}", status.action()).unwrap();
        writeln!(s).unwrap();
        writeln!(s, "| Method | Path | DevSpec | Registry | Bruno | Proc |").unwrap();
        writeln!(s, "| --- | --- | --- | --- | --- | --- |").unwrap();
        for r in group {
            let proc_cell = if r.proc_ambiguous {
                "⚠ ambig"
            } else if r.proc_bound {
                "✓"
            } else {
                "✗"
            };
            writeln!(
                s,
                "| {} | `{}` | {} | {} | {} | {} |",
                r.method,
                r.path,
                tick(r.dev_spec),
                tick(r.registry),
                tick(r.bruno),
                proc_cell,
            )
            .unwrap();
        }
        writeln!(s).unwrap();
    }

    std::fs::write(out_path, s).map_err(|e| {
        format!("write coverage report {}: {}", out_path.display(), e)
    })
}

fn tick(b: bool) -> &'static str {
    if b { "✓" } else { "✗" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(method: &str, path: &str) -> DevSpecEndpoint {
        DevSpecEndpoint {
            method: method.to_string(),
            path: path.to_string(),
            summary: String::new(),
            source_file: String::new(),
            path_params: Vec::new(),
            request_body: None,
            response_body: None,
        }
    }
    fn reg(method: &str, path: &str) -> RegistryEntry {
        RegistryEntry {
            api_id: uuid::Uuid::nil(),
            has_credential: true,
            path: path.to_string(),
            original_path: path.to_string(),
            method: Some(method.to_string()),
            enabled: true,
            source_table: "ars_Api_Rule_Settings".to_string(),
        }
    }
    fn bound(method: &str, path: &str) -> MissingOpAdd {
        MissingOpAdd {
            method: method.to_string(),
            path: path.to_string(),
            backed_by: "p_x".to_string(),
        }
    }

    #[test]
    fn classifies_phase_a_testable_when_all_four_present() {
        let dev = vec![dev("GET", "/binsponsors")];
        let registry = vec![reg("GET", "/binsponsors")];
        let mut report = FitReport::default();
        report.missing_ops_added.push(bound("GET", "/binsponsors"));

        // bruno_roots empty — we can't simulate bruno here without
        // touching disk. Instead test the classify() function directly.
        assert_eq!(classify(true, true, true, true, false), CoverageStatus::PhaseATestable);
        assert_eq!(classify(true, true, false, true, false), CoverageStatus::PhaseAFixBruno);
        assert_eq!(classify(true, true, true, false, false), CoverageStatus::PhaseBGenerateSp);
        assert_eq!(classify(true, true, false, false, false), CoverageStatus::PhaseBGenerateApi);
        assert_eq!(classify(false, true, false, false, false), CoverageStatus::PhaseBAuthorDevSpec);
        assert_eq!(classify(true, false, false, false, false), CoverageStatus::SpecOnly);
        assert_eq!(classify(true, true, false, false, true), CoverageStatus::PhaseAResolveAmbiguity);

        // Use the registry/dev/report inputs to also check build_coverage shape.
        let rows = build_coverage(&dev, &registry, &report, &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].method, "GET");
        assert_eq!(rows[0].path, "/binsponsors");
        // Bruno path is empty since no roots provided.
        assert!(!rows[0].bruno);
        assert!(rows[0].proc_bound);
        assert_eq!(rows[0].status, CoverageStatus::PhaseAFixBruno);
    }

    #[test]
    fn bruno_path_only_strips_template_and_query() {
        assert_eq!(
            bruno_path_only("{{localBaseURL}}/binsponsor/{binSponsorId}?page=1"),
            Some("/binsponsor/{binSponsorId}".to_string()),
        );
        assert_eq!(
            bruno_path_only("{{localBaseURL}}/binsponsors"),
            Some("/binsponsors".to_string()),
        );
        assert_eq!(
            bruno_path_only("/raw/path"),
            Some("/raw/path".to_string()),
        );
    }
}
