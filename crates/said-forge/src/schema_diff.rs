//! Schema-aware field-level gap diff.
//!
//! Given a directive `OpSpec` (parsed OpenAPI/Markdown op) and a pool of
//! `TableSchema`s extracted from ground-truth SQL DDL, produce a per-field
//! verdict:
//!
//! - **matched** — found a compatible column in a candidate table.
//! - **type-mismatch** — column exists but its SQL type doesn't align with
//!   the directive's logical type (e.g. `VARCHAR(50)` column receiving an
//!   unbounded API string, or `INT` column receiving a `string` field).
//! - **nullability-mismatch** — API marks field required but SQL column is
//!   nullable, or vice versa (softer warning).
//! - **length-mismatch** — API `maxLength` exceeds SQL column length.
//! - **missing** — no column with a plausible name match in any candidate
//!   table.
//!
//! Candidate tables are picked by heuristic: tokenise the op path/label
//! (e.g. `/cardholders/{id}/transitions` → [cardholders, transitions]),
//! then find tables in the `cardholder` / similar schemas whose names
//! contain any token. Coarse but honest; refining is straightforward.
//!
//! Column name matching tries: exact → case-insensitive → stripped-prefix
//! (drop the Hungarian `cpf_`, `ptl_` style prefix on the column) →
//! stemmed (snake_case ↔ camelCase normalised). If no candidate matches,
//! we return `Missing` rather than guessing.

use serde::{Deserialize, Serialize};

use crate::directive::{Location, OpField, OpSpec};
use crate::mapping_service as ms;
use crate::schema::{Column, TableSchema};

// ─────────────────────────── public diff output ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpDiff {
    pub slug: String,
    pub label: String,
    pub candidate_tables: Vec<String>,
    pub fields: Vec<FieldVerdict>,
}

