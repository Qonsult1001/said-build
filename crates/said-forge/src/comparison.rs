//! Per-epic comparison report — SQL source-of-truth vs OpenAPI
//! wishlist vs Dev Planning markdown.
//!
//! For each epic (currently driven by an SQL schema name) the report
//! enumerates every proposed op across the three sources, picks the
//! single best match per row, and emits two views into one file:
//!
//! - **Medium** — wide alignment table + one-paragraph SQL truth
//!   summary per op (tables touched, wipe-on-missing flags, conditional
//!   deletes).
//! - **Heavy** — full proc analysis: @param list, OPENJSON keys,
//!   bilateral diff against OpenAPI/Dev Planning fields, error codes.
//!
//! Each row carries a **Decision** column with forge's auto-suggestion:
//!
//! | Evidence | Suggested decision |
//! |---|---|
//! | All three sources agree on verb + path | `Aligned — implement as documented` |
//! | Verb mismatch | `⚠ Resolve verb` (lists each source's verb) |
//! | Path mismatch | `⚠ Resolve path` (lists each source's path) |
//! | OpenAPI says yes, SQL has no proc | `Build proc — endpoint proposed but not implemented` |
//! | SQL has proc, OpenAPI doesn't list it, but proc is called from another proc | `Keep — internal helper called by [list]` |
//! | SQL has proc, no OpenAPI, no internal callers, no C# callers | `Investigate — possible dead code` |
//! | SQL has proc, name suggests lifecycle/audit (status, transition, history) | `Keep — lifecycle support` |

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::directive::OpSpec;
use crate::proc_analysis::{analyse_proc, extract_literals, ProcAnalysis};
use crate::sql_catalog::{SqlCatalog, SqlKind, SqlObject};

// ─────────────────────────── public API ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub epic: String,
    pub rows: Vec<ComparisonRow>,
    pub orphan_procs: Vec<ComparisonRow>,
    /// Workspace-wide reverse index: `value → list of occurrences`.
    /// Built once per report. Used by the heavy section to render
    /// "this literal also appears in …" lineage.
    #[serde(default)]
    pub literal_index: LiteralIndex,
    /// Workspace-wide error-code description map. Built by harvesting
    /// `-- [<code>] - <description>` inline comments across every
    /// SQL file.
    #[serde(default)]
    pub error_code_index: ErrorCodeIndex,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiteralIndex {
    pub by_value: BTreeMap<String, Vec<LiteralLocation>>,
}

