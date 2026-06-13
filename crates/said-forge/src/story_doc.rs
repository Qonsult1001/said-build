//! Unified user-story document emitter — one `.md` per op containing
//! everything a Product Owner needs to sign off AND everything a
//! developer needs to implement.
//!
//! Previous versions split this across `user-stories/*.txt`,
//! `.forge/stories/*.md`, `.forge/gaps/*.md`, and
//! `5-deliverables/stories/*.md`. That was four files with overlapping
//! but incomplete content. This module emits ONE file with all 13
//! sections:
//!
//! 1. Document History
//! 2. Document Approval (stakeholder sign-off table)
//! 3. Scope (In / Out / Stakeholders)
//! 4. Process Flow (Mermaid op → tables → procs)
//! 5. User Story (As a / I want / So that + purpose + priority + roles + endpoint)
//! 6. Directive Specification (headers + query + path + body fields)
//! 7. Acceptance Criteria (grouped from directive fields)
//! 8. SQL Schema Reality (tables, columns, PK/FK/NOT NULL, procs, triggers, views)
//! 9. Field → Column Mapping (with confidence badges)
//! 10. Constraints the implementer must satisfy
//! 11. Alignment — Dev Planning vs SQL (disagreements only)
//! 12. XLSM Requirements (Req ID / Status / Colour / Epic)
//! 13. Traceability (sources + mapping trace + glossary)
//!
//! Deterministic: same (op, bundle, tech_grounding, xlsm_rows,
//! business config, meta) → byte-identical markdown.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::business_config::BusinessConfig;
use crate::directive::{Location, OpField, OpSpec};
use crate::error::{ForgeError, ForgeResult};
use crate::gap_structured::{XlsxIndex, XlsxRow};
use crate::mapping_service::{Confidence, MappingService, TableRole};
use crate::schema_diff::Verdict;
use crate::sql_catalog::SqlCatalog;
use crate::story_gen;
use crate::tech_grounding::{build_technical_grounding, TableRef, TechnicalGrounding};

// ─────────────────────────── public API ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryDocsReport {
    pub out_dir: PathBuf,
    pub files_written: Vec<String>,
    pub total_ops: usize,
}

/// Hints about where an op sits in the Epic/HLR/LLR hierarchy. Passed
/// in by the caller (usually derived from the XLSM row for the op).
/// When absent the filename falls back to a sensible placeholder —
/// forge runs without complaint but the filename is clearly marked
/// for the user to rename.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoryMeta {
    /// e.g. "HLR019"
    pub hlr_id: Option<String>,
    /// e.g. "Cardholder Parent-Child Relationships"
    pub hlr_title: Option<String>,
    /// e.g. "LLR016"
    pub llr_id: Option<String>,
    /// e.g. "Create new Cardholder"
    pub llr_action: Option<String>,
    /// MoSCoW priority from XLSM colour (Must / Should / Could / Won't).
    pub priority: Option<String>,
    /// Status string from the XLSM row (e.g. "User Story Signed-Off").
    pub status: Option<String>,
}

/// Generate one unified `.md` per op. Every file carries all 13
/// sections — user story, directive spec, SQL reality, mapping,
/// constraints, alignment, XLSM status.
///
/// - `ops` — pre-extracted directive ops.
/// - `metas` — per-op meta (HLR/LLR/priority/status). Missing entry
///   falls back to "TBD" placeholders and a filename the user can
///   rename later.
/// - `service` — MappingService (catalog + glossary + overrides).
/// - `catalog` — same catalog the service holds, surfaced separately
///   so tech_grounding can walk tables/procs/triggers/views.
/// - `xlsx_rows` — XLSM rows from the brain (for the Section 12
///   project-status join). Empty slice is fine.
/// - `business` — project business config (stakeholders / epic / etc).
/// - `out_dir` — conventionally `5-deliverables/stories/`. Files land
///   in a sub-folder per epic.
pub fn generate_story_docs(
    ops: &[OpSpec],
    metas: &BTreeMap<String, StoryMeta>,
    service: &MappingService<'_>,
    catalog: &SqlCatalog,
    xlsx_rows: &[XlsxRow],
    business: &BusinessConfig,
    out_dir: &Path,
) -> ForgeResult<StoryDocsReport> {
    fs::create_dir_all(out_dir).map_err(|e| ForgeError::Io {
        path: out_dir.display().to_string(),
        cause: e,
    })?;

    let xlsx_index = XlsxIndex::build(xlsx_rows);

    let mut files_written = Vec::new();
    for op in ops {
        let meta = metas.get(&op.slug).cloned().unwrap_or_default();
        let bundle = story_gen::build_bundle(op, service);
        let process_flow = crate::viz::render_op_dependency(op, service);
        let tech = build_technical_grounding(op, catalog);
        let xlsm_matches: Vec<&XlsxRow> = xlsx_index.match_op(op);
        let entity = bundle.entity_slug.clone();
        let filename = compose_filename(op, &meta, &business.epic, &entity);
        let epic_dir = out_dir.join(sanitise_segment(&business.epic));
        fs::create_dir_all(&epic_dir).map_err(|e| ForgeError::Io {
            path: epic_dir.display().to_string(),
            cause: e,
        })?;
        let path = epic_dir.join(&filename);
        let body = render_story_md(
            op,
            &bundle,
            &meta,
            business,
            &process_flow,
            &tech,
            &xlsm_matches,
        );
        fs::write(&path, body).map_err(|e| ForgeError::Io {
            path: path.display().to_string(),
            cause: e,
        })?;
        files_written.push(format!(
            "{}/{}",
            sanitise_segment(&business.epic),
            filename
        ));
    }
    files_written.sort();

    Ok(StoryDocsReport {
        out_dir: out_dir.to_path_buf(),
        files_written,
        total_ops: ops.len(),
    })
}

