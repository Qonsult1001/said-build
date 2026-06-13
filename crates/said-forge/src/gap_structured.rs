//! Structured gap output — one folder per group, one file per operation.
//!
//! Mirrors the Dev Planning `spec/<Group>/<METHOD>-<path>-<operationId>.md`
//! convention exactly. Each per-op file is an **extended** version of the
//! Dev Planning spec: the Dev Planning content is shown verbatim for the
//! developer to copy from, then the SQL schema reality, the alignment
//! diff, the OpenAPI disagreement callouts (only when they diverge), and
//! the matched XLSM requirement rows.
//!
//! Dev Planning is primary — shape + content. OpenAPI is the secondary
//! wishlist, surfaced only when it disagrees with Dev Planning. XLSM rows
//! add project status (green/blue/amber, owner, LLR-id).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::directive::{
    extract_ops_from_markdown, extract_ops_from_openapi, Location, OpField, OpSpec,
};
use crate::schema_diff::{diff_op, FieldVerdict, OpDiff, Verdict};
use crate::sql_catalog::{build_catalog, SqlCatalog};
use crate::tech_grounding::{build_technical_grounding, TechnicalGrounding};
use crate::{ForgeError, ForgeResult};

// ─────────────────────────── Public API ───────────────────────────

pub struct StructuredGapsInput<'a> {
    pub project: &'a str,
    pub workspace_root: &'a Path,
    /// Dev Planning base folder, e.g. `4-expectations/Dev Planning`.
    /// The walker looks for <dir>/<Group>/<METHOD-*.md> patterns.
    pub dev_planning_root: &'a Path,
    /// OpenAPI yaml/json file — parsed once, disagreements scored per op.
    pub openapi_path: Option<&'a Path>,
    /// XLSM requirements catalogue already ingested as frames tagged
    /// `forge-xlsx:*`. Pass the workspace `.said` path; the matcher reads
    /// rows from the brain.
    pub xlsx_rows: Vec<XlsxRow>,
    /// Output directory (typically `.forge/gaps/`).
    pub out_dir: &'a Path,
}

#[derive(Debug, Clone)]
pub struct XlsxRow {
    /// The frame doc_id (so we can emit a back-link).
    pub doc_id: String,
    /// Flat column → value map.
    pub cells: BTreeMap<String, String>,
}

impl XlsxRow {
    pub fn req_id(&self) -> Option<&str> {
        for key in ["Req ID", "req id", "Req_ID", "LLR ID", "HLR ID", "ID"] {
            if let Some(v) = self.cells.get(key) {
                if !v.trim().is_empty() {
                    return Some(v.trim());
                }
            }
        }
        None
    }
    pub fn description(&self) -> Option<&str> {
        self.cells
            .get("Description")
            .or_else(|| self.cells.get("description"))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }
    pub fn status(&self) -> Option<&str> {
        self.cells
            .get("Status")
            .or_else(|| self.cells.get("status"))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }
    pub fn color(&self) -> Option<&str> {
        self.cells.get("_color").map(|s| s.as_str())
    }
    pub fn epic(&self) -> Option<&str> {
        self.cells.get("Epic Name").map(|s| s.trim()).filter(|s| !s.is_empty())
    }
}

#[derive(Debug)]
pub struct StructuredGapsResult {
    pub groups_written: usize,
    pub op_files_written: usize,
    pub orphan_openapi_ops: Vec<String>,
    pub total_xlsx_rows: usize,
}

/// Read XLSM rows from a workspace `.said` brain. Walks all frames with
/// the `forge-kind:xlsx-row` tag, parses their JSON body into an XlsxRow.
/// Safe to call on brains without XLSM content — returns an empty vec.
pub fn read_xlsx_rows_from_brain(
    said: &mut sca_core::said_file::SaidFile,
) -> Vec<XlsxRow> {
    let mut rows = Vec::new();
    let metas: Vec<_> = said
        .frames
        .get_all_frames_with_pending()
        .into_iter()
        .filter(|m| m.tags.iter().any(|t| t == "forge-kind:xlsx-row"))
        .map(|m| (m.doc_id.clone(), m.tags.clone()))
        .collect();
    for (doc_id, _tags) in metas {
        let body = match said.get(&doc_id) {
            Some(b) => b,
            None => continue,
        };
        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(obj) = json.as_object() else {
            continue;
        };
        let mut cells = BTreeMap::new();
        for (k, v) in obj {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            };
            cells.insert(k.clone(), s);
        }
        rows.push(XlsxRow { doc_id, cells });
    }
    rows
}