/// Workspace-wide error-code → human-description map. Built by
/// scanning every SQL file for the inline comment convention used
/// in dtcard:
///
/// ```sql
/// WHERE prc_Code = 60105    -- [60105] - Invalid phone number entered
/// ```
///
/// The post-`--` comment is the canonical description. When the
/// comment is absent the code maps to `None` and renders as
/// "(no description found in any proc comment)".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorCodeIndex {
    pub by_code: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiteralLocation {
    /// Workspace-relative file path.
    pub file: String,
    pub line: usize,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonRow {
    pub op_label: String,
    pub openapi: Option<OpRef>,
    pub sql_proc: Option<String>,
    pub dev_planning: Option<OpRef>,
    pub status: AlignmentStatus,
    pub decision: String,
    pub proc_analysis: Option<ProcAnalysis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpRef {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AlignmentStatus {
    Aligned,
    VerbMismatch,
    PathMismatch,
    SqlMissing,
    OpenApiMissing,
    DevPlanningMissing,
    OrphanProc,
}

/// Walk SQL + C# files under the workspace and build a value-level
/// index. Each literal value (GUID, short code) gets the list of
/// `(file, line, context)` tuples where it appears. Used by the
/// heavy section to render lineage.
///
/// `cs_root_override` lets callers point the C# scan at a folder
/// outside the workspace. When `None`, forge falls back to
/// `<workspace_root>/2-progress/` and silently skips it when that
/// folder is missing or empty (SQL-only workspaces still work).
pub fn build_literal_index(
    workspace_root: &Path,
    cs_root_override: Option<&Path>,
) -> LiteralIndex {
    let mut index: BTreeMap<String, Vec<LiteralLocation>> = BTreeMap::new();
    let sql_root = workspace_root.join("1-ground-truth");
    let cs_root = match cs_root_override {
        Some(p) => p.to_path_buf(),
        None => workspace_root.join("2-progress"),
    };

    let mut files: Vec<(PathBuf, &'static str)> = Vec::new();
    walk_files(&sql_root, "sql", &mut files);
    walk_files(&cs_root, "cs", &mut files);

    for (file, kind) in &files {
        let body = match std::fs::read_to_string(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let occurrences: Vec<crate::proc_analysis::LiteralOccurrence> = match *kind {
            "sql" => extract_literals(&body),
            "cs" => extract_literals_cs(&body),
            _ => Vec::new(),
        };
        let rel = file
            .strip_prefix(workspace_root)
            .unwrap_or(file)
            .display()
            .to_string();
        for occ in occurrences {
            index
                .entry(occ.value.to_ascii_uppercase())
                .or_default()
                .push(LiteralLocation {
                    file: rel.clone(),
                    line: occ.line,
                    context: occ.context,
                });
        }
    }
    LiteralIndex { by_value: index }
}

fn walk_files(root: &Path, ext: &'static str, out: &mut Vec<(PathBuf, &'static str)>) {
    if !root.is_dir() {
        return;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
            {
                out.push((p, ext));
            }
        }
    }
}

/// Lift literal extraction for C# files. C# uses double-quoted
/// strings — we run the same GUID + short-code matchers on each line
/// but interpret quotes as `"..."` rather than `'...'`.
fn extract_literals_cs(body: &str) -> Vec<crate::proc_analysis::LiteralOccurrence> {
    use crate::proc_analysis::LiteralOccurrence;
    let mut out: Vec<LiteralOccurrence> = Vec::new();
    for (lineno_zero, raw_line) in body.lines().enumerate() {
        let line_num = lineno_zero + 1;
        // GUID anywhere in the line, with or without surrounding "..."
        let bytes = raw_line.as_bytes();
        let mut i = 0;
        while i + 36 <= bytes.len() {
            // Check for raw GUID at i (or "GUID" pattern).
            let candidate = &bytes[i..i + 36];
            let dashes_ok = candidate.get(8) == Some(&b'-')
                && candidate.get(13) == Some(&b'-')
                && candidate.get(18) == Some(&b'-')
                && candidate.get(23) == Some(&b'-');
            let hex_ok = candidate.iter().enumerate().all(|(idx, b)| {
                if [8, 13, 18, 23].contains(&idx) {
                    true
                } else {
                    b.is_ascii_hexdigit()
                }
            });
            if dashes_ok && hex_ok {
                let value = std::str::from_utf8(candidate)
                    .unwrap_or("")
                    .to_ascii_uppercase();
                if !value.starts_with("00000000-0000-0000-0000-") {
                    out.push(LiteralOccurrence {
                        value,
                        line: line_num,
                        context: short_context_helper(raw_line),
                    });
                }
                i += 36;
                continue;
            }
            i += 1;
        }
        // Short codes inside double quotes.
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'"' {
                    j += 1;
                }
                if j > start && j < bytes.len() {
                    let lit = &raw_line[start..j];
                    if cs_looks_like_code(lit) {
                        out.push(LiteralOccurrence {
                            value: lit.to_string(),
                            line: line_num,
                            context: short_context_helper(raw_line),
                        });
                    }
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

fn cs_looks_like_code(s: &str) -> bool {
    let len = s.chars().count();
    if !(4..=15).contains(&len) {
        return false;
    }
    if s.contains(' ') || s.contains('.') || s.contains(',') {
        return false;
    }
    let mut digits = 0usize;
    let mut max_upper_run = 0usize;
    let mut upper_run = 0usize;
    for c in s.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' {
            return false;
        }
        if c.is_ascii_digit() {
            digits += 1;
        }
        if c.is_ascii_uppercase() {
            upper_run += 1;
            if upper_run > max_upper_run {
                max_upper_run = upper_run;
            }
        } else {
            upper_run = 0;
        }
    }
    digits >= 1 || max_upper_run >= 3
}

fn short_context_helper(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= 80 {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..80])
    }
}

/// Walk every SQL file under `1-ground-truth/` and harvest the
/// inline comment convention `-- [<code>] - <description>` next to
/// `prc_Code = <num>` lines. Returns a `code → description` map. If
/// the same code is described differently in two procs (e.g. a typo),
/// the first occurrence wins — review the comparison-report orphan
/// list to spot drift.
pub fn build_error_code_index(workspace_root: &Path) -> ErrorCodeIndex {
    let sql_root = workspace_root.join("1-ground-truth");
    let mut files: Vec<(PathBuf, &'static str)> = Vec::new();
    walk_files(&sql_root, "sql", &mut files);

    let mut by_code: BTreeMap<String, String> = BTreeMap::new();
    for (file, _) in &files {
        let body = match std::fs::read_to_string(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        for line in body.lines() {
            // Match `prc_Code = <num>` or `prc_Desc = <num>` followed
            // by `-- [<code>] - <description>` on the same line.
            // Tolerates extra whitespace.
            let lower = line.to_ascii_lowercase();
            let needle = lower.find("prc_code = ").or_else(|| lower.find("prc_desc = "));
            let Some(eq_idx) = needle else { continue };
            let after = &line[eq_idx + "prc_code = ".len()..];
            // Take digits / minus.
            let mut end = 0;
            for c in after.chars() {
                if c.is_ascii_digit() || c == '-' {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }
            if end == 0 {
                continue;
            }
            let code = after[..end].trim().to_string();
            // Find the comment marker after the number.
            let tail = &after[end..];
            let Some(comment_start) = tail.find("--") else {
                continue;
            };
            let comment = tail[comment_start + 2..].trim();
            // Expect `[<code>] - <description>`.
            let Some(open) = comment.find('[') else {
                continue;
            };
            let Some(close) = comment.find(']') else {
                continue;
            };
            if close <= open {
                continue;
            }
            let bracketed = &comment[open + 1..close];
            if bracketed.trim() != code {
                // Mismatch — skip rather than corrupt the index.
                continue;
            }
            let desc_start = close + 1;
            let raw_desc = comment[desc_start..]
                .trim_start()
                .trim_start_matches('-')
                .trim();
            if raw_desc.is_empty() {
                continue;
            }
            // First write wins.
            by_code.entry(code).or_insert_with(|| raw_desc.to_string());
        }
    }
    ErrorCodeIndex { by_code }
}

/// Build the per-epic comparison report.
///
/// - `epic` — display label, e.g. `"Cardholder Management"`.
/// - `sql_schema` — SQL schema name to scope ops by, e.g. `"cardholder"`.
/// - `catalog` — full SqlCatalog (procs from every schema; we filter).
/// - `openapi_ops` — every op extracted from the OpenAPI yaml.
/// - `dev_planning_ops` — every op extracted from Dev Planning markdown.
/// - `sql_root` — workspace root (for re-reading proc files).
pub fn build_comparison(
    epic: &str,
    sql_schema: &str,
    catalog: &SqlCatalog,
    openapi_ops: &[OpSpec],
    dev_planning_ops: &[OpSpec],
    sql_root: &Path,
    workspace_root: &Path,
    cs_root_override: Option<&Path>,
) -> ComparisonReport {
    let in_scope_procs: Vec<&SqlObject> = catalog
        .objects
        .iter()
        .filter(|o| o.kind == SqlKind::Procedure)
        .filter(|o| {
            o.schema
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case(sql_schema))
                .unwrap_or(false)
        })
        .collect();

    // Build candidate row set: union of OpenAPI + Dev Planning ops
    // that look like they belong to this epic (path contains the
    // schema name or its singular).
    let entity_token = singular(sql_schema);
    let belongs = |path: &str| {
        let p = path.to_ascii_lowercase();
        p.contains(sql_schema) || p.contains(&entity_token)
    };

    let openapi_for_epic: Vec<&OpSpec> = openapi_ops
        .iter()
        .filter(|o| o.path.as_deref().map(belongs).unwrap_or(false))
        .collect();
    let dev_for_epic: Vec<&OpSpec> = dev_planning_ops
        .iter()
        .filter(|o| o.path.as_deref().map(belongs).unwrap_or(false))
        .collect();

    let mut rows: Vec<ComparisonRow> = Vec::new();
    let mut consumed_dev: BTreeSet<String> = BTreeSet::new();
    let mut consumed_proc: BTreeSet<String> = BTreeSet::new();

    // Walk OpenAPI ops first — that's the client wishlist, the most
    // authoritative source for "what should exist".
    for oa in &openapi_for_epic {
        let proc = match_proc_for_op(oa, &in_scope_procs);
        let dev = match_dev_for_op(oa, &dev_for_epic);
        if let Some(p) = proc {
            consumed_proc.insert(p.full_name());
        }
        if let Some(d) = dev {
            consumed_dev.insert(d.slug.clone());
        }
        rows.push(build_row(
            epic,
            Some(*oa),
            proc,
            dev,
            sql_root,
            workspace_root,
        ));
    }

    // Dev Planning ops not yet consumed (Dev Planning has an op the
    // OpenAPI doesn't).
    for dv in &dev_for_epic {
        if consumed_dev.contains(&dv.slug) {
            continue;
        }
        let proc = match_proc_for_op(dv, &in_scope_procs);
        if let Some(p) = proc {
            consumed_proc.insert(p.full_name());
        }
        rows.push(build_row(
            epic,
            None,
            proc,
            Some(*dv),
            sql_root,
            workspace_root,
        ));
    }

    // Procs left unmatched go to the orphan section.
    let mut orphan_procs: Vec<ComparisonRow> = Vec::new();
    for p in &in_scope_procs {
        if consumed_proc.contains(&p.full_name()) {
            continue;
        }
        orphan_procs.push(build_orphan_row(p, &in_scope_procs, sql_root, workspace_root));
    }

    let literal_index = build_literal_index(workspace_root, cs_root_override);
    let error_code_index = build_error_code_index(workspace_root);

    ComparisonReport {
        epic: epic.to_string(),
        rows,
        orphan_procs,
        literal_index,
        error_code_index,
    }
}

fn build_row(
    _epic: &str,
    openapi: Option<&OpSpec>,
    proc: Option<&SqlObject>,
    dev: Option<&OpSpec>,
    sql_root: &Path,
    _workspace_root: &Path,
) -> ComparisonRow {
    let oa_ref = openapi.map(|o| OpRef {
        method: o.method.clone().unwrap_or_default(),
        path: o.path.clone().unwrap_or_default(),
    });
    let dv_ref = dev.map(|o| OpRef {
        method: o.method.clone().unwrap_or_default(),
        path: o.path.clone().unwrap_or_default(),
    });
    let label = openapi
        .or(dev)
        .map(|o| {
            format!(
                "{} {}",
                o.method.clone().unwrap_or_default(),
                o.path.clone().unwrap_or_default()
            )
        })
        .unwrap_or_else(|| proc.map(|p| p.full_name()).unwrap_or_default());

    let proc_present = proc.is_some();
    let status = compute_status(&oa_ref, &dv_ref, proc_present);
    let decision = decide(&oa_ref, &dv_ref, proc, status);
    let analysis = proc.map(|p| analyse_proc(p, sql_root));

    ComparisonRow {
        op_label: label,
        openapi: oa_ref,
        sql_proc: proc.map(|p| p.full_name()),
        dev_planning: dv_ref,
        status,
        decision,
        proc_analysis: analysis,
    }
}

fn build_orphan_row(
    proc: &SqlObject,
    all_procs: &[&SqlObject],
    sql_root: &Path,
    _workspace_root: &Path,
) -> ComparisonRow {
    let analysis = analyse_proc(proc, sql_root);
    let callers = find_proc_callers(proc, all_procs, sql_root);
    let decision = decide_orphan(proc, &callers);
    ComparisonRow {
        op_label: proc.full_name(),
        openapi: None,
        sql_proc: Some(proc.full_name()),
        dev_planning: None,
        status: AlignmentStatus::OrphanProc,
        decision,
        proc_analysis: Some(analysis),
    }
}

// ─────────────────────────── matchers ───────────────────────────

/// Pick the best stored proc for an API op. Strategy: tokenise the op
/// path + verb, then score procs by token-overlap on the proc name.
/// Verb token (Create/Update/Get) gets a strong boost; entity token
/// (Cardholder/Card/etc.) is required.
fn match_proc_for_op<'a>(op: &OpSpec, procs: &[&'a SqlObject]) -> Option<&'a SqlObject> {
    let method = op.method.as_deref().unwrap_or("").to_ascii_uppercase();
    let path = op.path.as_deref().unwrap_or("");
    let verb_tokens = verb_for_method(&method);
    let path_tokens: Vec<String> = path
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty() && t.len() > 2 && !is_id_placeholder(t))
        .map(|t| t.to_ascii_lowercase())
        .collect();

    let mut scored: Vec<(&'a SqlObject, i32)> = procs
        .iter()
        .map(|p| (*p, score_proc(p, verb_tokens, &path_tokens)))
        .filter(|(_, s)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.first().map(|(p, _)| *p)
}

fn match_dev_for_op<'a>(op: &OpSpec, dev_ops: &[&'a OpSpec]) -> Option<&'a OpSpec> {
    let needle_path = op.path.as_deref().unwrap_or("").to_ascii_lowercase();
    let needle_method = op.method.as_deref().unwrap_or("").to_ascii_uppercase();

    // First pass: same method + same path, modulo /v1/ + plural-singular.
    let normal = |s: &str| {
        s.to_ascii_lowercase()
            .replace("/v1/", "/")
            .replace("cardholders", "cardholder")
            .replace("cards", "card")
            .replace("bins", "bin")
            .replace("binsponsors", "binsponsor")
            .replace("accounts", "account")
            .replace("transitions", "transition")
    };
    let need_norm = normal(&needle_path);
    for d in dev_ops {
        let dpath = d.path.as_deref().unwrap_or("").to_ascii_lowercase();
        let dm = d.method.as_deref().unwrap_or("").to_ascii_uppercase();
        if dm == needle_method && normal(&dpath) == need_norm {
            return Some(*d);
        }
    }
    // Second pass: method-agnostic, same normalised path.
    for d in dev_ops {
        let dpath = d.path.as_deref().unwrap_or("").to_ascii_lowercase();
        if normal(&dpath) == need_norm {
            return Some(*d);
        }
    }
    None
}