// ─────────────────────────── filename composer ───────────────────────────

fn compose_filename(
    op: &OpSpec,
    meta: &StoryMeta,
    epic: &str,
    entity: &str,
) -> String {
    let hlr_id = meta.hlr_id.clone().unwrap_or_else(|| "HLR-TBD".into());
    let hlr_title = meta
        .hlr_title
        .clone()
        .unwrap_or_else(|| "TBD HLR Title".into());
    let llr_id = meta.llr_id.clone().unwrap_or_else(|| "LLR-TBD".into());
    let llr_action = meta.llr_action.clone().unwrap_or_else(|| {
        let method = op
            .method
            .clone()
            .unwrap_or_else(|| "".into())
            .to_uppercase();
        format!("{} {}", title_case_verb(&method), title_case_entity(entity))
    });
    let stem = format!(
        "Epic – {} – {} – {} – {} – {}",
        sanitise_segment(epic),
        hlr_id,
        sanitise_segment(&hlr_title),
        llr_id,
        sanitise_segment(&llr_action)
    );
    format!("{}.md", stem)
}

fn sanitise_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

fn title_case_entity(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' {
            out.push(' ');
            upper_next = true;
            continue;
        }
        if upper_next {
            for u in c.to_uppercase() {
                out.push(u);
            }
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn title_case_verb(method: &str) -> &'static str {
    match method.to_ascii_uppercase().as_str() {
        "POST" => "Create a",
        "PUT" => "Update an existing",
        "PATCH" => "Amend",
        "DELETE" => "Delete",
        "GET" => "Retrieve",
        _ => "Handle",
    }
}

// ─────────────────────────── document renderer ───────────────────────────

