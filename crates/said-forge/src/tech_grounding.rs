//! Technical grounding — the SQL-aware block forge injects into story
//! prompts and `.forge/<slug>/brain.md`.
//!
//! Given an OpSpec + the workspace's SqlCatalog, we produce a
//! `TechnicalGrounding` with:
//!
//! - **candidate tables** — picked by op-token matching, scored by schema
//!   name match > table name > bare substring. Same heuristic as
//!   schema_diff; reused here via `pick_candidate_tables`.
//! - **related tables via FKs** — one-hop closure so a POST /cardholders
//!   story sees both cardholder.cpf_Client_Profile (primary) AND the
//!   lookup tables it FKs into (lookups.cps_Cardholder_Profile_Status).
//! - **stored procedures that reference the candidate set** — with the
//!   operation kinds they perform (INSERT / UPDATE / SELECT / DELETE).
//! - **views and triggers** on those tables.
//! - **constraints** — PK, UK, NOT NULL columns without defaults, CHECK
//!   constraints summarised (captured at parse time).
//! - **field → column mapping** — the schema_diff output for this op,
//!   inlined so the developer sees it next to the technical grounding.
//!
//! Output is a Markdown block suitable for the LLM prompt (Phase 19.3
//! wires it in) AND for direct inclusion in `brain.md`.

use serde::{Deserialize, Serialize};

use crate::directive::OpSpec;
use crate::mapping_service as ms;
use crate::schema::TableSchema;
use crate::schema_diff::{diff_op, OpDiff, Verdict};
use crate::sql_catalog::{SqlCatalog, SqlKind, TableOpKind};