/// Entry point. Returns counts; files are written to disk.
pub fn generate_structured_gaps(input: StructuredGapsInput<'_>) -> ForgeResult<StructuredGapsResult> {
    let catalog = build_catalog(input.workspace_root)?;

    // 1. Walk the Dev Planning tree → { group → Vec<(op_file_path, OpSpec)> }.
    let dev_planning = walk_dev_planning(input.dev_planning_root)?;

    // 2. Parse the OpenAPI (if present) into slug → OpSpec lookup.
    let openapi_ops: HashMap<String, OpSpec> = match input.openapi_path {
        Some(p) => load_openapi_ops(p)?.into_iter().map(|o| (o.slug.clone(), o)).collect(),
        None => HashMap::new(),
    };

    // 3. Index XLSM rows by Req ID + keywords.
    let xlsx_index = XlsxIndex::build(&input.xlsx_rows);

    std::fs::create_dir_all(input.out_dir).map_err(|e| io_err(input.out_dir, e))?;

    let mut groups_written = 0usize;
    let mut op_files_written = 0usize;
    let mut all_op_slugs_dev: std::collections::HashSet<String> = Default::default();

    // Top-level README.
    let mut top_readme = String::new();
    top_readme.push_str(&format!("# Gap Analysis — {}\n\n", input.project));
    top_readme.push_str(&format!("Generated at unix_ts={}\n\n", unix_ts()));
    top_readme.push_str(&format!(
        "Dev Planning groups: **{}**. Total ops: **{}**. OpenAPI ops: **{}**. XLSM rows: **{}**.\n\n",
        dev_planning.len(),
        dev_planning.values().map(|v| v.len()).sum::<usize>(),
        openapi_ops.len(),
        input.xlsx_rows.len(),
    ));
    top_readme.push_str("## Groups\n\n");

    for (group_name, ops) in &dev_planning {
        top_readme.push_str(&format!(
            "- [{}]({}/api.md) — {} operations\n",
            group_name, group_name, ops.len()
        ));

        // Write group folder + per-op files.
        let group_dir = input.out_dir.join(group_name);
        std::fs::create_dir_all(&group_dir).map_err(|e| io_err(&group_dir, e))?;
        groups_written += 1;

        let mut group_api = String::new();
        group_api.push_str(&format!("# {} — Gap Analysis\n\n", group_name));
        group_api.push_str(&format!(
            "{} operations. Each row links to the per-op gap file which mirrors the Dev Planning spec shape, extended with SQL reality and alignment diff.\n\n",
            ops.len()
        ));
        group_api.push_str("| Method | Path | File | SQL tables | Fields |\n");
        group_api.push_str("|--------|------|------|------------|--------|\n");

        for (op_file_name, op) in ops {
            all_op_slugs_dev.insert(op.slug.clone());
            let out_file = group_dir.join(op_file_name);
            let openapi_match = openapi_ops.get(&op.slug);
            let content = render_op_file(
                op,
                openapi_match,
                &catalog,
                &xlsx_index,
                input.dev_planning_root,
                group_name,
                op_file_name,
            )?;
            std::fs::write(&out_file, &content).map_err(|e| io_err(&out_file, e))?;
            op_files_written += 1;
            let tg = build_technical_grounding(op, &catalog);
            group_api.push_str(&format!(
                "| `{}` | `{}` | [{}]({}) | {} | {} |\n",
                op.method.as_deref().unwrap_or("?"),
                op.path.as_deref().unwrap_or("?"),
                op_file_name.trim_end_matches(".md"),
                op_file_name,
                tg.primary_tables.len(),
                op.fields.len(),
            ));
        }

        std::fs::write(group_dir.join("api.md"), group_api)
            .map_err(|e| io_err(&group_dir.join("api.md"), e))?;
    }

    // 4. OpenAPI orphans — ops in the client yaml that Dev Planning doesn't have.
    let orphan_openapi_ops: Vec<String> = openapi_ops
        .keys()
        .filter(|slug| !all_op_slugs_dev.contains(slug.as_str()))
        .cloned()
        .collect();
    if !orphan_openapi_ops.is_empty() {
        top_readme.push_str("\n## OpenAPI-only operations (not in Dev Planning)\n\n");
        top_readme.push_str(&format!("{} operations exist in the client OpenAPI spec but have no Dev Planning counterpart.\n\n", orphan_openapi_ops.len()));
        for slug in &orphan_openapi_ops {
            top_readme.push_str(&format!("- `{}`\n", slug));
        }
    }

    std::fs::write(input.out_dir.join("README.md"), top_readme)
        .map_err(|e| io_err(&input.out_dir.join("README.md"), e))?;

    Ok(StructuredGapsResult {
        groups_written,
        op_files_written,
        orphan_openapi_ops,
        total_xlsx_rows: input.xlsx_rows.len(),
    })
}