fn verb_for_method(method: &str) -> &'static [&'static str] {
    match method {
        "POST" => &["create"],
        "PUT" => &["update", "create"],
        "PATCH" => &["update"],
        "GET" => &["get", "retrieve", "list"],
        "DELETE" => &["delete", "remove"],
        _ => &[],
    }
}

fn score_proc(proc: &SqlObject, verbs: &[&str], path_tokens: &[String]) -> i32 {
    let name_lc = proc.name.to_ascii_lowercase();
    let mut score = 0i32;
    // Verb match — required, otherwise return 0.
    if !verbs.iter().any(|v| name_lc.contains(v)) {
        return 0;
    }
    score += 5;
    for tok in path_tokens {
        let singular = singular(tok);
        if name_lc.contains(&singular) {
            score += 3;
        } else if name_lc.contains(tok) {
            score += 2;
        }
    }
    // Penalty for procs whose name contains a token NOT in the path —
    // e.g. `_Status_` or `_Transition_` shouldn't match "GET /cardholder".
    for keyword in ["status", "transition"] {
        let in_name = name_lc.contains(keyword);
        let in_path = path_tokens.iter().any(|t| t.contains(keyword));
        if in_name && !in_path {
            score -= 4;
        }
    }
    // Specificity bonus — longer path with more matched tokens beats
    // a shorter path that just shares the entity. We already give +3
    // per matched token, so this falls out naturally.
    score
}

