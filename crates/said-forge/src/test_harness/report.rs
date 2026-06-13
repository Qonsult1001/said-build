//! Test-run summary types + Markdown / JSON emitters.

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize)]
pub struct TestRunSummary {
    pub entity_count_total: usize,
    pub stopped_early: bool,
    pub entities: Vec<EntityResult>,
}

impl TestRunSummary {
    pub fn passed_count(&self) -> usize {
        self.entities.iter().filter(|e| e.passed).count()
    }
    pub fn failed_count(&self) -> usize {
        self.entities.iter().filter(|e| !e.passed && e.skipped_reason.is_none()).count()
    }
    /// Entities skipped because no Bruno fixtures existed for them at
    /// all — the lifecycle runner sets `skipped_reason` to a "no Bruno
    /// fixtures found" message in that case.
    pub fn missing_fixture_count(&self) -> usize {
        self.entities.iter().filter(|e| {
            e.skipped_reason.as_deref()
                .map(|r| r.contains("no Bruno fixtures") || r.contains("no Create fixture"))
                .unwrap_or(false)
        }).count()
    }
    /// Entities that ran the lifecycle but had at least one failing
    /// step — distinguishes "Bruno is missing" (skipped above) from
    /// "everything wired but proc returned an error" (broken).
    pub fn broken_step_count(&self) -> usize {
        self.entities.iter().filter(|e| {
            !e.passed && !e.operations.is_empty() && e.skipped_reason.is_none()
        }).count()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityResult {
    pub entity: String,
    pub passed: bool,
    pub operations: Vec<OperationResult>,
    pub skipped_reason: Option<String>,
}

impl EntityResult {
    pub fn new(entity: &str) -> Self {
        Self {
            entity: entity.to_string(),
            passed: false,
            operations: Vec::new(),
            skipped_reason: None,
        }
    }
    pub fn finalise(mut self) -> Self {
        self.passed = !self.operations.is_empty()
            && self.operations.iter().all(|o| o.passed)
            && self.skipped_reason.is_none();
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationResult {
    pub label: String,            // "POST create", "GET by id", ...
    pub fixture_name: String,     // Bruno file stem
    pub method: String,
    pub url: String,
    pub status_code: Option<u16>,
    pub passed: bool,
    pub failure_reason: Option<String>,
    pub response_body: Option<String>,
}

impl OperationResult {
    pub fn failed(label: &str, reason: &str) -> Self {
        Self {
            label: label.to_string(),
            fixture_name: String::new(),
            method: String::new(),
            url: String::new(),
            status_code: None,
            passed: false,
            failure_reason: Some(reason.to_string()),
            response_body: None,
        }
    }
}

/// Count unique (METHOD, normalised-URL) pairs across an op list,
/// deduplicating GET re-reads against the same {id} placeholder.
/// "Normalised" = strip query string + replace any UUID-shaped path
/// segment with `{id}` so two GETs against the same endpoint shape
/// (run before vs. after PUT) collapse to one.
fn unique_endpoints(ops: &[OperationResult]) -> usize {
    use std::collections::HashSet;
    let uuid_re = regex_lite();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for op in ops {
        let url = op.url.split('?').next().unwrap_or(&op.url);
        // Strip the host:port prefix if present.
        let path = match url.find("://") {
            Some(_) => {
                let rest = url.splitn(4, '/').nth(3).unwrap_or("");
                format!("/{}", rest)
            }
            None => url.to_string(),
        };
        // Replace any UUID-looking segment with {id} so query-by-uuid
        // GETs from before/after PUT match.
        let normalised: String = path
            .split('/')
            .map(|seg| if uuid_re(seg) { "{id}" } else { seg })
            .collect::<Vec<&str>>()
            .join("/");
        seen.insert((op.method.to_uppercase(), normalised));
    }
    seen.len()
}

/// Tiny inline UUID detector (8-4-4-4-12 hex with optional braces).
/// We avoid the `regex` crate dep here — a closure does the job.
fn regex_lite() -> impl Fn(&str) -> bool {
    |s: &str| {
        let s = s.trim_start_matches('{').trim_end_matches('}');
        let bytes = s.as_bytes();
        // Standard UUID is 36 chars: 8-4-4-4-12 with dashes at 8,13,18,23.
        if bytes.len() != 36 {
            return false;
        }
        for (i, b) in bytes.iter().enumerate() {
            let is_dash_pos = i == 8 || i == 13 || i == 18 || i == 23;
            if is_dash_pos {
                if *b != b'-' { return false; }
            } else if !b.is_ascii_hexdigit() {
                return false;
            }
        }
        true
    }
}

/// Persist `test-report.md` to `5-deliverables/<client>/` and a
/// machine-readable `txn-test-results.json` to `2-progress/`.
pub fn write_artifacts(
    summary: &TestRunSummary,
    deliverables_root: &Path,
    workspace_root: &Path,
) -> Result<(), String> {
    let md_path = deliverables_root.join("test-report.md");
    if let Some(parent) = md_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create parent: {}", e))?;
    }
    std::fs::write(&md_path, render_markdown(summary))
        .map_err(|e| format!("write {}: {}", md_path.display(), e))?;

    let progress_dir = workspace_root.join("2-progress");
    std::fs::create_dir_all(&progress_dir).map_err(|e| format!("create progress: {}", e))?;
    let json_path = progress_dir.join("test-results.json");
    let json = serde_json::to_string_pretty(summary)
        .map_err(|e| format!("serialise summary: {}", e))?;
    std::fs::write(&json_path, json)
        .map_err(|e| format!("write {}: {}", json_path.display(), e))?;
    Ok(())
}

fn render_markdown(summary: &TestRunSummary) -> String {
    let mut out = String::new();
    out.push_str("# API contract test report\n\n");

    // Top-line counts.
    out.push_str(&format!(
        "Entities walked: {} (build order). Outcomes: {} green, {} missing-fixture, {} broken-step.\n\n",
        summary.entity_count_total,
        summary.passed_count(),
        summary.missing_fixture_count(),
        summary.broken_step_count(),
    ));
    out.push_str(
        "The harness walks every entity in the build order without stopping, \
         running EVERY Bruno fixture in the entity's folder (root POST → \
         GET-by-id → PUT/PATCH → re-read → sub-resource ops → collection \
         list). Each section below groups entities by outcome: \"green\" \
         entities passed every fixture against the live sandbox; \"missing-\
         fixture\" entities had no Bruno files at all, so no lifecycle could \
         run; \"broken-step\" entities had at least one fixture return an \
         error envelope, a non-2xx HTTP status, or a URL the OpenAPI spec \
         doesn't model.\n\n",
    );

    // Section: green.
    let green: Vec<&EntityResult> = summary.entities.iter().filter(|e| e.passed).collect();
    if !green.is_empty() {
        out.push_str(&format!("## ✓ Green ({})\n\n", green.len()));
        out.push_str(
            "Every Bruno fixture in the entity's folder ran successfully \
             against the live sandbox. **Endpoints** is the count of unique \
             `(method, path)` pairs (after collapsing UUIDs and dropping query \
             strings); **Runs** is the raw fixture-invocation count, which can \
             exceed Endpoints because GET-by-id is re-run after PUT as a \
             read-back sanity check.\n\n",
        );
        out.push_str("| Entity | Endpoints | Runs |\n");
        out.push_str("| --- | --- | --- |\n");
        for e in &green {
            let unique = unique_endpoints(&e.operations);
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                e.entity, unique, e.operations.len()
            ));
        }
        out.push('\n');
    }

    // Section: missing-fixture.
    let missing: Vec<&EntityResult> = summary.entities.iter().filter(|e| {
        e.skipped_reason.as_deref()
            .map(|r| r.contains("no Bruno fixtures") || r.contains("no Create fixture"))
            .unwrap_or(false)
    }).collect();
    if !missing.is_empty() {
        out.push_str(&format!("## ○ Missing fixture ({})\n\n", missing.len()));
        out.push_str(
            "Bruno collection has no fixtures for these entities at all. \
             Action per the workflow: hand-author one `.bru` file per entity. \
             Until then the lifecycle can't run.\n\n",
        );
        out.push_str("| Entity | Reason |\n");
        out.push_str("| --- | --- |\n");
        for e in &missing {
            out.push_str(&format!(
                "| {} | {} |\n",
                e.entity,
                e.skipped_reason.as_deref().unwrap_or(""),
            ));
        }
        out.push('\n');
    }

    // Section: broken-step.
    let broken: Vec<&EntityResult> = summary.entities.iter().filter(|e| {
        !e.passed && !e.operations.is_empty() && e.skipped_reason.is_none()
    }).collect();
    if !broken.is_empty() {
        out.push_str(&format!("## ✗ Broken step ({})\n\n", broken.len()));
        out.push_str(
            "These entities ran lifecycle but at least one step failed. \
             Each row shows the first failing step's URL + the proc's error \
             envelope so the cause is identifiable without re-running.\n\n",
        );
        for e in &broken {
            out.push_str(&format!("### {}\n\n", e.entity));
            out.push_str("| Step | Method | URL | Status | Outcome |\n");
            out.push_str("| --- | --- | --- | --- | --- |\n");
            for op in &e.operations {
                let outcome = if op.passed {
                    "pass".to_string()
                } else {
                    op.failure_reason.clone()
                        .unwrap_or_else(|| "fail (no reason captured)".to_string())
                };
                let status = op.status_code
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string());
                out.push_str(&format!(
                    "| {} | `{}` | `{}` | {} | {} |\n",
                    op.label, op.method, op.url, status, escape_md(&outcome),
                ));
            }
            out.push('\n');
        }
    }

    out
}

fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}