// ─────────────────────────── Dev Planning walker ───────────────────────────

pub fn walk_dev_planning(root: &Path) -> ForgeResult<BTreeMap<String, Vec<(String, OpSpec)>>> {
    let mut out: BTreeMap<String, Vec<(String, OpSpec)>> = Default::default();
    if !root.is_dir() {
        return Ok(out);
    }
    for group_entry in std::fs::read_dir(root).map_err(|e| io_err(root, e))? {
        let group_entry = group_entry.map_err(|e| io_err(root, e))?;
        let group_path = group_entry.path();
        if !group_path.is_dir() {
            continue;
        }
        let group_name = match group_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let mut ops = Vec::new();
        for file_entry in std::fs::read_dir(&group_path).map_err(|e| io_err(&group_path, e))? {
            let file_entry = file_entry.map_err(|e| io_err(&group_path, e))?;
            let fp = file_entry.path();
            if !fp.is_file() {
                continue;
            }
            let fname = match fp.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            // Skip the index file — we regenerate our own.
            if fname == "api.md" {
                continue;
            }
            if !fname.to_lowercase().ends_with(".md") {
                continue;
            }
            let content = match std::fs::read_to_string(&fp) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // The file itself is one op — we don't use the multi-op H2 splitter,
            // we use a single-op-per-file parser (each file has `# METHOD /path`).
            if let Some(op) = parse_single_op_md(&content, &fp.display().to_string()) {
                ops.push((fname, op));
            }
        }
        // Sort ops by filename for deterministic output.
        ops.sort_by(|a, b| a.0.cmp(&b.0));
        if !ops.is_empty() {
            out.insert(group_name, ops);
        }
    }
    Ok(out)
}

/// Parse a single-file Dev Planning spec (starts with `# METHOD /path`).
fn parse_single_op_md(text: &str, source: &str) -> Option<OpSpec> {
    // Find the first `# ` heading that looks like `METHOD /path`.
    let (method, path, heading) = text
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            if !t.starts_with("# ") || t.starts_with("## ") {
                return None;
            }
            let body = t.trim_start_matches('#').trim();
            let mut parts = body.splitn(2, char::is_whitespace);
            let m = parts.next()?.to_uppercase();
            let p = parts.next()?;
            if !matches!(
                m.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD"
            ) {
                return None;
            }
            if !p.starts_with('/') {
                return None;
            }
            Some((m, p.to_string(), body.to_string()))
        })
        .next()?;

    let summary = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("**Summary**:").map(|r| r.trim().to_string()));
    let fields = parse_params_section(text);
    let slug = slugify_op(&method, &path);

    Some(OpSpec {
        slug,
        label: heading,
        method: Some(method),
        path: Some(path),
        summary,
        fields,
        adapter: "markdown-dev-planning".into(),
        source: source.to_string(),
    })
}

