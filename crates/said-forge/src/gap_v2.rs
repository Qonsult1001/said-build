//! Schema-aware gap report — the Phase 18 rewrite.
//!
//! Walks the workspace root, parses every `CREATE TABLE` in `1-ground-truth/`
//! into `TableSchema`, parses every directive op (OpenAPI or Markdown) into
//! `OpSpec`, then produces a per-op / per-field diff via `schema_diff`.
//!
//! Output is a single Markdown file (default `.forge/gaps.md`) with a top-
//! level summary, per-op sections, and a per-field table:
//!
//! ```
//! ### POST /cardholders — cpf_Client_Profile
//!
//! | field | api type | verdict | SQL column | note |
//! |-------|----------|---------|------------|------|
//! | first_name | string (req) | ⚠ length | cpf_First_Name NVARCHAR(50) | API allows 200, DB capped at 50 |
//! | nationality | string | ✗ missing | — | No column with this name |
//! ```
//!
//! This is a synchronous walker that opens files directly — it doesn't read
//! from the .said brain, because frame content is pre-ingest-chunked and
//! lossy for structural parsing. The source files are the authoritative
//! input for schema + directive extraction.

use std::path::{Path, PathBuf};

use crate::directive::{extract_ops_from_markdown, extract_ops_from_openapi, OpSpec};
use crate::schema::{parse_create_table, TableSchema};
use crate::schema_diff::{diff_op, OpDiff, Verdict};
use crate::sql_catalog::{build_catalog, SqlCatalog};
use crate::tech_grounding::{build_technical_grounding, TechnicalGrounding};
use crate::{ForgeError, ForgeResult};

// ─────────────────────────── Public entry ───────────────────────────

pub struct GapsV2Input<'a> {
    pub project: &'a str,
    pub workspace_root: &'a Path,
    /// Relative path (from root) to the primary directive file. Markdown or
    /// OpenAPI YAML/JSON — dispatched by extension + content sniff.
    pub primary_directive: &'a Path,
    pub secondary_directive: Option<&'a Path>,
    /// Attach the full TechnicalGrounding (procs, FK graph, constraints)
    /// for every op. Makes the report longer but answers the "which procs
    /// does this op touch" question inline. Default true for
    /// `said forge gaps`.
    pub include_technical_grounding: bool,
}

pub fn generate_gaps_v2(input: GapsV2Input<'_>) -> ForgeResult<GapsReport> {
    let catalog = if input.include_technical_grounding {
        Some(build_catalog(input.workspace_root)?)
    } else {
        None
    };
    let tables: Vec<TableSchema> = match &catalog {
        Some(c) => c.tables.clone(),
        None => collect_tables(input.workspace_root)?,
    };
    let primary_ops = load_ops(input.workspace_root, input.primary_directive)?;
    let secondary_ops = match input.secondary_directive {
        Some(p) => Some(load_ops(input.workspace_root, p)?),
        None => None,
    };

    let op_diffs: Vec<OpDiff> = primary_ops.iter().map(|op| diff_op(op, &tables)).collect();

    let tech_groundings: Vec<Option<TechnicalGrounding>> = match &catalog {
        Some(cat) => primary_ops
            .iter()
            .map(|op| Some(build_technical_grounding(op, cat)))
            .collect(),
        None => primary_ops.iter().map(|_| None).collect(),
    };

    let secondary_slugs: Option<std::collections::HashSet<String>> =
        secondary_ops.as_ref().map(|ops| ops.iter().map(|o| o.slug.clone()).collect());

    Ok(GapsReport {
        project: input.project.into(),
        workspace_root: input.workspace_root.to_path_buf(),
        primary_directive: input.primary_directive.to_path_buf(),
        secondary_directive: input.secondary_directive.map(|p| p.to_path_buf()),
        tables_loaded: tables.iter().map(|t| t.full_name()).collect(),
        op_diffs,
        tech_groundings,
        secondary_slugs,
    })
}

// ─────────────────────────── SQL walking ───────────────────────────