fn singular(s: &str) -> String {
    if s.ends_with("ies") && s.len() > 3 {
        let mut o = s[..s.len() - 3].to_string();
        o.push('y');
        return o;
    }
    if s.ends_with('s') && !s.ends_with("ss") {
        return s[..s.len() - 1].to_string();
    }
    s.to_string()
}

fn is_id_placeholder(t: &str) -> bool {
    let lc = t.to_ascii_lowercase();
    matches!(lc.as_str(), "id" | "guid" | "uuid")
}

// ─────────────────────────── status + decision ───────────────────────────

fn compute_status(
    oa: &Option<OpRef>,
    dv: &Option<OpRef>,
    sql_present: bool,
) -> AlignmentStatus {
    if oa.is_some() && !sql_present {
        return AlignmentStatus::SqlMissing;
    }
    if oa.is_none() && dv.is_some() {
        return AlignmentStatus::OpenApiMissing;
    }
    if oa.is_some() && dv.is_none() {
        return AlignmentStatus::DevPlanningMissing;
    }
    if let (Some(a), Some(d)) = (oa, dv) {
        if !a.method.eq_ignore_ascii_case(&d.method) {
            return AlignmentStatus::VerbMismatch;
        }
        if !paths_equivalent(&a.path, &d.path) {
            return AlignmentStatus::PathMismatch;
        }
    }
    AlignmentStatus::Aligned
}