fn slugify_op(method: &str, path: &str) -> String {
    let p: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let combined = format!("{}-{}", method.to_lowercase(), p);
    let mut out = String::new();
    let mut prev_dash = false;
    for c in combined.chars() {
        if c == '-' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Reuse the Markdown adapter's params-section parser by splitting the file
/// into a single-op chunk.
fn parse_params_section(text: &str) -> Vec<OpField> {
    // Wrap the whole file as one ## heading so extract_ops_from_markdown
    // gives us back one op with the fields. Cheaper than duplicating the
    // parser.
    let pseudo = format!(
        "## synthetic-op\n\n{}",
        text.lines().collect::<Vec<_>>().join("\n")
    );
    let ops = extract_ops_from_markdown(&pseudo, "<inline>");
    ops.into_iter().next().map(|o| o.fields).unwrap_or_default()
}

pub fn load_openapi_ops(path: &Path) -> ForgeResult<Vec<OpSpec>> {
    let s = std::fs::read_to_string(path).map_err(|e| io_err(path, e))?;
    let json: serde_json::Value = if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
    {
        serde_json::from_str(&s).map_err(|e| ForgeError::Parse {
            path: path.display().to_string(),
            message: format!("json: {}", e),
        })?
    } else {
        serde_yaml::from_str(&s).map_err(|e| ForgeError::Parse {
            path: path.display().to_string(),
            message: format!("yaml: {}", e),
        })?
    };
    Ok(extract_ops_from_openapi(&json, &path.display().to_string()))
}

// ─────────────────────────── XLSM index ───────────────────────────

pub struct XlsxIndex {
    by_req_id: HashMap<String, Vec<usize>>,
    rows: Vec<XlsxRow>,
}

impl XlsxIndex {
    pub fn build(rows: &[XlsxRow]) -> Self {
        let mut by_req_id: HashMap<String, Vec<usize>> = Default::default();
        for (i, r) in rows.iter().enumerate() {
            if let Some(id) = r.req_id() {
                by_req_id.entry(id.to_ascii_lowercase()).or_default().push(i);
            }
        }
        Self {
            by_req_id,
            rows: rows.to_vec(),
        }
    }

    /// Match rows to an op. Two modes:
    /// 1. **Strong**: the Dev Planning file itself mentions an LLR-id, or
    ///    the row's Description literally contains the op's path
    ///    (e.g. "POST /cardholders"). These are unambiguous — include all.
    /// 2. **Weak**: path-token substring match (used only when strong
    ///    mode produced zero hits). Capped to top 10 by hit count so a
    ///    common word like "cardholder" doesn't flood the result list.
    pub fn match_op(&self, op: &OpSpec) -> Vec<&XlsxRow> {
        let mut strong: Vec<usize> = Vec::new();
        let mut strong_seen: std::collections::HashSet<usize> = Default::default();

        // Mode 1a — Req ID mentioned in Dev Planning file body/label.
        let haystack = format!(
            "{} {}",
            op.label,
            op.summary.as_deref().unwrap_or("")
        )
        .to_ascii_lowercase();
        for (req_id, idxs) in &self.by_req_id {
            if haystack.contains(req_id) {
                for &i in idxs {
                    if strong_seen.insert(i) {
                        strong.push(i);
                    }
                }
            }
        }

        // Mode 1b — row Description literally quotes the op's path.
        if let Some(path) = &op.path {
            let path_lc = path.to_ascii_lowercase();
            let method_lc = op
                .method
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase();
            for (i, row) in self.rows.iter().enumerate() {
                if strong_seen.contains(&i) {
                    continue;
                }
                if let Some(desc) = row.description() {
                    let lower = desc.to_ascii_lowercase();
                    // Description must contain the exact path, OR method + path segment.
                    if lower.contains(&path_lc)
                        || (!method_lc.is_empty()
                            && lower.contains(&format!("{} {}", method_lc, path_lc)))
                    {
                        if strong_seen.insert(i) {
                            strong.push(i);
                        }
                    }
                }
            }
        }

        if !strong.is_empty() {
            // Dedupe rows with the same Req ID — pick the first occurrence.
            return dedupe_by_req_id(strong.into_iter().map(|i| &self.rows[i]).collect());
        }

        // Mode 2 — weak fallback. Score each row by how many op tokens
        // appear in its Description. Keep the top 10 with ≥2 hits.
        let tokens: Vec<String> = op_path_tokens(op).into_iter().map(|t| stem(&t)).collect();
        if tokens.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(usize, usize)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                row.description().and_then(|desc| {
                    let lower = desc.to_ascii_lowercase();
                    let hits = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
                    if hits >= 2 { Some((i, hits)) } else { None }
                })
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        let indices: Vec<usize> = scored.into_iter().take(10).map(|(i, _)| i).collect();
        dedupe_by_req_id(indices.into_iter().map(|i| &self.rows[i]).collect())
    }
}

fn op_path_tokens(op: &OpSpec) -> Vec<String> {
    let mut s = String::new();
    if let Some(p) = &op.path {
        s.push_str(p);
    }
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() > 3)
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Collapse duplicate rows that share a Req ID (XLSM often has the same
/// LLR-id on multiple rows — one per API endpoint within the same epic).
/// Keep the first occurrence; discard the rest.
fn dedupe_by_req_id<'a>(rows: Vec<&'a XlsxRow>) -> Vec<&'a XlsxRow> {
    let mut seen_ids: std::collections::HashSet<String> = Default::default();
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        match r.req_id() {
            Some(id) => {
                if seen_ids.insert(id.to_ascii_lowercase()) {
                    out.push(r);
                }
            }
            None => out.push(r), // no req_id — keep, can't dedupe
        }
    }
    out
}