fn collect_tables(root: &Path) -> ForgeResult<Vec<TableSchema>> {
    let ground_truth = root.join("1-ground-truth");
    if !ground_truth.is_dir() {
        return Ok(Vec::new());
    }
    let mut tables = Vec::new();
    let mut stack = vec![ground_truth];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| ForgeError::Io {
            path: dir.display().to_string(),
            cause: e,
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| ForgeError::Io {
                path: dir.display().to_string(),
                cause: e,
            })?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
                != Some("sql".into())
            {
                continue;
            }
            let content = match std::fs::read_to_string(&p) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // A file may contain multiple CREATE TABLE statements separated
            // by GO batches. Split on GO and try parse each batch.
            for batch in split_go_batches(&content) {
                if let Some(t) = parse_create_table(batch) {
                    tables.push(t);
                }
            }
        }
    }
    Ok(tables)
}

fn split_go_batches(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, line) in src.lines().enumerate() {
        if line.trim().eq_ignore_ascii_case("GO") {
            let byte_pos = line_byte_offset(src, i);
            if byte_pos > start {
                out.push(&src[start..byte_pos]);
            }
            start = byte_pos + line.len() + 1;
        }
    }
    if start < src.len() {
        out.push(&src[start..]);
    }
    if out.is_empty() {
        out.push(src);
    }
    out
}

fn line_byte_offset(src: &str, line_idx: usize) -> usize {
    let mut count = 0;
    let mut offset = 0;
    for line in src.lines() {
        if count == line_idx {
            return offset;
        }
        offset += line.len() + 1; // +1 for newline
        count += 1;
    }
    src.len()
}

// ─────────────────────────── Directive loading ───────────────────────────

fn load_ops(root: &Path, rel: &Path) -> ForgeResult<Vec<OpSpec>> {
    let abs = root.join(rel);
    let content = std::fs::read_to_string(&abs).map_err(|e| ForgeError::Io {
        path: abs.display().to_string(),
        cause: e,
    })?;
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => Ok(extract_ops_from_markdown(&content, &abs.display().to_string())),
        "yaml" | "yml" => {
            let json: serde_json::Value =
                serde_yaml::from_str(&content).map_err(|e| ForgeError::Parse {
                    path: abs.display().to_string(),
                    message: format!("yaml: {}", e),
                })?;
            Ok(extract_ops_from_openapi(&json, &abs.display().to_string()))
        }
        "json" => {
            let json: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| ForgeError::Parse {
                    path: abs.display().to_string(),
                    message: format!("json: {}", e),
                })?;
            Ok(extract_ops_from_openapi(&json, &abs.display().to_string()))
        }
        other => Err(ForgeError::Validation(format!(
            "unsupported directive extension .{} (want .md, .yaml, .yml, .json)",
            other
        ))),
    }
}

// ─────────────────────────── Report + rendering ───────────────────────────

pub struct GapsReport {
    pub project: String,
    pub workspace_root: PathBuf,
    pub primary_directive: PathBuf,
    pub secondary_directive: Option<PathBuf>,
    pub tables_loaded: Vec<String>,
    pub op_diffs: Vec<OpDiff>,
    /// Per-op grounding aligned by index with `op_diffs`. Entry is Some(..)
    /// when input.include_technical_grounding was true, None otherwise.
    pub tech_groundings: Vec<Option<TechnicalGrounding>>,
    /// Optional set of slugs from the secondary directive, used for
    /// "missing in secondary / missing in primary" flags.
    pub secondary_slugs: Option<std::collections::HashSet<String>>,
}