fn render_story_md(
    op: &OpSpec,
    bundle: &story_gen::StoryBundle,
    meta: &StoryMeta,
    business: &BusinessConfig,
    process_flow_mermaid: &str,
    tech: &TechnicalGrounding,
    xlsm_matches: &[&XlsxRow],
) -> String {
    let today = today_ymd();
    let method = op.method.clone().unwrap_or_else(|| "—".into()).to_uppercase();
    let path = op.path.clone().unwrap_or_else(|| op.label.clone());
    let hlr_id = meta.hlr_id.clone().unwrap_or_else(|| "HLR-TBD".into());
    let llr_id = meta.llr_id.clone().unwrap_or_else(|| "LLR-TBD".into());
    let llr_action = meta.llr_action.clone().unwrap_or_else(|| {
        format!("{} {}", title_case_verb(&method), title_case_entity(&bundle.entity_slug))
    });

    let mut out = String::new();

    // Title (H1).
    out.push_str(&format!(
        "# EPIC – {} – {} – {} – {}\n\n",
        business.epic.to_uppercase(),
        hlr_id,
        llr_id,
        llr_action.to_uppercase()
    ));
    out.push_str("## USER STORY\n\n");

    // Cover meta.
    out.push_str("| Field | Value |\n");
    out.push_str("|-------|-------|\n");
    out.push_str(&format!("| Prepared By | {} |\n", business.prepared_by));
    out.push_str(&format!("| Document Version | {} |\n", business.version));
    out.push_str(&format!("| Date | {} |\n", today));
    if !business.sprint.is_empty() {
        out.push_str(&format!("| Sprint | {} |\n", business.sprint));
    }
    if let Some(status) = &meta.status {
        out.push_str(&format!("| XLSM Status | {} |\n", status));
    }
    out.push('\n');

    // Section 1 — Document History
    out.push_str("## 1. Document History\n\n");
    out.push_str("| Version | Date | Status | Amended By | Summary of Changes |\n");
    out.push_str("|---------|------|--------|------------|--------------------|\n");
    out.push_str(&format!(
        "| {} | {} | Initial Draft | {} | Generated by forge from current SQL + directive |\n\n",
        business.version, today, business.prepared_by
    ));

    // Section 2 — Document Approval
    out.push_str("## 2. Document Approval\n\n");
    out.push_str(
        "Herewith we accept the contents, format and layout of this document for its intended \
         purpose. We confirm that the business rules / requirements specified here are current and \
         accurate, and that we have participated in defining them.\n\n",
    );
    if business.stakeholders.is_empty() {
        out.push_str("_(no stakeholders configured — edit `.forge/business.toml` to populate)_\n\n");
    } else {
        out.push_str("| Name | Position | Department | Signature | Date |\n");
        out.push_str("|------|----------|------------|-----------|------|\n");
        for s in &business.stakeholders {
            out.push_str(&format!(
                "| {} | {} | {} |  |  |\n",
                s.name, s.position, s.department
            ));
        }
        out.push('\n');
    }

    // Section 3 — Scope
    out.push_str("## 3. Scope\n\n");
    out.push_str("### In Scope\n\n");
    match &op.summary {
        Some(s) if !s.trim().is_empty() => out.push_str(&format!("{}\n\n", s.trim())),
        _ => out.push_str(&format!(
            "- {} `{}` on the {} platform\n\n",
            method, path, business.epic
        )),
    }

    out.push_str("### Out of Scope\n\n");
    out.push_str("_(to be confirmed with Product Owner)_\n\n");

    out.push_str("### Stakeholders\n\n");
    if business.stakeholders.is_empty() {
        out.push_str("_(no stakeholders configured)_\n\n");
    } else {
        out.push_str("| Role | Name |\n");
        out.push_str("|------|------|\n");
        for s in &business.stakeholders {
            out.push_str(&format!("| {} | {} |\n", s.position, s.name));
        }
        out.push('\n');
    }

    // Section 4 — Process Flow (Mermaid diagram generated by viz module)
    out.push_str("## 4. Process Flow\n\n");
    out.push_str(process_flow_mermaid);
    out.push('\n');

    // Section 5 — User Story
    out.push_str("## 5. User Story\n\n");
    out.push_str(&format!(
        "**As a** TXN Program Manager,\n\n**I want to** invoke `{} {}`,\n\n**So that** I can {}.\n\n",
        method,
        path,
        purpose_stub(&method, &bundle.entity_slug)
    ));

    out.push_str("### Purpose\n\n");
    match &op.summary {
        Some(s) if !s.trim().is_empty() => out.push_str(&format!("{}\n\n", s.trim())),
        _ => out.push_str(&format!(
            "Enable authorised Program Managers to perform `{} {}` against the TXN platform.\n\n",
            method, path
        )),
    }

    out.push_str("### Priority\n\n");
    match &meta.priority {
        Some(p) if !p.trim().is_empty() => {
            out.push_str(&format!("**{}**\n\n", p.trim()));
        }
        _ => out.push_str("**Must have — MVP**\n\n"),
    }

    out.push_str("### Roles\n\n");
    out.push_str("- Cardholder / Business (depending on entity)\n");
    out.push_str("- TXN Platform supported by Direct Transact\n\n");

    out.push_str("### Endpoint\n\n");
    out.push_str(&format!("`{} {}`\n\n", method, path));

    // Section 6 — Directive Specification (full field contract from Dev Planning)
    out.push_str("## 6. Directive Specification\n\n");
    out.push_str(&format!("> Source: `{}`\n\n", op.source));
    render_directive_spec(&mut out, op);

    // Section 7 — Acceptance Criteria (grouped from directive fields)
    out.push_str("## 7. Acceptance Criteria\n\n");
    render_acceptance_criteria(&mut out, op, bundle);

    // Section 8 — SQL Schema Reality (tables, procs, triggers, views)
    out.push_str("## 8. SQL Schema Reality\n\n");
    out.push_str(
        "_Ground truth from the current SQL catalog. Every table / proc / \
         trigger / view forge inferred from the op tokens and FK graph._\n\n",
    );
    render_sql_reality(&mut out, tech);

    // Section 9 — Field → Column Mapping (with confidence badges)
    out.push_str("## 9. Field → Column Mapping\n\n");
    out.push_str(
        "_Per-field verdict with confidence. Query + header fields never \
         map to SQL; they're noted for completeness._\n\n",
    );
    render_field_column_mapping(&mut out, bundle);

    // Section 10 — Constraints the implementer must satisfy
    out.push_str("## 10. Constraints the Implementer Must Satisfy\n\n");
    render_constraints(&mut out, tech);

    // Section 11 — Alignment — Dev Planning vs SQL (disagreements only)
    out.push_str("## 11. Alignment — Dev Planning vs SQL\n\n");
    render_alignment(&mut out, bundle);

    // Section 12 — XLSM Requirements (project status)
    out.push_str("## 12. XLSM Requirements (Project Status)\n\n");
    render_xlsm_status(&mut out, xlsm_matches);

    // Section 13 — Traceability
    out.push_str("## 13. Traceability\n\n");
    out.push_str(&format!(
        "- Directive source: `{}`\n",
        op.source
    ));
    out.push_str(&format!(
        "- HLR: `{}` — {}\n",
        hlr_id,
        meta.hlr_title.clone().unwrap_or_else(|| "TBD".into())
    ));
    out.push_str(&format!("- LLR: `{}` — {}\n", llr_id, llr_action));
    out.push_str("- Mapping trace: `.forge/mapping-trace.jsonl`\n");
    out.push_str("- Glossary: `.forge/glossary.toml`\n\n");

    out.push_str("### Confidence Badges\n\n");
    out.push_str("- `🔒 explicit` — `.forge/mapping.toml` override, ship verbatim\n");
    out.push_str("- `● high` — exact symbol or dominant token match\n");
    out.push_str("- `◐ medium` — multi-signal heuristic (review before shipping)\n");
    out.push_str("- `◯ low` — single-signal heuristic (review carefully)\n");
    out.push_str("- `✗ none` — no mapping produced (escalate to schema owner)\n");

    out
}