/// Light stemming — strip trailing `s` / `ies` so `cardholders` → `cardholder`.
fn stem(s: &str) -> String {
    if s.ends_with("ies") && s.len() > 4 {
        let mut o = s.to_string();
        o.truncate(o.len() - 3);
        o.push('y');
        return o;
    }
    if s.ends_with('s') && !s.ends_with("ss") && s.len() > 4 {
        return s[..s.len() - 1].to_string();
    }
    s.to_string()
}

// ─────────────────────────── Per-op rendering ───────────────────────────

fn render_op_file(
    op: &OpSpec,
    openapi: Option<&OpSpec>,
    catalog: &SqlCatalog,
    xlsx_index: &XlsxIndex,
    dev_planning_root: &Path,
    group_name: &str,
    op_file_name: &str,
) -> ForgeResult<String> {
    let mut out = String::new();
    let label = op.label.trim();
    out.push_str(&format!("# {}\n\n", label));
    if let Some(s) = &op.summary {
        out.push_str(&format!("**Summary**: {}\n\n", s));
    }

    // --- Original Dev Planning content (copied verbatim for developer reference) ---
    out.push_str("## Dev Planning specification (primary wishlist)\n\n");
    out.push_str(&format!(
        "> Source: `{}/{}`\n\n",
        dev_planning_root.file_name().and_then(|s| s.to_str()).unwrap_or("Dev Planning"),
        format!("{}/{}", group_name, op_file_name)
    ));
    // Inline parameter list with required flag.
    if op.fields.is_empty() {
        out.push_str("_(no parameters parsed from the spec — see the source file linked above)_\n\n");
    } else {
        out.push_str("| Field | Location | Required | Type | Format | Max length |\n");
        out.push_str("|-------|----------|----------|------|--------|------------|\n");
        for f in &op.fields {
            out.push_str(&format!(
                "| `{}` | {} | {} | `{}` | {} | {} |\n",
                f.name,
                loc_label(f.location),
                if f.required { "yes" } else { "no" },
                f.logical_type,
                f.format.as_deref().unwrap_or("—"),
                f.max_length.map(|n| n.to_string()).unwrap_or_else(|| "—".into()),
            ));
        }
        out.push('\n');
    }

    // --- SQL reality ---
    let tg = build_technical_grounding(op, catalog);
    out.push_str("## SQL schema reality (ground truth)\n\n");
    if tg.primary_tables.is_empty() && tg.related_tables.is_empty() {
        out.push_str("_(no SQL tables matched the op tokens — the implementer needs to identify target tables manually)_\n\n");
    } else {
        out.push_str(&strip_h2_title(&tg.to_markdown()));
        out.push('\n');
    }

    // --- Alignment diff (Dev Planning × SQL) ---
    let diff = diff_op(op, &catalog.tables);
    out.push_str("## Alignment — Dev Planning field → SQL column\n\n");
    if diff.fields.is_empty() {
        out.push_str("_(no body/path fields to align)_\n\n");
    } else {
        out.push_str("| Field | Dev Planning | SQL column | Verdict | Note |\n");
        out.push_str("|-------|--------------|------------|---------|------|\n");
        for f in &diff.fields {
            let (mark, col, note) = field_verdict_row(f);
            out.push_str(&format!(
                "| `{}` | {} ({}) | {} | {} | {} |\n",
                f.field_name,
                f.logical_type,
                if f.required { "req" } else { "opt" },
                col,
                mark,
                note
            ));
        }
        out.push('\n');
    }

    // --- OpenAPI disagreement callout (only when divergent) ---
    out.push_str("## Client OpenAPI wishlist — disagreements only\n\n");
    match openapi {
        None => {
            out.push_str("_(this operation has no matching entry in the client OpenAPI spec — Dev Planning is the sole wishlist)_\n\n");
        }
        Some(oa) => {
            let diffs = compare_op_field_sets(op, oa);
            if diffs.is_empty() {
                out.push_str("_(OpenAPI agrees with Dev Planning on all field-level attributes compared)_\n\n");
            } else {
                out.push_str("| Field | Dev Planning | OpenAPI | Concern |\n");
                out.push_str("|-------|--------------|---------|---------|\n");
                for d in diffs {
                    out.push_str(&format!(
                        "| `{}` | {} | {} | {} |\n",
                        d.field_name, d.dev_planning_note, d.openapi_note, d.concern
                    ));
                }
                out.push('\n');
            }
        }
    }

    // --- XLSM requirements linkage ---
    out.push_str("## XLSM requirements (project status)\n\n");
    let matches = xlsx_index.match_op(op);
    if matches.is_empty() {
        out.push_str("_(no matching XLSM rows — consider adding a row for this op if it's in scope)_\n\n");
    } else {
        out.push_str("| Req ID | Status | Color | Epic | Description (truncated) |\n");
        out.push_str("|--------|--------|-------|------|-------------------------|\n");
        for r in &matches {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                r.req_id().unwrap_or("—"),
                r.status().unwrap_or("—"),
                r.color().unwrap_or("—"),
                r.epic().unwrap_or("—"),
                truncate_line(r.description().unwrap_or(""), 80),
            ));
        }
        out.push('\n');
    }

    Ok(out)
}