fn paths_equivalent(a: &str, b: &str) -> bool {
    let n = |s: &str| {
        s.to_ascii_lowercase()
            .replace("/v1/", "/")
            .replace("cardholders", "cardholder")
            .replace("cards", "card")
            .replace("bins", "bin")
            .replace("transitions", "transition")
    };
    n(a) == n(b)
}

fn decide(
    oa: &Option<OpRef>,
    dv: &Option<OpRef>,
    proc: Option<&SqlObject>,
    status: AlignmentStatus,
) -> String {
    match status {
        AlignmentStatus::Aligned => {
            if proc.is_some() {
                "Aligned — implement as documented; SQL proc exists.".into()
            } else {
                "Aligned — but no SQL proc found. Build the proc next sprint.".into()
            }
        }
        AlignmentStatus::VerbMismatch => format!(
            "⚠ Resolve verb conflict: OpenAPI says `{}`, Dev Planning says `{}`. \
             Cross-check against the SQL proc body to find which side matches the \
             actual behaviour.",
            oa.as_ref().map(|o| o.method.as_str()).unwrap_or("?"),
            dv.as_ref().map(|d| d.method.as_str()).unwrap_or("?"),
        ),
        AlignmentStatus::PathMismatch => format!(
            "⚠ Resolve path conflict: OpenAPI `{}` vs Dev Planning `{}`.",
            oa.as_ref().map(|o| o.path.as_str()).unwrap_or("?"),
            dv.as_ref().map(|d| d.path.as_str()).unwrap_or("?"),
        ),
        AlignmentStatus::SqlMissing => {
            "Build SQL proc — OpenAPI proposes this op but no implementation found in \
             `1-ground-truth/`."
                .into()
        }
        AlignmentStatus::OpenApiMissing => {
            "⚠ Add to OpenAPI — Dev Planning describes this op but the client \
             wishlist (OpenAPI yaml) doesn't expose it."
                .into()
        }
        AlignmentStatus::DevPlanningMissing => {
            "⚠ Author Dev Planning markdown — OpenAPI proposes this but no \
             internal spec exists yet."
                .into()
        }
        AlignmentStatus::OrphanProc => {
            "Orphan proc — see Decision column in the orphan section.".into()
        }
    }
}

fn decide_orphan(proc: &SqlObject, callers: &[String]) -> String {
    if !callers.is_empty() {
        return format!(
            "Keep — internal helper called by: {}",
            callers.join(", ")
        );
    }
    let lc = proc.name.to_ascii_lowercase();
    if lc.contains("transition") || lc.contains("status") || lc.contains("history") {
        return "Keep — lifecycle / audit support, even without a public API.".into();
    }
    if lc.contains("client_profile") || lc.contains("profile_data") {
        return "Investigate — possible internal helper or dead code. \
             Confirm with schema owner before removing."
            .into();
    }
    "Investigate — no callers found in SQL or APIs; possible dead code.".into()
}

fn find_proc_callers(
    proc: &SqlObject,
    all_procs: &[&SqlObject],
    sql_root: &Path,
) -> Vec<String> {
    let needle = format!("[{}]", proc.name);
    let needle_lc = needle.to_ascii_lowercase();
    let bare_lc = proc.name.to_ascii_lowercase();
    let mut out = Vec::new();
    for other in all_procs {
        if other.name == proc.name {
            continue;
        }
        let path = sql_root.join(&other.source_path);
        let body = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let body_lc = body.to_ascii_lowercase();
        if body_lc.contains(&needle_lc) || body_lc.contains(&format!("exec {}", bare_lc)) {
            out.push(other.full_name());
        }
    }
    out
}

// ─────────────────────────── renderers ───────────────────────────