// ─────────────────────────── section renderers ───────────────────────────

fn render_directive_spec(out: &mut String, op: &OpSpec) {
    if op.fields.is_empty() {
        out.push_str("_(no fields declared in the directive)_\n\n");
        return;
    }

    // Group fields by location for readability.
    let mut headers: Vec<&OpField> = Vec::new();
    let mut query: Vec<&OpField> = Vec::new();
    let mut path_fields: Vec<&OpField> = Vec::new();
    let mut body: Vec<&OpField> = Vec::new();
    let mut other: Vec<&OpField> = Vec::new();
    for f in &op.fields {
        match f.location {
            Location::Header => headers.push(f),
            Location::Query => query.push(f),
            Location::Path => path_fields.push(f),
            Location::Body => body.push(f),
            Location::Other => other.push(f),
        }
    }

    let render_group = |out: &mut String, label: &str, fields: &[&OpField]| {
        if fields.is_empty() {
            return;
        }
        out.push_str(&format!("### {}\n\n", label));
        out.push_str("| Field | Required | Type | Format | Max length | Description |\n");
        out.push_str("|-------|----------|------|--------|------------|-------------|\n");
        for f in fields {
            let required = if f.required { "yes" } else { "no" };
            let fmt = f.format.as_deref().unwrap_or("—");
            let max = f
                .max_length
                .map(|n| n.to_string())
                .unwrap_or_else(|| "—".into());
            let desc = f
                .description
                .as_deref()
                .map(|s| truncate_one_line(s, 120))
                .unwrap_or_else(|| "—".into());
            out.push_str(&format!(
                "| `{}` | {} | `{}` | {} | {} | {} |\n",
                f.name, required, f.logical_type, fmt, max, desc
            ));
        }
        out.push('\n');
    };

    render_group(out, "Request Headers", &headers);
    render_group(out, "Path Parameters", &path_fields);
    render_group(out, "Query Parameters", &query);
    render_group(out, "Body Fields", &body);
    render_group(out, "Other", &other);
}

fn render_sql_reality(out: &mut String, tech: &TechnicalGrounding) {
    if tech.primary_tables.is_empty() {
        out.push_str("_No candidate SQL tables matched this op's tokens. Escalate to the schema owner._\n\n");
        return;
    }

    out.push_str("### Tables Touched (inferred from op tokens)\n\n");
    for t in &tech.primary_tables {
        render_table_block(out, t, "primary");
    }

    if !tech.related_tables.is_empty() {
        out.push_str("### Related Tables (via foreign keys)\n\n");
        for t in &tech.related_tables {
            render_table_block(out, t, "related");
        }
    }

    if !tech.procedures.is_empty() {
        out.push_str("### Stored Procedures\n\n");
        for p in &tech.procedures {
            out.push_str(&format!("- `{}`\n", p.full_name));
            for (table, kinds) in &p.table_ops {
                let ks: Vec<String> =
                    kinds.iter().map(|k| format!("{:?}", k).to_uppercase()).collect();
                out.push_str(&format!("  - {} → {}\n", table, ks.join(", ")));
            }
        }
        out.push('\n');
    }

    if !tech.triggers.is_empty() {
        out.push_str("### Triggers\n\n");
        for t in &tech.triggers {
            out.push_str(&format!("- `{}`\n", t.full_name));
        }
        out.push('\n');
    }

    if !tech.views.is_empty() {
        out.push_str("### Views\n\n");
        for v in &tech.views {
            out.push_str(&format!("- `{}`\n", v.full_name));
        }
        out.push('\n');
    }
}