impl GapsReport {
    pub fn total_summary(&self) -> TotalSummary {
        let mut s = TotalSummary::default();
        s.ops = self.op_diffs.len();
        for op in &self.op_diffs {
            let fs = op.summary();
            s.field_total += fs.total;
            s.field_matched += fs.matched;
            s.field_type_mismatch += fs.type_mismatch;
            s.field_length_mismatch += fs.length_mismatch;
            s.field_nullability_mismatch += fs.nullability_mismatch;
            s.field_missing += fs.missing;
            s.field_skipped += fs.skipped;
            let body_fields: Vec<&crate::schema_diff::FieldVerdict> = op
                .fields
                .iter()
                .filter(|f| !matches!(f.verdict, Verdict::Skipped))
                .collect();
            if body_fields.is_empty() {
                s.ops_no_body += 1;
            } else if body_fields
                .iter()
                .all(|f| matches!(f.verdict, Verdict::Matched { .. }))
            {
                s.ops_full_support += 1;
            } else if body_fields
                .iter()
                .any(|f| matches!(f.verdict, Verdict::Matched { .. }))
            {
                s.ops_partial_support += 1;
            } else {
                s.ops_no_support += 1;
            }
        }
        s
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let s = self.total_summary();
        out.push_str(&format!("# Schema-Aware Gap Report — {}\n\n", self.project));
        out.push_str(&format!(
            "Workspace: {}\n",
            normalize(&self.workspace_root)
        ));
        out.push_str(&format!(
            "Primary directive: `{}`\n",
            normalize(&self.primary_directive)
        ));
        if let Some(sec) = &self.secondary_directive {
            out.push_str(&format!("Secondary directive: `{}`\n", normalize(sec)));
        }
        out.push_str(&format!(
            "Ground-truth tables parsed: {}\n\n",
            self.tables_loaded.len()
        ));

        out.push_str("## Summary\n\n");
        out.push_str(&format!("- **{}** operations analysed\n", s.ops));
        out.push_str(&format!(
            "  - ✓ {} fully supported (all body/path fields matched SQL columns)\n",
            s.ops_full_support
        ));
        out.push_str(&format!(
            "  - ⚠ {} partial support (some fields matched, some missing or mismatched)\n",
            s.ops_partial_support
        ));
        out.push_str(&format!(
            "  - ✗ {} no support (no body/path fields matched any SQL column)\n",
            s.ops_no_support
        ));
        if s.ops_no_body > 0 {
            out.push_str(&format!(
                "  - — {} ops with no body/path fields (query/header only; not subject to schema diff)\n",
                s.ops_no_body
            ));
        }
        out.push_str(&format!("- **{}** fields examined\n", s.field_total));
        out.push_str(&format!(
            "  - ✓ {} matched | ⚠ {} type | ⚠ {} length | ⚠ {} nullable | ✗ {} missing | — {} skipped\n\n",
            s.field_matched,
            s.field_type_mismatch,
            s.field_length_mismatch,
            s.field_nullability_mismatch,
            s.field_missing,
            s.field_skipped,
        ));

        out.push_str("## Per-operation details\n\n");
        for (idx, op) in self.op_diffs.iter().enumerate() {
            out.push_str(&render_op(op));
            // If we have technical grounding for this op, append it right
            // after the field table so the developer sees the SQL context
            // in one place per operation.
            if let Some(Some(tg)) = self.tech_groundings.get(idx) {
                // The technical-grounding renderer starts with `## Technical
                // grounding — ...`. We're already inside ## Per-operation
                // details, so demote its headings by prepending `#` to each
                // top-level heading the block emits.
                let demoted = demote_h2_to_h4(&tg.to_markdown());
                out.push_str(&demoted);
            }
        }

        if let Some(slugs) = &self.secondary_slugs {
            let primary_slugs: std::collections::HashSet<&str> =
                self.op_diffs.iter().map(|o| o.slug.as_str()).collect();
            let only_in_secondary: Vec<&String> =
                slugs.iter().filter(|s| !primary_slugs.contains(s.as_str())).collect();
            if !only_in_secondary.is_empty() {
                out.push_str("\n## Only in secondary directive\n\n");
                for s in only_in_secondary {
                    out.push_str(&format!("- `{}`\n", s));
                }
                out.push('\n');
            }
        }
        out
    }
}

#[derive(Default)]
pub struct TotalSummary {
    pub ops: usize,
    pub ops_full_support: usize,
    pub ops_partial_support: usize,
    pub ops_no_support: usize,
    pub ops_no_body: usize,
    pub field_total: usize,
    pub field_matched: usize,
    pub field_type_mismatch: usize,
    pub field_length_mismatch: usize,
    pub field_nullability_mismatch: usize,
    pub field_missing: usize,
    pub field_skipped: usize,
}