pub fn render_report(report: &ComparisonReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Comparison Report — {}\n\n", report.epic));
    out.push_str(
        "_Source-of-truth ranking: **SQL** (law) > **OpenAPI** (client wishlist) \
         > **Dev Planning** (internal interpretation). Mismatches are flagged \
         for resolution; the SQL body is authoritative on what the system \
         actually does._\n\n",
    );

    out.push_str("## 1. Op Inventory (Medium)\n\n");
    out.push_str("| # | OpenAPI op | SQL proc | Dev Planning op | Status | Decision |\n");
    out.push_str("|---|------------|----------|-----------------|--------|----------|\n");
    for (i, row) in report.rows.iter().enumerate() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            i + 1,
            render_op_ref(&row.openapi),
            render_proc(&row.sql_proc),
            render_op_ref(&row.dev_planning),
            render_status(row.status),
            truncate(&row.decision, 220),
        ));
    }
    out.push('\n');

    out.push_str("## 2. Per-op Comparison (Medium)\n\n");
    for (i, row) in report.rows.iter().enumerate() {
        out.push_str(&format!("### {}. {}\n\n", i + 1, row.op_label));
        render_medium_block(&mut out, row);
    }

    out.push_str("## 3. Per-op Comparison (Heavy)\n\n");
    for (i, row) in report.rows.iter().enumerate() {
        out.push_str(&format!("### {}. {}\n\n", i + 1, row.op_label));
        render_heavy_block(
            &mut out,
            row,
            &report.literal_index,
            &report.error_code_index,
        );
    }

    if !report.orphan_procs.is_empty() {
        out.push_str("## 4. Orphan SQL Procs\n\n");
        out.push_str(
            "_Procs in `1-ground-truth/` with no matching API op. Forge greps \
             every other proc body for callers; absence of callers raises the \
             `Investigate` flag._\n\n",
        );
        out.push_str("| Proc | Decision |\n|------|----------|\n");
        for row in &report.orphan_procs {
            out.push_str(&format!(
                "| `{}` | {} |\n",
                row.sql_proc.clone().unwrap_or_default(),
                truncate(&row.decision, 220),
            ));
        }
        out.push('\n');
        for row in &report.orphan_procs {
            out.push_str(&format!(
                "### `{}`\n\n",
                row.sql_proc.clone().unwrap_or_default()
            ));
            render_heavy_block(
            &mut out,
            row,
            &report.literal_index,
            &report.error_code_index,
        );
        }
    }

    out
}

fn render_op_ref(r: &Option<OpRef>) -> String {
    match r {
        Some(o) => format!("`{} {}`", o.method, o.path),
        None => "—".into(),
    }
}

fn render_proc(p: &Option<String>) -> String {
    match p {
        Some(name) => format!("`{}`", name),
        None => "⚠ no proc found".into(),
    }
}

fn render_status(s: AlignmentStatus) -> &'static str {
    match s {
        AlignmentStatus::Aligned => "✓ aligned",
        AlignmentStatus::VerbMismatch => "⚠ verb",
        AlignmentStatus::PathMismatch => "⚠ path",
        AlignmentStatus::SqlMissing => "⚠ sql missing",
        AlignmentStatus::OpenApiMissing => "⚠ openapi missing",
        AlignmentStatus::DevPlanningMissing => "⚠ devplanning missing",
        AlignmentStatus::OrphanProc => "○ orphan",
    }
}