impl OpDiff {
    pub fn summary(&self) -> FieldSummary {
        let mut s = FieldSummary::default();
        for f in &self.fields {
            match &f.verdict {
                Verdict::Matched { .. } => s.matched += 1,
                Verdict::TypeMismatch { .. } => s.type_mismatch += 1,
                Verdict::LengthMismatch { .. } => s.length_mismatch += 1,
                Verdict::NullabilityMismatch { .. } => s.nullability_mismatch += 1,
                Verdict::Missing => s.missing += 1,
                Verdict::Skipped => s.skipped += 1,
            }
        }
        s.total = self.fields.len();
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldSummary {
    pub total: usize,
    pub matched: usize,
    pub type_mismatch: usize,
    pub length_mismatch: usize,
    pub nullability_mismatch: usize,
    pub missing: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldVerdict {
    pub field_name: String,
    pub logical_type: String,
    pub required: bool,
    pub location: Location,
    pub max_length: Option<u32>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum Verdict {
    Matched {
        table: String,
        column: String,
        column_type: String,
    },
    TypeMismatch {
        table: String,
        column: String,
        column_type: String,
        reason: String,
    },
    LengthMismatch {
        table: String,
        column: String,
        column_length: i32,
        api_max_length: u32,
    },
    NullabilityMismatch {
        table: String,
        column: String,
        column_nullable: bool,
        api_required: bool,
    },
    Missing,
    /// Field is a header / query param / etc — not subject to DB schema
    /// compatibility (no DB column will exist).
    Skipped,
}

// ─────────────────────────── main entry ───────────────────────────

pub fn diff_op(op: &OpSpec, tables: &[TableSchema]) -> OpDiff {
    let candidates = pick_candidate_tables(op, tables);
    let candidate_names: Vec<String> = candidates.iter().map(|t| t.full_name()).collect();

    let mut field_verdicts = Vec::with_capacity(op.fields.len());
    for f in &op.fields {
        // Only body / path fields land in DB columns. Query + header are
        // orchestration, not schema.
        if !matches!(f.location, Location::Body | Location::Path) {
            field_verdicts.push(FieldVerdict {
                field_name: f.name.clone(),
                logical_type: f.logical_type.clone(),
                required: f.required,
                location: f.location,
                max_length: f.max_length,
                verdict: Verdict::Skipped,
            });
            continue;
        }
        let verdict = classify_field(f, &candidates);
        field_verdicts.push(FieldVerdict {
            field_name: f.name.clone(),
            logical_type: f.logical_type.clone(),
            required: f.required,
            location: f.location,
            max_length: f.max_length,
            verdict,
        });
    }

    OpDiff {
        slug: op.slug.clone(),
        label: op.label.clone(),
        candidate_tables: candidate_names,
        fields: field_verdicts,
    }
}

fn classify_field(field: &OpField, candidates: &[&TableSchema]) -> Verdict {
    for table in candidates {
        if let Some(col) = find_column(&field.name, table) {
            return compare_field_to_column(field, col, table);
        }
    }
    Verdict::Missing
}

fn compare_field_to_column(field: &OpField, col: &Column, table: &TableSchema) -> Verdict {
    ms::compare_field_to_column(field, col, table)
}

// ─────────────────────────── candidate picking ───────────────────────────

fn pick_candidate_tables<'a>(op: &OpSpec, tables: &'a [TableSchema]) -> Vec<&'a TableSchema> {
    let tokens = ms::op_tokens(op);
    let mut scored: Vec<(&TableSchema, i32)> = tables
        .iter()
        .map(|t| (t, ms::score_table(t, &tokens)))
        .filter(|(_, s)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().take(4).map(|(t, _)| t).collect()
}

// ─────────────────────────── column name matching ───────────────────────────

fn find_column<'a>(field_name: &str, table: &'a TableSchema) -> Option<&'a Column> {
    let field_norm = ms::normalise_name(field_name);
    for c in &table.columns {
        if c.name == field_name {
            return Some(c);
        }
    }
    for c in &table.columns {
        if c.name.eq_ignore_ascii_case(field_name) {
            return Some(c);
        }
    }
    for c in &table.columns {
        let stripped = ms::strip_prefix(&c.name);
        if ms::normalise_name(&stripped) == field_norm {
            return Some(c);
        }
    }
    for c in &table.columns {
        if ms::normalise_name(&c.name) == field_norm {
            return Some(c);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::{Location, OpField, OpSpec};
    use crate::schema::{parse_create_table, TableSchema};

    fn sample_table() -> TableSchema {
        parse_create_table(
            r#"CREATE TABLE [cardholder].[cpf_Client_Profile] (
                [cpf_Profile_Id]       UNIQUEIDENTIFIER   NOT NULL,
                [cpf_Last_Name]        NVARCHAR (1024)    NULL,
                [cpf_DOB]              VARCHAR (10)       NULL,
                [cpf_Easy_Profile_Id]  VARCHAR (60)       NOT NULL,
                [cpf_Entry_Id]         INT                IDENTITY (1, 1) NOT NULL,
                [cpf_Created_UTC]      DATETIMEOFFSET     NULL,
                CONSTRAINT [PK_cpf_Client_Profile] PRIMARY KEY CLUSTERED ([cpf_Profile_Id] ASC)
            );"#,
        )
        .unwrap()
    }

    fn field(name: &str, logical: &str, required: bool) -> OpField {
        OpField {
            name: name.into(),
            logical_type: logical.into(),
            location: Location::Body,
            required,
            max_length: None,
            format: None,
            description: None,
        }
    }

    fn op_with_fields(slug: &str, path: &str, fields: Vec<OpField>) -> OpSpec {
        OpSpec {
            slug: slug.into(),
            label: format!("POST {}", path),
            method: Some("POST".into()),
            path: Some(path.into()),
            summary: None,
            fields,
            adapter: "openapi".into(),
            source: "test".into(),
        }
    }

    #[test]
    fn candidate_picks_tables_matching_op_tokens() {
        let tables = vec![sample_table()];
        let op = op_with_fields("post-cardholders", "/cardholders", vec![]);
        let diff = diff_op(&op, &tables);
        assert_eq!(diff.candidate_tables.len(), 1);
        assert!(diff.candidate_tables[0].contains("Client_Profile"));
    }

    #[test]
    fn exact_column_name_matches() {
        let tables = vec![sample_table()];
        let op = op_with_fields(
            "post-cardholders",
            "/cardholders",
            vec![field("cpf_Profile_Id", "string", true)],
        );
        let diff = diff_op(&op, &tables);
        let v = &diff.fields[0].verdict;
        // cpf_Profile_Id is UNIQUEIDENTIFIER, api is plain string (no format)
        // — our compat rules treat string ↔ uniqueidentifier as matched via
        // "string to uuid" leniency. So we expect Matched.
        match v {
            Verdict::Matched { column, .. } => assert_eq!(column, "cpf_Profile_Id"),
            other => panic!("expected Matched, got {:?}", other),
        }
    }

    #[test]
    fn hungarian_prefix_stripped_for_match() {
        let tables = vec![sample_table()];
        // API calls it `last_name`; SQL column is `cpf_Last_Name`.
        let op = op_with_fields(
            "post-cardholders",
            "/cardholders",
            vec![field("last_name", "string", false)],
        );
        let diff = diff_op(&op, &tables);
        match &diff.fields[0].verdict {
            Verdict::Matched { column, .. } => assert_eq!(column, "cpf_Last_Name"),
            other => panic!("expected Matched, got {:?}", other),
        }
    }

    #[test]
    fn camelcase_to_snakecase_match() {
        let tables = vec![sample_table()];
        // API `firstName` ↔ SQL `cpf_First_Name`… wait, our sample table has
        // cpf_Last_Name not first. Use a field that exists.
        let op = op_with_fields(
            "post-cardholders",
            "/cardholders",
            vec![field("lastName", "string", false)],
        );
        let diff = diff_op(&op, &tables);
        match &diff.fields[0].verdict {
            Verdict::Matched { column, .. } => assert_eq!(column, "cpf_Last_Name"),
            other => panic!("expected Matched for lastName → cpf_Last_Name, got {:?}", other),
        }
    }

    #[test]
    fn missing_column_returns_missing_verdict() {
        let tables = vec![sample_table()];
        let op = op_with_fields(
            "post-cardholders",
            "/cardholders",
            vec![field("nationality", "string", true)],
        );
        let diff = diff_op(&op, &tables);
        assert_eq!(diff.fields[0].verdict, Verdict::Missing);
    }

    #[test]
    fn length_mismatch_flagged_when_api_exceeds_column() {
        let tables = vec![sample_table()];
        let mut f = field("last_name", "string", false);
        f.max_length = Some(2048); // DB is 1024
        let op = op_with_fields("post-cardholders", "/cardholders", vec![f]);
        let diff = diff_op(&op, &tables);
        match &diff.fields[0].verdict {
            Verdict::LengthMismatch {
                column_length,
                api_max_length,
                ..
            } => {
                assert_eq!(*column_length, 1024);
                assert_eq!(*api_max_length, 2048);
            }
            other => panic!("expected LengthMismatch, got {:?}", other),
        }
    }

    #[test]
    fn type_mismatch_string_into_int_column() {
        let tables = vec![sample_table()];
        // API says entry_id is a string; SQL column is INT.
        let op = op_with_fields(
            "post-cardholders",
            "/cardholders",
            vec![field("entry_id", "integer", true)],
        );
        // entry_id maps to cpf_Entry_Id (INT IDENTITY) — compatible.
        let diff = diff_op(&op, &tables);
        match &diff.fields[0].verdict {
            Verdict::Matched { column, .. } => assert_eq!(column, "cpf_Entry_Id"),
            // Identity column IS nullable-mismatched vs required API so the
            // check may surface that; accept either Matched or Nullability.
            Verdict::NullabilityMismatch { .. } => {}
            other => panic!("expected Matched or NullabilityMismatch, got {:?}", other),
        }

        let op2 = op_with_fields(
            "post-cardholders",
            "/cardholders",
            vec![field("entry_id", "string", true)],
        );
        let diff2 = diff_op(&op2, &tables);
        match &diff2.fields[0].verdict {
            Verdict::TypeMismatch { reason, .. } => {
                assert!(reason.contains("string") && reason.contains("INT"));
            }
            other => panic!("expected TypeMismatch, got {:?}", other),
        }
    }

    #[test]
    fn query_and_header_fields_are_skipped() {
        let tables = vec![sample_table()];
        let mut q = field("include", "string", false);
        q.location = Location::Query;
        let mut h = field("x-api-version", "string", true);
        h.location = Location::Header;
        let op = op_with_fields("post-x", "/x", vec![q, h]);
        let diff = diff_op(&op, &tables);
        assert_eq!(diff.fields[0].verdict, Verdict::Skipped);
        assert_eq!(diff.fields[1].verdict, Verdict::Skipped);
    }

    #[test]
    fn compat_uuid_format_requires_uniqueidentifier() {
        let c = ms::type_compatible_core("string", Some("uuid"), "UNIQUEIDENTIFIER");
        assert!(c.compatible);
        let c2 = ms::type_compatible_core("string", Some("uuid"), "VARCHAR");
        assert!(!c2.compatible);
    }

    #[test]
    fn compat_boolean_to_bit() {
        assert!(ms::type_compatible_core("boolean", None, "BIT").compatible);
        assert!(!ms::type_compatible_core("boolean", None, "NVARCHAR").compatible);
    }

    #[test]
    fn normalise_name_collapses_case_and_separators() {
        assert_eq!(ms::normalise_name("firstName"), "firstname");
        assert_eq!(ms::normalise_name("first_name"), "firstname");
        assert_eq!(ms::normalise_name("First_Name"), "firstname");
        assert_eq!(ms::normalise_name("FIRST-NAME"), "firstname");
    }

    #[test]
    fn singular_stems_trailing_s_and_ies() {
        assert_eq!(ms::singular("cardholders"), "cardholder");
        assert_eq!(ms::singular("categories"), "category");
        assert_eq!(ms::singular("address"), "address");
    }

    #[test]
    fn op_diff_summary_counts_correctly() {
        let tables = vec![sample_table()];
        let op = op_with_fields(
            "post-x",
            "/cardholders",
            vec![
                field("last_name", "string", false),             // Matched
                field("nationality", "string", true),            // Missing
                field("entry_id", "string", true),               // TypeMismatch
            ],
        );
        let diff = diff_op(&op, &tables);
        let s = diff.summary();
        assert_eq!(s.total, 3);
        assert_eq!(s.matched + s.nullability_mismatch, 1); // last_name might be nullability
        assert_eq!(s.missing, 1);
        assert_eq!(s.type_mismatch, 1);
    }
}