fn normalize(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s.replace('\\', "/")
    }
}

fn render_op(op: &OpDiff) -> String {
    let mut out = String::new();
    let tables = if op.candidate_tables.is_empty() {
        "_(no candidate tables — op tokens didn't match any SQL table name)_".to_string()
    } else {
        op.candidate_tables
            .iter()
            .map(|t| format!("`{}`", t))
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.push_str(&format!("### `{}` — {}\n\n", op.slug, op.label));
    out.push_str(&format!("Candidate tables: {}\n\n", tables));

    if op.fields.is_empty() {
        out.push_str("_(no fields extracted from this op)_\n\n");
        return out;
    }

    out.push_str("| field | type | required | verdict | SQL column | note |\n");
    out.push_str("|-------|------|----------|---------|------------|------|\n");
    for f in &op.fields {
        let (mark, col_desc, note) = match &f.verdict {
            Verdict::Matched { table, column, column_type } => (
                "✓",
                format!("{}.{} ({})", shortname(table), column, column_type),
                String::new(),
            ),
            Verdict::TypeMismatch { table, column, column_type, reason } => (
                "⚠ type",
                format!("{}.{} ({})", shortname(table), column, column_type),
                reason.clone(),
            ),
            Verdict::LengthMismatch { table, column, column_length, api_max_length } => (
                "⚠ length",
                format!("{}.{} (len={})", shortname(table), column, column_length),
                format!("API maxLength={} > SQL length={}", api_max_length, column_length),
            ),
            Verdict::NullabilityMismatch { table, column, column_nullable, api_required } => (
                "⚠ nullable",
                format!("{}.{}", shortname(table), column),
                format!(
                    "API required={}, SQL nullable={}",
                    api_required, column_nullable
                ),
            ),
            Verdict::Missing => ("✗ missing", "—".to_string(), "no column with a matching name".to_string()),
            Verdict::Skipped => ("—", "_n/a_".to_string(), format!("{:?}", f.location).to_lowercase()),
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            f.field_name,
            f.logical_type,
            if f.required { "yes" } else { "no" },
            mark,
            col_desc,
            note
        ));
    }
    out.push('\n');
    out
}

fn shortname(full: &str) -> String {
    full.rsplit_once('.').map(|(_, n)| n.to_string()).unwrap_or_else(|| full.to_string())
}

/// Shift every heading down by two levels so content nests cleanly under
/// `## Per-operation details`. `## X` → `#### X`, `### Y` → `##### Y`, etc.
fn demote_h2_to_h4(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    for line in md.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            out.push_str("#### ");
            out.push_str(rest);
        } else if let Some(rest) = line.strip_prefix("### ") {
            out.push_str("##### ");
            out.push_str(rest);
        } else if let Some(rest) = line.strip_prefix("#### ") {
            out.push_str("###### ");
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

// ─────────────────────────── Tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_workspace_with_sql_and_md(ddl: &str, md: &str) -> tempfile::TempDir {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let gt = root.join("1-ground-truth/cardholder/Tables");
        std::fs::create_dir_all(&gt).unwrap();
        let mut f = std::fs::File::create(gt.join("cpf.sql")).unwrap();
        f.write_all(ddl.as_bytes()).unwrap();

        let exp = root.join("4-expectations/spec");
        std::fs::create_dir_all(&exp).unwrap();
        let mut f = std::fs::File::create(exp.join("spec.md")).unwrap();
        f.write_all(md.as_bytes()).unwrap();

        tmp
    }

    const DDL: &str = r#"
CREATE TABLE [cardholder].[cpf_Client_Profile] (
    [cpf_Profile_Id]       UNIQUEIDENTIFIER   NOT NULL,
    [cpf_Last_Name]        NVARCHAR (1024)    NULL,
    [cpf_Entry_Id]         INT                IDENTITY (1, 1) NOT NULL,
    CONSTRAINT [PK_cpf] PRIMARY KEY CLUSTERED ([cpf_Profile_Id] ASC)
);
"#;

    const MD: &str = r#"