fn field_verdict_row(f: &FieldVerdict) -> (&'static str, String, String) {
    match &f.verdict {
        Verdict::Matched { table, column, column_type } => (
            "✓",
            format!("`{}.{}` ({})", short_table(table), column, column_type),
            String::new(),
        ),
        Verdict::TypeMismatch { table, column, column_type, reason } => (
            "⚠ type",
            format!("`{}.{}` ({})", short_table(table), column, column_type),
            reason.clone(),
        ),
        Verdict::LengthMismatch { table, column, column_length, api_max_length } => (
            "⚠ length",
            format!("`{}.{}` (len={})", short_table(table), column, column_length),
            format!("API max={} > SQL len={}", api_max_length, column_length),
        ),
        Verdict::NullabilityMismatch { table, column, column_nullable, api_required } => (
            "⚠ nullable",
            format!("`{}.{}`", short_table(table), column),
            format!("API required={}, SQL nullable={}", api_required, column_nullable),
        ),
        Verdict::Missing => ("✗ missing", "—".to_string(), "no matching SQL column".to_string()),
        Verdict::Skipped => ("—", "_n/a_".to_string(), format!("{:?}", f.location).to_lowercase()),
    }
}

fn short_table(full: &str) -> String {
    full.rsplit_once('.').map(|(_, n)| n.to_string()).unwrap_or_else(|| full.to_string())
}

fn strip_h2_title(md: &str) -> String {
    // `TechnicalGrounding::to_markdown` starts with `## Technical grounding — <label>`.
    // We already have our own H2 ("SQL schema reality"); demote the block's
    // headings and drop its title line so the content merges cleanly.
    let mut lines = md.lines();
    // Skip the `## Technical grounding` title.
    lines.next();
    // Also skip the first blank line.
    let rest: String = lines.collect::<Vec<_>>().join("\n");
    // Demote `###` → `###` (same — already correct at our level)
    rest
}

fn loc_label(l: Location) -> &'static str {
    match l {
        Location::Body => "body",
        Location::Path => "path",
        Location::Query => "query",
        Location::Header => "header",
        Location::Other => "other",
    }
}

fn truncate_line(s: &str, max: usize) -> String {
    let single: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let normalised: String = single.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalised.chars().count() <= max {
        normalised
    } else {
        normalised.chars().take(max).collect::<String>() + "…"
    }
}

// ─────────────────────────── Dev Planning vs OpenAPI diff ───────────────────────────

struct FieldPairDiff {
    field_name: String,
    dev_planning_note: String,
    openapi_note: String,
    concern: String,
}

fn compare_op_field_sets(dev: &OpSpec, openapi: &OpSpec) -> Vec<FieldPairDiff> {
    let mut out = Vec::new();
    let oa_by_name: HashMap<String, &OpField> =
        openapi.fields.iter().map(|f| (f.name.to_ascii_lowercase(), f)).collect();

    let mut seen: std::collections::HashSet<String> = Default::default();

    for df in &dev.fields {
        let key = df.name.to_ascii_lowercase();
        seen.insert(key.clone());
        let matching = oa_by_name.get(&key);
        match matching {
            None => {
                out.push(FieldPairDiff {
                    field_name: df.name.clone(),
                    dev_planning_note: format!(
                        "{} ({})",
                        df.logical_type,
                        if df.required { "required" } else { "optional" }
                    ),
                    openapi_note: "—".into(),
                    concern: "OpenAPI does not define this field".into(),
                });
            }
            Some(of) => {
                let mut concerns = Vec::new();
                if df.required != of.required {
                    concerns.push(format!(
                        "required differs (dev={}, openapi={})",
                        df.required, of.required
                    ));
                }
                if !logical_types_agree(&df.logical_type, &of.logical_type) {
                    concerns.push(format!(
                        "type differs (dev={}, openapi={})",
                        df.logical_type, of.logical_type
                    ));
                }
                match (df.max_length, of.max_length) {
                    (Some(a), Some(b)) if a != b => {
                        concerns.push(format!("maxLength differs (dev={}, openapi={})", a, b));
                    }
                    _ => {}
                }
                if !concerns.is_empty() {
                    out.push(FieldPairDiff {
                        field_name: df.name.clone(),
                        dev_planning_note: format!(
                            "{} ({})",
                            df.logical_type,
                            if df.required { "required" } else { "optional" }
                        ),
                        openapi_note: format!(
                            "{} ({})",
                            of.logical_type,
                            if of.required { "required" } else { "optional" }
                        ),
                        concern: concerns.join("; "),
                    });
                }
            }
        }
    }

    // Fields in OpenAPI that Dev Planning doesn't have.
    for of in &openapi.fields {
        if !seen.contains(&of.name.to_ascii_lowercase()) {
            out.push(FieldPairDiff {
                field_name: of.name.clone(),
                dev_planning_note: "—".into(),
                openapi_note: format!(
                    "{} ({})",
                    of.logical_type,
                    if of.required { "required" } else { "optional" }
                ),
                concern: "Dev Planning does not define this field — verify if it's in scope".into(),
            });
        }
    }
    out
}