/// Fully built grounding for one op.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalGrounding {
    pub slug: String,
    pub label: String,
    pub method: Option<String>,
    pub path: Option<String>,
    /// Direct candidate tables (scored against op tokens).
    pub primary_tables: Vec<TableRef>,
    /// Tables reached via FK from any primary_table (one-hop).
    pub related_tables: Vec<TableRef>,
    /// Procedures in catalog that reference any primary or related table.
    pub procedures: Vec<ProcRef>,
    /// Views on the candidate tables.
    pub views: Vec<ObjectRef>,
    /// Triggers on the candidate tables.
    pub triggers: Vec<ObjectRef>,
    /// Field-level diff for the op's request shape (reused from schema_diff).
    pub field_diff: OpDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRef {
    pub full_name: String,
    pub columns_count: usize,
    pub pk: Vec<String>,
    pub not_null_no_default: Vec<String>,
    pub unique_keys: Vec<Vec<String>>,
    pub fks: Vec<ForeignKeyBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyBrief {
    pub column: String,
    pub references: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcRef {
    pub full_name: String,
    /// What the proc does to each referenced table ({ "cpf_Client_Profile": [Insert, Select] }).
    pub table_ops: Vec<(String, Vec<TableOpKind>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectRef {
    pub full_name: String,
    pub referenced_tables: Vec<String>,
}

// ─────────────────────────── public entry ───────────────────────────

pub fn build_technical_grounding(op: &OpSpec, catalog: &SqlCatalog) -> TechnicalGrounding {
    let candidate_refs: Vec<&TableSchema> = pick_candidate_tables(op, &catalog.tables);

    let mut primary: Vec<TableRef> = candidate_refs
        .iter()
        .map(|t| summarise_table(t))
        .collect();

    // Related tables — one-hop FK closure from candidate set.
    let mut related: Vec<TableRef> = Vec::new();
    let mut seen: std::collections::HashSet<String> =
        primary.iter().map(|t| t.full_name.clone()).collect();
    for cand in &candidate_refs {
        for fk in &cand.foreign_keys {
            let target_full = match &fk.ref_schema {
                Some(s) => format!("{}.{}", s, fk.ref_table),
                None => fk.ref_table.clone(),
            };
            if seen.contains(&target_full) {
                continue;
            }
            if let Some(target) = catalog.find_table(&target_full) {
                related.push(summarise_table(target));
                seen.insert(target_full);
            }
        }
    }

    let all_tables: Vec<String> = primary
        .iter()
        .chain(related.iter())
        .map(|t| t.full_name.clone())
        .collect();

    let procedures: Vec<ProcRef> = catalog
        .objects_referencing(&all_tables)
        .into_iter()
        .filter(|o| o.kind == SqlKind::Procedure)
        .map(|o| ProcRef {
            full_name: o.full_name(),
            table_ops: o
                .ops
                .iter()
                .filter(|to| all_tables.iter().any(|t| table_match(t, &to.table)))
                .map(|to| (to.table.clone(), to.kinds.clone()))
                .collect(),
        })
        .collect();

    let views: Vec<ObjectRef> = catalog
        .objects_referencing(&all_tables)
        .into_iter()
        .filter(|o| o.kind == SqlKind::View)
        .map(|o| ObjectRef {
            full_name: o.full_name(),
            referenced_tables: o.referenced_tables.clone(),
        })
        .collect();

    let triggers: Vec<ObjectRef> = catalog
        .objects_referencing(&all_tables)
        .into_iter()
        .filter(|o| o.kind == SqlKind::Trigger)
        .map(|o| ObjectRef {
            full_name: o.full_name(),
            referenced_tables: o.referenced_tables.clone(),
        })
        .collect();

    // Sort for determinism — tests + diffs are easier with stable output.
    primary.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    related.sort_by(|a, b| a.full_name.cmp(&b.full_name));

    let field_diff = diff_op(op, &catalog.tables);

    TechnicalGrounding {
        slug: op.slug.clone(),
        label: op.label.clone(),
        method: op.method.clone(),
        path: op.path.clone(),
        primary_tables: primary,
        related_tables: related,
        procedures,
        views,
        triggers,
        field_diff,
    }
}

fn table_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    a.rsplit('.').next().unwrap_or(a).eq_ignore_ascii_case(b.rsplit('.').next().unwrap_or(b))
}

fn summarise_table(t: &TableSchema) -> TableRef {
    let not_null_no_default: Vec<String> = t
        .columns
        .iter()
        .filter(|c| !c.nullable && !c.has_default && !c.identity && !c.is_computed)
        .map(|c| c.name.clone())
        .collect();
    let fks = t
        .foreign_keys
        .iter()
        .map(|fk| ForeignKeyBrief {
            column: fk.column.clone(),
            references: format!(
                "{}{}.{}",
                fk.ref_schema.as_deref().map(|s| format!("{}.", s)).unwrap_or_default(),
                fk.ref_table,
                fk.ref_column
            ),
        })
        .collect();
    TableRef {
        full_name: t.full_name(),
        columns_count: t.columns.len(),
        pk: t.primary_key.clone(),
        not_null_no_default,
        unique_keys: t.unique_keys.clone(),
        fks,
    }
}

// ─────────────────────────── Candidate picker (delegates to MappingService) ───────────────────────────

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

// ─────────────────────────── Markdown rendering ───────────────────────────

impl TechnicalGrounding {
    /// Render as a Markdown section suitable for embedding in `brain.md` or
    /// the LLM prompt's context pane.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("## Technical grounding — {}\n\n", self.label));

        if self.primary_tables.is_empty() {
            out.push_str(
                "_No candidate SQL tables matched this operation's name tokens. \
                 The field-diff below runs across the whole schema._\n\n",
            );
        } else {
            out.push_str("### Tables touched (inferred from op tokens)\n\n");
            for t in &self.primary_tables {
                out.push_str(&render_table_block(t, "primary"));
            }
            if !self.related_tables.is_empty() {
                out.push_str("### Related tables (via foreign keys)\n\n");
                for t in &self.related_tables {
                    out.push_str(&render_table_block(t, "related"));
                }
            }
        }

        if !self.procedures.is_empty() {
            out.push_str("### Stored procedures\n\n");
            for p in &self.procedures {
                out.push_str(&format!("- `{}`\n", p.full_name));
                for (table, kinds) in &p.table_ops {
                    let ks: Vec<String> = kinds.iter().map(|k| format!("{:?}", k).to_uppercase()).collect();
                    out.push_str(&format!("  - {} → {}\n", table, ks.join(", ")));
                }
            }
            out.push('\n');
        }

        if !self.triggers.is_empty() {
            out.push_str("### Triggers\n\n");
            for t in &self.triggers {
                out.push_str(&format!("- `{}`\n", t.full_name));
            }
            out.push('\n');
        }
        if !self.views.is_empty() {
            out.push_str("### Views\n\n");
            for v in &self.views {
                out.push_str(&format!("- `{}`\n", v.full_name));
            }
            out.push('\n');
        }

        // Field diff table.
        out.push_str("### Field → column mapping\n\n");
        if self.field_diff.fields.is_empty() {
            out.push_str("_(no body/path fields on this op)_\n\n");
        } else {
            out.push_str("| field | api | verdict | SQL column | note |\n");
            out.push_str("|-------|-----|---------|------------|------|\n");
            for f in &self.field_diff.fields {
                let required = if f.required { "req" } else { "opt" };
                let (mark, col, note) = match &f.verdict {
                    Verdict::Matched { table, column, column_type } => (
                        "✓".to_string(),
                        format!("{}.{} ({})", short_name(table), column, column_type),
                        String::new(),
                    ),
                    Verdict::TypeMismatch { table, column, column_type, reason } => (
                        "⚠ type".to_string(),
                        format!("{}.{} ({})", short_name(table), column, column_type),
                        reason.clone(),
                    ),
                    Verdict::LengthMismatch { table, column, column_length, api_max_length } => (
                        "⚠ length".to_string(),
                        format!("{}.{} (len={})", short_name(table), column, column_length),
                        format!("API max={} > SQL={}", api_max_length, column_length),
                    ),
                    Verdict::NullabilityMismatch { table, column, column_nullable, api_required } => (
                        "⚠ nullable".to_string(),
                        format!("{}.{}", short_name(table), column),
                        format!("API required={}, SQL nullable={}", api_required, column_nullable),
                    ),
                    Verdict::Missing => ("✗ missing".to_string(), "—".to_string(), "no column".to_string()),
                    Verdict::Skipped => ("—".to_string(), "_n/a_".to_string(), format!("{:?}", f.location).to_lowercase()),
                };
                out.push_str(&format!(
                    "| `{}` | {} ({}) | {} | {} | {} |\n",
                    f.field_name, f.logical_type, required, mark, col, note
                ));
            }
            out.push('\n');
        }

        out.push_str("### Constraints the implementer must satisfy\n\n");
        let mut had_constraint = false;
        for t in self.primary_tables.iter().chain(self.related_tables.iter()) {
            if t.pk.is_empty() && t.unique_keys.is_empty() && t.not_null_no_default.is_empty() {
                continue;
            }
            had_constraint = true;
            out.push_str(&format!("- **{}**\n", t.full_name));
            if !t.pk.is_empty() {
                out.push_str(&format!("  - PK: {}\n", t.pk.join(", ")));
            }
            if !t.not_null_no_default.is_empty() {
                out.push_str(&format!(
                    "  - NOT NULL (no default): {}\n",
                    t.not_null_no_default.join(", ")
                ));
            }
            for uk in &t.unique_keys {
                out.push_str(&format!("  - UNIQUE: {}\n", uk.join(", ")));
            }
        }
        if !had_constraint {
            out.push_str("_(no constraints captured — no primary tables matched)_\n");
        }
        out.push('\n');
        out
    }
}

fn render_table_block(t: &TableRef, label: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "- **`{}`** · {} cols · {} FKs · role: {}\n",
        t.full_name, t.columns_count, t.fks.len(), label
    ));
    if !t.pk.is_empty() {
        s.push_str(&format!("  - PK: `{}`\n", t.pk.join(", ")));
    }
    if !t.not_null_no_default.is_empty() {
        s.push_str(&format!(
            "  - NOT NULL (no default): {}\n",
            t.not_null_no_default
                .iter()
                .map(|c| format!("`{}`", c))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !t.fks.is_empty() {
        s.push_str("  - FKs:\n");
        for fk in &t.fks {
            s.push_str(&format!("    - `{}` → `{}`\n", fk.column, fk.references));
        }
    }
    s.push('\n');
    s
}

fn short_name(full: &str) -> String {
    full.rsplit_once('.').map(|(_, n)| n.to_string()).unwrap_or_else(|| full.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::{Location, OpField, OpSpec};
    use crate::schema::parse_create_table;

    fn sample_catalog() -> SqlCatalog {
        let table = parse_create_table(
            r#"CREATE TABLE [cardholder].[cpf_Client_Profile] (
                [cpf_Profile_Id]       UNIQUEIDENTIFIER   NOT NULL,
                [cpf_Easy_Profile_Id]  VARCHAR (60)       NOT NULL,
                [cpf_Last_Name]        NVARCHAR (1024)    NULL,
                [ptl_Code]             VARCHAR (10)       NOT NULL,
                CONSTRAINT [PK_cpf] PRIMARY KEY ([cpf_Profile_Id]),
                CONSTRAINT [UK_easy] UNIQUE ([cpf_Easy_Profile_Id]),
                CONSTRAINT [FK_ptl] FOREIGN KEY ([ptl_Code]) REFERENCES [lookups].[ptl_Passport] ([ptl_Code])
            );"#,
        )
        .unwrap();
        let proc = crate::sql_catalog::SqlObject {
            kind: SqlKind::Procedure,
            schema: Some("cardholder".into()),
            name: "p_create_cardholder".into(),
            referenced_tables: vec!["cardholder.cpf_Client_Profile".into()],
            ops: vec![crate::sql_catalog::TableOp {
                table: "cardholder.cpf_Client_Profile".into(),
                kinds: vec![TableOpKind::Insert],
            }],
            source_path: "proc.sql".into(),
        };
        SqlCatalog {
            tables: vec![table],
            objects: vec![proc],
        }
    }

    fn api_op() -> OpSpec {
        OpSpec {
            slug: "post-cardholders".into(),
            label: "POST /cardholders".into(),
            method: Some("POST".into()),
            path: Some("/cardholders".into()),
            summary: Some("Create a cardholder".into()),
            fields: vec![
                OpField {
                    name: "last_name".into(),
                    logical_type: "string".into(),
                    location: Location::Body,
                    required: true,
                    max_length: Some(1024),
                    format: None,
                    description: None,
                },
                OpField {
                    name: "nationality".into(),
                    logical_type: "string".into(),
                    location: Location::Body,
                    required: true,
                    max_length: None,
                    format: None,
                    description: None,
                },
            ],
            adapter: "openapi".into(),
            source: "test".into(),
        }
    }

    #[test]
    fn grounding_includes_primary_table_and_related_via_fk() {
        let cat = sample_catalog();
        let op = api_op();
        let g = build_technical_grounding(&op, &cat);
        assert_eq!(g.primary_tables.len(), 1);
        assert_eq!(g.primary_tables[0].full_name, "cardholder.cpf_Client_Profile");
        // FK to lookups.ptl_Passport — but we don't have that table in the
        // catalog, so it shouldn't appear as related.
        assert!(g.related_tables.is_empty());
    }

    #[test]
    fn grounding_includes_procedure_that_references_table() {
        let cat = sample_catalog();
        let op = api_op();
        let g = build_technical_grounding(&op, &cat);
        assert_eq!(g.procedures.len(), 1);
        assert_eq!(g.procedures[0].full_name, "cardholder.p_create_cardholder");
        let ops = &g.procedures[0].table_ops;
        assert_eq!(ops.len(), 1);
        assert!(ops[0].1.contains(&TableOpKind::Insert));
    }

    #[test]
    fn grounding_reports_not_null_no_default_columns() {
        let cat = sample_catalog();
        let op = api_op();
        let g = build_technical_grounding(&op, &cat);
        let primary = &g.primary_tables[0];
        assert!(primary.not_null_no_default.iter().any(|c| c == "cpf_Easy_Profile_Id"));
        assert!(primary.not_null_no_default.iter().any(|c| c == "ptl_Code"));
        // cpf_Profile_Id is PK + NOT NULL. It should be flagged as NOT NULL
        // too (no default, no identity).
        assert!(primary.not_null_no_default.iter().any(|c| c == "cpf_Profile_Id"));
    }

    #[test]
    fn grounding_renders_markdown_with_all_sections() {
        let cat = sample_catalog();
        let op = api_op();
        let g = build_technical_grounding(&op, &cat);
        let md = g.to_markdown();
        assert!(md.contains("## Technical grounding"));
        assert!(md.contains("### Tables touched"));
        assert!(md.contains("cpf_Client_Profile"));
        assert!(md.contains("### Stored procedures"));
        assert!(md.contains("p_create_cardholder"));
        assert!(md.contains("### Field → column mapping"));
        assert!(md.contains("### Constraints the implementer must satisfy"));
        assert!(md.contains("PK:"));
        assert!(md.contains("UNIQUE"));
    }

    #[test]
    fn grounding_embeds_field_diff_so_missing_column_surfaces() {
        let cat = sample_catalog();
        let op = api_op();
        let g = build_technical_grounding(&op, &cat);
        let md = g.to_markdown();
        // last_name → matches cpf_Last_Name (hungarian strip).
        assert!(md.contains("`last_name`"));
        // nationality → no column → ✗ missing
        assert!(md.contains("`nationality`"));
        assert!(md.contains("missing"));
    }

    #[test]
    fn empty_candidate_set_emits_placeholder_message() {
        let cat = SqlCatalog::default();
        let mut op = api_op();
        op.path = Some("/zzz-unrelated".into());
        op.label = "GET /zzz-unrelated".into();
        let g = build_technical_grounding(&op, &cat);
        let md = g.to_markdown();
        assert!(md.contains("No candidate SQL tables matched"));
    }
}