## POST /cardholders

**Summary**: Creates a cardholder.

### Parameters

#### lastName

- **Location**: body
- **Required**: True
- **Type**: string

#### nationality

- **Location**: body
- **Required**: True
- **Type**: string
"#;

    #[test]
    fn end_to_end_matches_one_field_misses_another() {
        let tmp = make_workspace_with_sql_and_md(DDL, MD);
        let report = generate_gaps_v2(GapsV2Input {
            project: "test",
            workspace_root: tmp.path(),
            primary_directive: Path::new("4-expectations/spec/spec.md"),
            secondary_directive: None,
            include_technical_grounding: false,
        })
        .unwrap();

        assert_eq!(report.op_diffs.len(), 1);
        let op = &report.op_diffs[0];
        assert_eq!(op.slug, "post-cardholders");

        // lastName matches cpf_Last_Name via hungarian-strip + case-normalise.
        let last = op.fields.iter().find(|f| f.field_name == "lastName").unwrap();
        match &last.verdict {
            Verdict::Matched { column, .. } | Verdict::NullabilityMismatch { column, .. } => {
                assert_eq!(column, "cpf_Last_Name");
            }
            other => panic!("expected Matched/NullabilityMismatch for lastName, got {:?}", other),
        }

        // nationality has no column — Missing.
        let nat = op.fields.iter().find(|f| f.field_name == "nationality").unwrap();
        assert_eq!(nat.verdict, Verdict::Missing);
    }

    #[test]
    fn markdown_renders_table_per_op() {
        let tmp = make_workspace_with_sql_and_md(DDL, MD);
        let report = generate_gaps_v2(GapsV2Input {
            project: "t",
            workspace_root: tmp.path(),
            primary_directive: Path::new("4-expectations/spec/spec.md"),
            secondary_directive: None,
            include_technical_grounding: false,
        })
        .unwrap();
        let md = report.to_markdown();
        assert!(md.contains("# Schema-Aware Gap Report"));
        assert!(md.contains("Candidate tables:"));
        assert!(md.contains("| field |"));
        assert!(md.contains("`lastName`"));
        assert!(md.contains("`nationality`"));
    }

    #[test]
    fn include_technical_grounding_adds_block_per_op_in_markdown() {
        let tmp = make_workspace_with_sql_and_md(DDL, MD);
        let report = generate_gaps_v2(GapsV2Input {
            project: "t",
            workspace_root: tmp.path(),
            primary_directive: Path::new("4-expectations/spec/spec.md"),
            secondary_directive: None,
            include_technical_grounding: true,
        })
        .unwrap();
        // Alignment: one grounding entry per op_diff.
        assert_eq!(report.tech_groundings.len(), report.op_diffs.len());
        assert!(report.tech_groundings[0].is_some());
        let md = report.to_markdown();
        // Header is demoted to #### Technical grounding
        assert!(md.contains("#### Technical grounding"));
        // Field diff mentions our fields.
        assert!(md.contains("`lastName`"));
    }

    #[test]
    fn empty_ground_truth_returns_zero_tables() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exp = tmp.path().join("4-expectations");
        std::fs::create_dir_all(&exp).unwrap();
        std::fs::write(exp.join("spec.md"), "## GET /x\n\n### Parameters\n\n#### q\n\n- **Location**: body\n- **Required**: True\n- **Type**: string\n").unwrap();
        let report = generate_gaps_v2(GapsV2Input {
            project: "t",
            workspace_root: tmp.path(),
            primary_directive: Path::new("4-expectations/spec.md"),
            secondary_directive: None,
            include_technical_grounding: false,
        })
        .unwrap();
        assert_eq!(report.tables_loaded.len(), 0);
        // Every field should be Missing because no tables exist.
        let q = &report.op_diffs[0].fields[0];
        assert_eq!(q.verdict, Verdict::Missing);
    }
}