fn logical_types_agree(a: &str, b: &str) -> bool {
    let al = a.to_ascii_lowercase();
    let bl = b.to_ascii_lowercase();
    if al == bl {
        return true;
    }
    // Treat `integer` and `number` as agreeable; OpenAPI sometimes uses
    // `number` where Dev Planning says `integer`.
    let int_like = |s: &str| s == "integer" || s == "number";
    if int_like(&al) && int_like(&bl) {
        return true;
    }
    false
}

// ─────────────────────────── Helpers ───────────────────────────

fn io_err(path: &Path, cause: std::io::Error) -> ForgeError {
    ForgeError::Io {
        path: path.display().to_string(),
        cause,
    }
}

fn unix_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(root: &Path, rel: &str, content: &str) -> PathBuf {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        p
    }

    const SAMPLE_DEV_SPEC: &str = r#"
# POST /cardholders

**Summary**: Creates a new cardholder with the provided information.

### Parameters

#### firstName

- **Location**: body
- **Required**: True
- **Type**: string
- **MaxLength**: 50

#### nationality

- **Location**: body
- **Required**: True
- **Type**: string
"#;

    const SAMPLE_SQL: &str = r#"
CREATE TABLE [cardholder].[cpf_Client_Profile] (
    [cpf_Profile_Id]  UNIQUEIDENTIFIER NOT NULL,
    [cpf_Last_Name]   NVARCHAR (100)   NULL,
    CONSTRAINT [PK_cpf] PRIMARY KEY ([cpf_Profile_Id])
);
"#;

    #[test]
    fn walker_finds_group_and_parses_op() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("dev-planning");
        write_file(&root, "Cardholders/POST-cardholders-createCardholder.md", SAMPLE_DEV_SPEC);
        let groups = walk_dev_planning(&root).unwrap();
        assert!(groups.contains_key("Cardholders"));
        let ops = &groups["Cardholders"];
        assert_eq!(ops.len(), 1);
        let (_fname, op) = &ops[0];
        assert_eq!(op.method.as_deref(), Some("POST"));
        assert_eq!(op.path.as_deref(), Some("/cardholders"));
        assert_eq!(op.slug, "post-cardholders");
        assert_eq!(op.fields.len(), 2);
    }

    #[test]
    fn xlsx_index_matches_by_req_id_mentioned_in_summary() {
        let rows = vec![
            XlsxRow {
                doc_id: "row-a".into(),
                cells: [
                    ("Req ID".to_string(), "LLR-042".to_string()),
                    ("Description".to_string(), "Create cardholder".to_string()),
                    ("Status".to_string(), "Signed-Off".to_string()),
                ]
                .into_iter()
                .collect(),
            },
        ];
        let idx = XlsxIndex::build(&rows);
        // Op whose summary mentions LLR-042
        let op = OpSpec {
            slug: "x".into(),
            label: "POST /cardholders".into(),
            method: Some("POST".into()),
            path: Some("/cardholders".into()),
            summary: Some("Implements LLR-042 per Dev Planning".into()),
            fields: Vec::new(),
            adapter: "test".into(),
            source: "t".into(),
        };
        let matched = idx.match_op(&op);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].doc_id, "row-a");
    }

    #[test]
    fn xlsx_index_matches_by_path_token_keywords() {
        let rows = vec![XlsxRow {
            doc_id: "row-b".into(),
            cells: [
                ("Req ID".to_string(), "LLR-099".to_string()),
                (
                    "Description".to_string(),
                    // Contains both stemmed tokens: "cardholder" + "createcardholder"
                    "cardholder createcardholder records and sets initial status".to_string(),
                ),
            ]
            .into_iter()
            .collect(),
        }];
        let idx = XlsxIndex::build(&rows);
        let op = OpSpec {
            slug: "post-cardholders-createcardholder".into(),
            label: "POST /cardholders/createCardholder".into(),
            method: Some("POST".into()),
            path: Some("/cardholders/createCardholder".into()),
            summary: None,
            fields: Vec::new(),
            adapter: "test".into(),
            source: "t".into(),
        };
        let matched = idx.match_op(&op);
        assert_eq!(matched.len(), 1);
    }

    #[test]
    fn xlsx_strong_match_by_path_in_description_wins_over_weak() {
        let rows = vec![
            // Strong match: description mentions the exact path.
            XlsxRow {
                doc_id: "strong".into(),
                cells: [
                    ("Req ID".to_string(), "LLR-100".to_string()),
                    (
                        "Description".to_string(),
                        "POST /cardholders endpoint creates a new cardholder".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            },
            // Weak match: description contains "cardholder" but no path.
            XlsxRow {
                doc_id: "weak".into(),
                cells: [
                    ("Req ID".to_string(), "LLR-101".to_string()),
                    (
                        "Description".to_string(),
                        "cardholder cardholder relationship description".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            },
        ];
        let idx = XlsxIndex::build(&rows);
        let op = OpSpec {
            slug: "post-cardholders".into(),
            label: "POST /cardholders".into(),
            method: Some("POST".into()),
            path: Some("/cardholders".into()),
            summary: None,
            fields: Vec::new(),
            adapter: "test".into(),
            source: "t".into(),
        };
        let matched = idx.match_op(&op);
        // Strong match found — weak row should be excluded.
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].doc_id, "strong");
    }

    #[test]
    fn compare_op_field_sets_flags_required_divergence() {
        let mk_field = |name: &str, req: bool| OpField {
            name: name.into(),
            logical_type: "string".into(),
            location: Location::Body,
            required: req,
            max_length: None,
            format: None,
            description: None,
        };
        let dev = OpSpec {
            slug: "x".into(),
            label: "POST /x".into(),
            method: Some("POST".into()),
            path: Some("/x".into()),
            summary: None,
            fields: vec![mk_field("firstName", true)],
            adapter: "dev".into(),
            source: "t".into(),
        };
        let oa = OpSpec {
            fields: vec![mk_field("firstName", false)],
            ..dev.clone()
        };
        let diffs = compare_op_field_sets(&dev, &oa);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].concern.contains("required differs"));
    }

    #[test]
    fn end_to_end_writes_group_folder_with_per_op_file_and_indices() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Workspace scaffold: one SQL table.
        write_file(root, "1-ground-truth/Tables/cpf.sql", SAMPLE_SQL);

        // Dev Planning tree: one group, one op file.
        let dp = root.join("dev-planning");
        write_file(
            &dp,
            "Cardholders/POST-cardholders-createCardholder.md",
            SAMPLE_DEV_SPEC,
        );

        // Empty OpenAPI + empty XLSM — focus on the structural write path.
        let out = root.join("gaps");
        let input = StructuredGapsInput {
            project: "t",
            workspace_root: root,
            dev_planning_root: &dp,
            openapi_path: None,
            xlsx_rows: Vec::new(),
            out_dir: &out,
        };
        let result = generate_structured_gaps(input).unwrap();
        assert_eq!(result.groups_written, 1);
        assert_eq!(result.op_files_written, 1);
        // README + group/api.md + per-op file all exist.
        assert!(out.join("README.md").is_file());
        assert!(out.join("Cardholders/api.md").is_file());
        assert!(out
            .join("Cardholders/POST-cardholders-createCardholder.md")
            .is_file());

        // Op file content includes all five sections.
        let content = std::fs::read_to_string(
            out.join("Cardholders/POST-cardholders-createCardholder.md"),
        )
        .unwrap();
        assert!(content.contains("# POST /cardholders"));
        assert!(content.contains("## Dev Planning specification"));
        assert!(content.contains("## SQL schema reality"));
        assert!(content.contains("## Alignment"));
        assert!(content.contains("## Client OpenAPI wishlist"));
        assert!(content.contains("## XLSM requirements"));
    }
}