fn render_medium_block(out: &mut String, row: &ComparisonRow) {
    out.push_str(&format!("**Status:** {}\n\n", render_status(row.status)));
    out.push_str(&format!("**Decision:** {}\n\n", row.decision));

    if let Some(analysis) = &row.proc_analysis {
        if !analysis.destructive_updates.is_empty() {
            out.push_str("**SQL semantics — destructive UPDATEs (full replacement):**\n\n");
            for upd in &analysis.destructive_updates {
                out.push_str(&format!("- `{}`\n", upd.target_table));
                if !upd.wiped_columns.is_empty() {
                    out.push_str(&format!(
                        "  - Sending the body without these source fields will null these columns: {}\n",
                        upd.wiped_columns
                            .iter()
                            .map(|w| format!("`{}` (from `{}`)", w.column, w.source_variable))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !upd.forced_empty_columns.is_empty() {
                    out.push_str(&format!(
                        "  - Always wiped to empty string regardless of body: {}\n",
                        upd.forced_empty_columns
                            .iter()
                            .map(|c| format!("`{}`", c))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
            out.push('\n');
        }
        if !analysis.conditional_deletes.is_empty() {
            out.push_str("**SQL semantics — conditional DELETEs:**\n\n");
            for cd in &analysis.conditional_deletes {
                out.push_str(&format!(
                    "- ⚠ Sending body without `{}` will DELETE rows from `{}`\n",
                    cd.guarded_by, cd.target_table
                ));
            }
            out.push('\n');
        }
    } else {
        out.push_str("_(no SQL proc matched — nothing to analyse)_\n\n");
    }
}

fn render_heavy_block(
    out: &mut String,
    row: &ComparisonRow,
    index: &LiteralIndex,
    err_index: &ErrorCodeIndex,
) {
    let analysis = match &row.proc_analysis {
        Some(a) => a,
        None => {
            out.push_str("_No SQL proc matched — heavy detail unavailable._\n\n");
            return;
        }
    };

    if !analysis.params.is_empty() {
        out.push_str("**Proc parameters:**\n\n");
        out.push_str("| Name | SQL type | Default |\n|------|----------|---------|\n");
        for p in &analysis.params {
            out.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                p.name,
                p.sql_type,
                p.default
                    .as_deref()
                    .map(|s| format!("`{}`", s))
                    .unwrap_or_else(|| "—".into())
            ));
        }
        out.push('\n');
    }

    if !analysis.json_keys.is_empty() {
        out.push_str("**JSON body keys read by the proc (`OPENJSON`):**\n\n");
        out.push_str("| JSON path | SQL type |\n|-----------|----------|\n");
        for k in &analysis.json_keys {
            out.push_str(&format!("| `{}` | `{}` |\n", k.json_path, k.sql_type));
        }
        out.push('\n');
    }

    if !analysis.error_codes.is_empty() {
        out.push_str("**Error codes raised (`lookups.prc_Program_Response_Const_Lookup`):**\n\n");
        out.push_str("| Code | Description (from `-- [code] - desc` comment) |\n");
        out.push_str("|------|-----------------------------------------------|\n");
        for code in &analysis.error_codes {
            let desc = err_index
                .by_code
                .get(code)
                .cloned()
                .unwrap_or_else(|| "_(no description found in any proc comment — add to lookup seed or annotate inline)_".into());
            out.push_str(&format!("| `{}` | {} |\n", code, desc));
        }
        out.push('\n');
    }

    render_linked_literals(out, analysis, index);

    out.push_str(&format!("_Source: `{}`_\n\n", analysis.source_path));
}

/// Render the "Linked literals" block — show every distinctive value
/// in this proc that ALSO appears in another proc or C# file. Hides
/// values that only appear in this proc (no lineage = not interesting).
fn render_linked_literals(out: &mut String, analysis: &ProcAnalysis, index: &LiteralIndex) {
    if analysis.literals.is_empty() || index.by_value.is_empty() {
        return;
    }
    // Dedupe by value within the proc, keep the first line we saw it.
    let mut seen_local: BTreeMap<String, &crate::proc_analysis::LiteralOccurrence> =
        BTreeMap::new();
    for occ in &analysis.literals {
        let key = occ.value.to_ascii_uppercase();
        seen_local.entry(key).or_insert(occ);
    }

    // Filter to literals that have at least one occurrence outside
    // this proc.
    let mut linked: Vec<(String, &crate::proc_analysis::LiteralOccurrence, Vec<&LiteralLocation>)> =
        Vec::new();
    for (value_key, local_occ) in &seen_local {
        let all = match index.by_value.get(value_key) {
            Some(v) => v,
            None => continue,
        };
        let elsewhere: Vec<&LiteralLocation> = all
            .iter()
            .filter(|loc| !file_matches_proc(&loc.file, &analysis.source_path))
            .collect();
        if !elsewhere.is_empty() {
            linked.push((value_key.clone(), *local_occ, elsewhere));
        }
    }

    if linked.is_empty() {
        return;
    }

    out.push_str("**Linked literals — values defined here that also appear elsewhere in the workspace:**\n\n");
    out.push_str("| Value | Source in this proc | Other places |\n|-------|---------------------|--------------|\n");
    for (value_key, local, elsewhere) in &linked {
        let other_str = elsewhere
            .iter()
            .take(8)
            .map(|loc| format!("`{}` line {}", loc.file, loc.line))
            .collect::<Vec<_>>()
            .join("<br/>");
        let trailer = if elsewhere.len() > 8 {
            format!("<br/>_(+{} more)_", elsewhere.len() - 8)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "| `{}` | line {} — `{}` | {}{} |\n",
            value_key,
            local.line,
            truncate(&local.context, 60),
            other_str,
            trailer
        ));
    }
    out.push('\n');
}

fn file_matches_proc(file_path: &str, proc_source_path: &str) -> bool {
    // The proc's `source_path` may be absolute (passed in from
    // disk-walk) while the literal index records workspace-relative
    // paths. Tail-match on filename is good enough.
    let pf = std::path::Path::new(file_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file_path);
    let pp = std::path::Path::new(proc_source_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(proc_source_path);
    pf.eq_ignore_ascii_case(pp)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let clipped: String = s.chars().take(max).collect();
    format!("{}…", clipped)
}

// ─────────────────────────── per-op summary for story injection ───────────────────────────

/// Produce the SQL Source-of-Truth section markdown to inject into a
/// per-op user-story `.md`. Returns an empty string when no proc was
/// matched (caller should not emit the section header in that case).
pub fn render_story_injection(row: &ComparisonRow) -> String {
    let analysis = match &row.proc_analysis {
        Some(a) => a,
        None => {
            return format!(
                "## SQL Source of Truth\n\n\
                 ⚠ No matching stored procedure found in `1-ground-truth/`.\n\n\
                 **Decision:** {}\n\n",
                row.decision
            );
        }
    };
    let mut out = String::new();
    out.push_str("## SQL Source of Truth\n\n");
    out.push_str(&format!(
        "This op is implemented (or proposed to be implemented) by `{}`.\n\n",
        analysis.full_name
    ));

    if !analysis.destructive_updates.is_empty() {
        out.push_str("**The proc UPDATEs these tables with full-replacement semantics:**\n\n");
        for upd in &analysis.destructive_updates {
            out.push_str(&format!("- `{}`\n", upd.target_table));
        }
        out.push('\n');
    }
    if !analysis.conditional_deletes.is_empty() {
        out.push_str("**Conditional DELETEs — sending the body without the guarded field deletes rows:**\n\n");
        for cd in &analysis.conditional_deletes {
            out.push_str(&format!(
                "- `{}` deleted when `{}` is absent / empty\n",
                cd.target_table, cd.guarded_by
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "**Status vs documentation:** {} \n\n**Decision:** {}\n\n",
        render_status(row.status),
        row.decision
    ));

    out.push_str("→ See [Comparison Report](../comparison/) for full SQL semantics.\n\n");
    out
}

// ─────────────────────────── file emit ───────────────────────────

pub fn emit_report_file(report: &ComparisonReport, out_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let safe_epic: String = report
        .epic
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let path = out_dir.join(format!("{}.md", safe_epic));
    let body = render_report(report);
    std::fs::write(&path, body)?;
    Ok(path)
}

// ─────────────────────────── tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::{Location, OpField, OpSpec};
    use crate::sql_catalog::{SqlCatalog, SqlKind, SqlObject, TableOp, TableOpKind};

    fn op(method: &str, path: &str) -> OpSpec {
        OpSpec {
            slug: format!("{}-{}", method.to_ascii_lowercase(), path.replace('/', "-")),
            label: format!("{} {}", method, path),
            method: Some(method.into()),
            path: Some(path.into()),
            summary: None,
            fields: vec![OpField {
                name: "id".into(),
                logical_type: "string".into(),
                location: Location::Body,
                required: true,
                max_length: None,
                format: Some("uuid".into()),
                description: None,
            }],
            adapter: "openapi".into(),
            source: "test".into(),
        }
    }

    fn proc(schema: &str, name: &str) -> SqlObject {
        SqlObject {
            kind: SqlKind::Procedure,
            schema: Some(schema.into()),
            name: name.into(),
            referenced_tables: Vec::new(),
            ops: vec![TableOp {
                table: format!("{}.x", schema),
                kinds: vec![TableOpKind::Update],
            }],
            source_path: "fake.sql".into(),
        }
    }

    #[test]
    fn aligned_when_openapi_devplanning_proc_all_present_and_match() {
        let cat = SqlCatalog {
            tables: Vec::new(),
            objects: vec![proc("cardholder", "p_txn_Update_Cardholder")],
        };
        let oa = vec![op("PUT", "/cardholder")];
        let dv = vec![op("PUT", "/cardholder")];
        let r = build_comparison(
            "Cardholder Management",
            "cardholder",
            &cat,
            &oa,
            &dv,
            std::path::Path::new("."),
            std::path::Path::new("."),
            None,
        );
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0].status, AlignmentStatus::Aligned);
        assert!(r.rows[0].sql_proc.is_some());
    }

    #[test]
    fn verb_mismatch_when_openapi_put_but_devplanning_patch() {
        let cat = SqlCatalog {
            tables: Vec::new(),
            objects: vec![proc("cardholder", "p_txn_Update_Cardholder")],
        };
        let oa = vec![op("PUT", "/cardholder")];
        let dv = vec![op("PATCH", "/cardholder")];
        let r = build_comparison(
            "Cardholder Management",
            "cardholder",
            &cat,
            &oa,
            &dv,
            std::path::Path::new("."),
            std::path::Path::new("."),
            None,
        );
        assert_eq!(r.rows[0].status, AlignmentStatus::VerbMismatch);
        assert!(r.rows[0].decision.contains("verb"));
    }

    #[test]
    fn sql_missing_when_openapi_proposes_with_no_proc() {
        let cat = SqlCatalog {
            tables: Vec::new(),
            objects: Vec::new(),
        };
        let oa = vec![op("POST", "/cardholder/notes")];
        let dv: Vec<OpSpec> = Vec::new();
        let r = build_comparison(
            "Cardholder Management",
            "cardholder",
            &cat,
            &oa,
            &dv,
            std::path::Path::new("."),
            std::path::Path::new("."),
            None,
        );
        assert_eq!(r.rows[0].status, AlignmentStatus::SqlMissing);
        assert!(r.rows[0].decision.contains("Build"));
    }

    #[test]
    fn orphan_proc_with_no_callers_gets_investigate_decision() {
        let cat = SqlCatalog {
            tables: Vec::new(),
            objects: vec![proc("cardholder", "p_txn_Get_Client_Profile_Data")],
        };
        let oa: Vec<OpSpec> = Vec::new();
        let dv: Vec<OpSpec> = Vec::new();
        let r = build_comparison(
            "Cardholder Management",
            "cardholder",
            &cat,
            &oa,
            &dv,
            std::path::Path::new("."),
            std::path::Path::new("."),
            None,
        );
        assert_eq!(r.orphan_procs.len(), 1);
        assert_eq!(r.orphan_procs[0].status, AlignmentStatus::OrphanProc);
        assert!(r.orphan_procs[0].decision.to_lowercase().contains("investigate"));
    }

    #[test]
    fn render_report_includes_all_four_sections_when_orphan_present() {
        let cat = SqlCatalog {
            tables: Vec::new(),
            objects: vec![
                proc("cardholder", "p_txn_Update_Cardholder"),
                proc("cardholder", "p_txn_Get_Client_Profile_Data"),
            ],
        };
        let oa = vec![op("PUT", "/cardholder")];
        let dv = vec![op("PATCH", "/cardholder")];
        let r = build_comparison(
            "Cardholder Management",
            "cardholder",
            &cat,
            &oa,
            &dv,
            std::path::Path::new("."),
            std::path::Path::new("."),
            None,
        );
        let body = render_report(&r);
        assert!(body.contains("## 1. Op Inventory (Medium)"));
        assert!(body.contains("## 2. Per-op Comparison (Medium)"));
        assert!(body.contains("## 3. Per-op Comparison (Heavy)"));
        assert!(body.contains("## 4. Orphan SQL Procs"));
    }
}