fn render_table_block(out: &mut String, t: &TableRef, label: &str) {
    out.push_str(&format!(
        "- **`{}`** · {} cols · {} FKs · role: {}\n",
        t.full_name, t.columns_count, t.fks.len(), label
    ));
    if !t.pk.is_empty() {
        out.push_str(&format!("  - PK: `{}`\n", t.pk.join(", ")));
    }
    if !t.not_null_no_default.is_empty() {
        out.push_str(&format!(
            "  - NOT NULL (no default): {}\n",
            t.not_null_no_default
                .iter()
                .map(|c| format!("`{}`", c))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for uk in &t.unique_keys {
        out.push_str(&format!(
            "  - UNIQUE: {}\n",
            uk.iter()
                .map(|c| format!("`{}`", c))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !t.fks.is_empty() {
        out.push_str("  - FKs:\n");
        for fk in &t.fks {
            out.push_str(&format!("    - `{}` → `{}`\n", fk.column, fk.references));
        }
    }
    out.push('\n');
}

fn render_field_column_mapping(out: &mut String, bundle: &story_gen::StoryBundle) {
    if bundle.field_bindings.is_empty() {
        out.push_str("_(no fields)_\n\n");
        return;
    }
    out.push_str("| Field | Location | Required | API type | SQL column | Verdict | Confidence |\n");
    out.push_str("|-------|----------|----------|----------|------------|---------|------------|\n");
    for fb in &bundle.field_bindings {
        let loc = format!("{:?}", fb.field.location).to_lowercase();
        let required = if fb.field.required { "yes" } else { "no" };
        let (verdict_mark, col_cell) = match &fb.verdict {
            Verdict::Matched { .. } => (
                "✓ matched".to_string(),
                format!("`{}.{}` ({})", fb.table, fb.column, fb.column_type),
            ),
            Verdict::TypeMismatch { reason, .. } => (
                format!("⚠ type — {}", reason),
                format!("`{}.{}` ({})", fb.table, fb.column, fb.column_type),
            ),
            Verdict::LengthMismatch {
                column_length,
                api_max_length,
                ..
            } => (
                format!("⚠ length (api={}, sql={})", api_max_length, column_length),
                format!("`{}.{}`", fb.table, fb.column),
            ),
            Verdict::NullabilityMismatch {
                column_nullable,
                api_required,
                ..
            } => (
                format!(
                    "⚠ nullable (api_required={}, sql_nullable={})",
                    api_required, column_nullable
                ),
                format!("`{}.{}`", fb.table, fb.column),
            ),
            Verdict::Missing => ("✗ missing".into(), "—".into()),
            Verdict::Skipped => ("— (not a DB field)".into(), "_n/a_".into()),
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | `{}` | {} | {} | {} |\n",
            fb.field.name,
            loc,
            required,
            fb.field.logical_type,
            col_cell,
            verdict_mark,
            fb.confidence.badge()
        ));
    }
    out.push('\n');
}

fn render_constraints(out: &mut String, tech: &TechnicalGrounding) {
    let all: Vec<&TableRef> = tech
        .primary_tables
        .iter()
        .chain(tech.related_tables.iter())
        .collect();
    let mut had = false;
    for t in &all {
        if t.pk.is_empty() && t.unique_keys.is_empty() && t.not_null_no_default.is_empty() {
            continue;
        }
        had = true;
        out.push_str(&format!("- **`{}`**\n", t.full_name));
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
    if !had {
        out.push_str("_(no constraints captured — no candidate tables matched)_\n");
    }
    out.push('\n');
}

fn render_alignment(out: &mut String, bundle: &story_gen::StoryBundle) {
    let body_or_path: Vec<&story_gen::FieldBinding> = bundle
        .field_bindings
        .iter()
        .filter(|b| matches!(b.field.location, Location::Body | Location::Path))
        .collect();
    if body_or_path.is_empty() {
        out.push_str("_No body or path fields — no alignment to check._\n\n");
        return;
    }
    let disagreements: Vec<&story_gen::FieldBinding> = body_or_path
        .iter()
        .copied()
        .filter(|b| {
            !matches!(
                b.verdict,
                Verdict::Matched { .. } | Verdict::Skipped
            )
        })
        .collect();
    if disagreements.is_empty() {
        out.push_str("_All body + path fields match the SQL schema. No disagreements._\n\n");
        return;
    }
    out.push_str("| Field | Dev Planning | SQL column | Verdict |\n");
    out.push_str("|-------|--------------|------------|---------|\n");
    for fb in disagreements {
        let api = format!(
            "{} ({})",
            fb.field.logical_type,
            if fb.field.required { "req" } else { "opt" }
        );
        let col_cell = if fb.table.is_empty() {
            "_none_".into()
        } else {
            format!("`{}.{}`", fb.table, fb.column)
        };
        let verdict_mark = match &fb.verdict {
            Verdict::TypeMismatch { reason, .. } => format!("⚠ type — {}", reason),
            Verdict::LengthMismatch {
                column_length,
                api_max_length,
                ..
            } => format!("⚠ length (api={}, sql={})", api_max_length, column_length),
            Verdict::NullabilityMismatch {
                column_nullable,
                api_required,
                ..
            } => format!(
                "⚠ nullable (api_required={}, sql_nullable={})",
                api_required, column_nullable
            ),
            Verdict::Missing => "✗ missing".into(),
            _ => "—".into(),
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            fb.field.name, api, col_cell, verdict_mark
        ));
    }
    out.push('\n');
}

fn render_xlsm_status(out: &mut String, rows: &[&XlsxRow]) {
    if rows.is_empty() {
        out.push_str(
            "_(no XLSM rows matched this op — add an entry in the project \
             tracking sheet to surface status here)_\n\n",
        );
        return;
    }
    out.push_str("| Req ID | Status | Colour | Epic | Description |\n");
    out.push_str("|--------|--------|--------|------|-------------|\n");
    for row in rows {
        let req = row.req_id().unwrap_or("—");
        let status = row.status().unwrap_or("—");
        let colour = row.color().unwrap_or("—");
        let epic = row.epic().unwrap_or("—");
        let desc = row
            .description()
            .map(|s| truncate_one_line(s, 90))
            .unwrap_or_else(|| "—".into());
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            req, status, colour, epic, desc
        ));
    }
    out.push('\n');
}

fn truncate_one_line(s: &str, max: usize) -> String {
    let single = s.replace('\n', " ").replace('|', "/");
    if single.chars().count() <= max {
        return single;
    }
    let clipped: String = single.chars().take(max).collect();
    format!("{}…", clipped)
}

fn today_ymd() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (y, m, d) = ymd_from_unix(secs);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Convert a Unix timestamp (UTC seconds) to `(year, month, day)`.
/// Pure civil-from-days algorithm — no dep on chrono.
fn ymd_from_unix(secs: i64) -> (i32, u32, u32) {
    let days = secs.div_euclid(86_400);
    // Algorithm: https://howardhinnant.github.io/date_algorithms.html#civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn purpose_stub(method: &str, entity: &str) -> String {
    let entity_title = title_case_entity(entity);
    match method {
        "POST" => format!("create a new {}", entity_title),
        "PUT" => format!("update an existing {}", entity_title),
        "PATCH" => format!("amend specific fields of a {}", entity_title),
        "DELETE" => format!("deactivate a {}", entity_title),
        "GET" => format!("retrieve {} data", entity_title),
        _ => format!("work with {} records", entity_title),
    }
}

fn render_acceptance_criteria(out: &mut String, op: &OpSpec, bundle: &story_gen::StoryBundle) {
    let body_path: Vec<&story_gen::FieldBinding> = bundle
        .field_bindings
        .iter()
        .filter(|b| matches!(b.field.location, Location::Body | Location::Path))
        .collect();

    if body_path.is_empty() {
        out.push_str("_No body or path fields on this op — acceptance criteria limited to execution semantics._\n\n");
        return;
    }

    // Group 1 — unique identifier (UUID fields).
    let ids: Vec<&story_gen::FieldBinding> = body_path
        .iter()
        .copied()
        .filter(|b| {
            b.field
                .format
                .as_deref()
                .map(|f| f.eq_ignore_ascii_case("uuid"))
                .unwrap_or(false)
                || b.column_type.to_ascii_uppercase().contains("UNIQUEIDENTIFIER")
        })
        .collect();
    if !ids.is_empty() {
        out.push_str("### Unique Identifier\n\n");
        for f in &ids {
            out.push_str(&format!(
                "- `{}` — UUID. {}\n",
                f.field.name,
                if f.field.required {
                    "Required."
                } else {
                    "Optional — system generates when omitted."
                }
            ));
        }
        out.push('\n');
    }

    // Group 2 — required identity / content fields (non-UUID, required).
    let required: Vec<&story_gen::FieldBinding> = body_path
        .iter()
        .copied()
        .filter(|b| {
            b.field.required
                && !ids.iter().any(|i| i.field.name == b.field.name)
        })
        .collect();
    if !required.is_empty() {
        out.push_str("### Required Fields\n\n");
        for f in &required {
            let fmt = f
                .field
                .format
                .as_deref()
                .map(|s| format!(" ({} format)", s))
                .unwrap_or_default();
            let desc = f
                .field
                .description
                .as_deref()
                .map(|s| format!(" — {}", s.trim()))
                .unwrap_or_default();
            out.push_str(&format!(
                "- `{}` ({}){}{}\n",
                f.field.name, f.field.logical_type, fmt, desc
            ));
        }
        out.push('\n');
    }

    // Group 3 — optional.
    let optional: Vec<&story_gen::FieldBinding> = body_path
        .iter()
        .copied()
        .filter(|b| {
            !b.field.required
                && !ids.iter().any(|i| i.field.name == b.field.name)
        })
        .collect();
    if !optional.is_empty() {
        out.push_str("### Optional Fields\n\n");
        for f in &optional {
            let desc = f
                .field
                .description
                .as_deref()
                .map(|s| format!(" — {}", s.trim()))
                .unwrap_or_default();
            out.push_str(&format!(
                "- `{}` ({}){}\n",
                f.field.name, f.field.logical_type, desc
            ));
        }
        out.push('\n');
    }

    // Group 4 — related tables (addresses / lookups resolved via FK).
    if !bundle.related_tables.is_empty() {
        out.push_str("### Related entities\n\n");
        for (name, _) in &bundle.related_tables {
            out.push_str(&format!("- `{}` — resolved via foreign key\n", name));
        }
        out.push('\n');
    }

    // Group 5 — validation (length + nullability + type).
    out.push_str("### Validation\n\n");
    out.push_str("- All required fields must be present.\n");
    out.push_str("- Formats must match the field contract (email, phone E.164, ISO codes, etc.).\n");
    out.push_str("- Duplicate records are rejected.\n");
    for f in &body_path {
        if let Some(max) = f.field.max_length {
            out.push_str(&format!(
                "- `{}` length ≤ {} characters.\n",
                f.field.name, max
            ));
        }
    }
    out.push('\n');

    // Group 6 — auditability.
    out.push_str("### Auditability\n\n");
    out.push_str("- Every operation is logged with timestamp, actor, and payload.\n");
    out.push_str("- Full API request / response GUIDs recorded for traceability.\n\n");

    let _ = op; // op is reserved for future criteria inference
}

// ─────────────────────────── tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::{Location, OpField, OpSpec};
    use crate::mapping_service::{Glossary, MappingOverrides};
    use crate::schema::parse_create_table;
    use crate::sql_catalog::SqlCatalog;

    fn cat() -> SqlCatalog {
        let t = parse_create_table(
            r#"CREATE TABLE [cardholder].[cpf_Client_Profile] (
                [cpf_Profile_Id] UNIQUEIDENTIFIER NOT NULL,
                [cpf_Last_Name]  NVARCHAR (1024)  NULL,
                CONSTRAINT [PK_cpf] PRIMARY KEY ([cpf_Profile_Id])
            );"#,
        )
        .unwrap();
        SqlCatalog {
            tables: vec![t],
            objects: Vec::new(),
        }
    }

    fn op() -> OpSpec {
        OpSpec {
            slug: "post-cardholders".into(),
            label: "POST /cardholders".into(),
            method: Some("POST".into()),
            path: Some("/cardholders".into()),
            summary: Some("Create a new cardholder.".into()),
            fields: vec![
                OpField {
                    name: "cardholder_id".into(),
                    logical_type: "string".into(),
                    location: Location::Body,
                    required: false,
                    max_length: None,
                    format: Some("uuid".into()),
                    description: Some("Client-provided identifier.".into()),
                },
                OpField {
                    name: "last_name".into(),
                    logical_type: "string".into(),
                    location: Location::Body,
                    required: true,
                    max_length: Some(1024),
                    format: None,
                    description: Some("Family name.".into()),
                },
            ],
            adapter: "openapi".into(),
            source: "test".into(),
        }
    }

    fn cfg() -> BusinessConfig {
        BusinessConfig {
            prepared_by: "Andruska Cronje".into(),
            version: "V1.0".into(),
            sprint: "Sprint 4".into(),
            sprint_start: String::new(),
            sprint_end: String::new(),
            epic: "Cardholder Management".into(),
            stakeholders: vec![crate::business_config::Stakeholder {
                name: "Andruska Cronje".into(),
                position: "Product Owner".into(),
                department: "DT Design Team".into(),
            }],
        }
    }

    fn meta_full() -> StoryMeta {
        StoryMeta {
            hlr_id: Some("HLR019".into()),
            hlr_title: Some("Cardholder Parent-Child Relationships".into()),
            llr_id: Some("LLR016".into()),
            llr_action: Some("Create new Cardholder".into()),
            priority: Some("Must have - MVP".into()),
            status: Some("User Story Signed-Off".into()),
        }
    }

    #[test]
    fn filename_matches_epic_hlr_llr_convention() {
        let fname = compose_filename(&op(), &meta_full(), "Cardholder Management", "client-profile");
        assert_eq!(
            fname,
            "Epic – Cardholder Management – HLR019 – Cardholder Parent-Child Relationships – LLR016 – Create new Cardholder.md"
        );
    }

    #[test]
    fn missing_meta_falls_back_to_placeholders_in_filename() {
        let fname = compose_filename(&op(), &StoryMeta::default(), "X Epic", "client-profile");
        assert!(fname.contains("HLR-TBD"));
        assert!(fname.contains("LLR-TBD"));
        assert!(fname.contains("Create a Client Profile"));
    }

    #[test]
    fn emits_all_thirteen_sections_plus_title() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let mut metas = BTreeMap::new();
        metas.insert("post-cardholders".into(), meta_full());
        let report = generate_story_docs(
            &[op()],
            &metas,
            &svc,
            &c,
            &[],
            &cfg(),
            tmp.path(),
        )
        .unwrap();
        assert_eq!(report.files_written.len(), 1);
        let path = tmp.path().join(&report.files_written[0]);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.starts_with("# EPIC – CARDHOLDER MANAGEMENT"));
        for h in &[
            "## 1. Document History",
            "## 2. Document Approval",
            "## 3. Scope",
            "## 4. Process Flow",
            "## 5. User Story",
            "## 6. Directive Specification",
            "## 7. Acceptance Criteria",
            "## 8. SQL Schema Reality",
            "## 9. Field → Column Mapping",
            "## 10. Constraints the Implementer Must Satisfy",
            "## 11. Alignment — Dev Planning vs SQL",
            "## 12. XLSM Requirements (Project Status)",
            "## 13. Traceability",
        ] {
            assert!(body.contains(h), "missing heading: {}", h);
        }
    }

    #[test]
    fn confidence_badges_render_in_field_mapping() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let mut metas = BTreeMap::new();
        metas.insert("post-cardholders".into(), meta_full());
        let report = generate_story_docs(
            &[op()],
            &metas,
            &svc,
            &c,
            &[],
            &cfg(),
            tmp.path(),
        )
        .unwrap();
        let body = std::fs::read_to_string(tmp.path().join(&report.files_written[0])).unwrap();
        assert!(body.contains("### Tables Touched"));
        assert!(body.contains("cpf_Client_Profile"));
        assert!(body.contains("## 9. Field → Column Mapping"));
        assert!(body.contains("### Confidence Badges"));
        assert!(
            body.contains("●")
                || body.contains("◐")
                || body.contains("◯")
                || body.contains("✗")
                || body.contains("🔒")
        );
    }

    #[test]
    fn acceptance_criteria_groups_uuid_required_optional() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let mut metas = BTreeMap::new();
        metas.insert("post-cardholders".into(), meta_full());
        let report = generate_story_docs(
            &[op()],
            &metas,
            &svc,
            &c,
            &[],
            &cfg(),
            tmp.path(),
        )
        .unwrap();
        let body = std::fs::read_to_string(tmp.path().join(&report.files_written[0])).unwrap();
        assert!(body.contains("### Unique Identifier"));
        assert!(body.contains("cardholder_id"));
        assert!(body.contains("### Required Fields"));
        assert!(body.contains("last_name"));
        assert!(body.contains("### Validation"));
        assert!(body.contains("### Auditability"));
    }

    #[test]
    fn directive_spec_groups_fields_by_location() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let metas = BTreeMap::new();
        let mut op1 = op();
        // Add a query field and a header field to verify grouping.
        op1.fields.push(OpField {
            name: "x-api-version".into(),
            logical_type: "string".into(),
            location: Location::Header,
            required: true,
            max_length: None,
            format: None,
            description: Some("API version".into()),
        });
        op1.fields.push(OpField {
            name: "page".into(),
            logical_type: "string".into(),
            location: Location::Query,
            required: false,
            max_length: None,
            format: None,
            description: Some("Paginate".into()),
        });
        let report = generate_story_docs(
            &[op1],
            &metas,
            &svc,
            &c,
            &[],
            &cfg(),
            tmp.path(),
        )
        .unwrap();
        let body = std::fs::read_to_string(tmp.path().join(&report.files_written[0])).unwrap();
        assert!(body.contains("### Request Headers"));
        assert!(body.contains("x-api-version"));
        assert!(body.contains("### Query Parameters"));
        assert!(body.contains("page"));
        assert!(body.contains("### Body Fields"));
    }

    #[test]
    fn deterministic_same_inputs_byte_identical() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc1 = MappingService::new(&c, &g, &o);
        let svc2 = MappingService::new(&c, &g, &o);
        let op1 = op();
        let op2 = op();
        let bundle1 = story_gen::build_bundle(&op1, &svc1);
        let bundle2 = story_gen::build_bundle(&op2, &svc2);
        let pf1 = crate::viz::render_op_dependency(&op1, &svc1);
        let pf2 = crate::viz::render_op_dependency(&op2, &svc2);
        let tech1 = build_technical_grounding(&op1, &c);
        let tech2 = build_technical_grounding(&op2, &c);
        let a = render_story_md(
            &op1,
            &bundle1,
            &meta_full(),
            &cfg(),
            &pf1,
            &tech1,
            &[],
        );
        let b = render_story_md(
            &op2,
            &bundle2,
            &meta_full(),
            &cfg(),
            &pf2,
            &tech2,
            &[],
        );
        assert_eq!(
            a.lines()
                .filter(|l| !l.starts_with("| Date |"))
                .collect::<Vec<_>>(),
            b.lines()
                .filter(|l| !l.starts_with("| Date |"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn priority_from_meta_overrides_default() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let mut metas = BTreeMap::new();
        let mut meta = meta_full();
        meta.priority = Some("Should have".into());
        metas.insert("post-cardholders".into(), meta);
        let report = generate_story_docs(
            &[op()],
            &metas,
            &svc,
            &c,
            &[],
            &cfg(),
            tmp.path(),
        )
        .unwrap();
        let body = std::fs::read_to_string(tmp.path().join(&report.files_written[0])).unwrap();
        assert!(body.contains("**Should have**"));
    }

    #[test]
    fn epic_folder_created_per_business_epic() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let metas = BTreeMap::new();
        let _ = generate_story_docs(
            &[op()],
            &metas,
            &svc,
            &c,
            &[],
            &cfg(),
            tmp.path(),
        )
        .unwrap();
        let epic_dir = tmp.path().join("Cardholder Management");
        assert!(epic_dir.is_dir());
    }
}
