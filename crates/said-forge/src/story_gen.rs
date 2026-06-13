//! Per-op story generation — bridges directive + MappingService into
//! two artefacts:
//!
//! 1. **Flat `user-stories/<entity>.txt`** — shape consumable by the
//!    existing `txn-api-generator` SKILL.md parser. One file per
//!    resolved entity (grouping all ops on the same primary table).
//!    Fields follow the "Field | Type | Required | Permitted values |
//!    Format" table the skill expects.
//!
//! 2. **Rich `.forge/stories/<Group>/<METHOD>-<slug>.md`** — per-op
//!    dev-facing view with the mapping-service bindings (tables,
//!    proc, column-per-field) each tagged with a confidence badge.
//!    This is what a developer reads alongside the skill output when
//!    debugging or reviewing.
//!
//! The stories are pure data: same (catalog, glossary, overrides,
//! ops) input → byte-identical output. Deterministic ordering via
//! BTreeMap and sorted filename emission.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::directive::{Location, OpField, OpSpec};
use crate::error::{ForgeError, ForgeResult};
use crate::mapping_service::{
    Confidence, MappingService, TableRole,
};
use crate::schema_diff::Verdict;

// ─────────────────────────── public API ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoriesReport {
    pub user_stories_dir: PathBuf,
    pub rich_stories_dir: PathBuf,
    pub user_story_files: Vec<String>,
    pub rich_story_files: Vec<String>,
    pub total_ops: usize,
}

/// Generate both flat user-stories/*.txt files and rich
/// `.forge/stories/<Group>/*.md` files.
///
/// - `ops` — pre-extracted directive ops (OpenAPI or Dev Planning).
///   Caller pipes these in; `story_gen` does not read the directive
///   itself (separation of concerns + easier to test).
/// - `service` — the constructed MappingService (catalog + glossary
///   + overrides). All mapping decisions route through it so the
///   trace log captures every call.
/// - `workspace_root` — where `user-stories/` lands.
/// - `stories_out_dir` — where the rich stories land (conventionally
///   `.forge/stories/`).
pub fn generate_stories(
    ops: &[OpSpec],
    service: &MappingService<'_>,
    workspace_root: &Path,
    stories_out_dir: &Path,
) -> ForgeResult<StoriesReport> {
    let user_dir = workspace_root.join("user-stories");
    fs::create_dir_all(&user_dir).map_err(|e| ForgeError::Io {
        path: user_dir.display().to_string(),
        cause: e,
    })?;
    fs::create_dir_all(stories_out_dir).map_err(|e| ForgeError::Io {
        path: stories_out_dir.display().to_string(),
        cause: e,
    })?;

    // Build per-op bundles first — we reuse them for both outputs.
    let bundles: Vec<StoryBundle> = ops
        .iter()
        .map(|op| build_bundle(op, service))
        .collect();

    // Group by entity for the flat user-stories files.
    let mut by_entity: BTreeMap<String, Vec<&StoryBundle>> = BTreeMap::new();
    for b in &bundles {
        by_entity
            .entry(b.entity_slug.clone())
            .or_default()
            .push(b);
    }

    let mut user_story_files = Vec::new();
    for (entity, ops) in &by_entity {
        let fname = format!("{}.txt", entity);
        let path = user_dir.join(&fname);
        let body = render_user_story_txt(entity, ops);
        fs::write(&path, body).map_err(|e| ForgeError::Io {
            path: path.display().to_string(),
            cause: e,
        })?;
        user_story_files.push(fname);
    }

    // Per-op rich stories grouped by derived group (from op.label first
    // token or `/<group>/` path segment).
    let mut rich_story_files = Vec::new();
    for b in &bundles {
        let group = derive_group(&b.op);
        let file_stem = rich_story_filename(&b.op);
        let group_dir = stories_out_dir.join(&group);
        fs::create_dir_all(&group_dir).map_err(|e| ForgeError::Io {
            path: group_dir.display().to_string(),
            cause: e,
        })?;
        let path = group_dir.join(format!("{}.md", file_stem));
        let body = render_rich_story_md(b);
        fs::write(&path, body).map_err(|e| ForgeError::Io {
            path: path.display().to_string(),
            cause: e,
        })?;
        rich_story_files.push(format!("{}/{}.md", group, file_stem));
    }
    rich_story_files.sort();
    user_story_files.sort();

    Ok(StoriesReport {
        user_stories_dir: user_dir,
        rich_stories_dir: stories_out_dir.to_path_buf(),
        user_story_files,
        rich_story_files,
        total_ops: ops.len(),
    })
}

// ─────────────────────────── bundle ───────────────────────────

/// Everything we know about an op after MappingService has resolved it.
pub struct StoryBundle<'a> {
    pub op: &'a OpSpec,
    pub entity_slug: String,
    pub primary_tables: Vec<(String, Confidence)>,
    pub related_tables: Vec<(String, Confidence)>,
    pub procs: Vec<(String, Confidence)>,
    pub field_bindings: Vec<FieldBinding>,
}

pub struct FieldBinding {
    pub field: OpField,
    pub table: String,
    pub column: String,
    pub column_type: String,
    pub verdict: Verdict,
    pub confidence: Confidence,
}

pub fn build_bundle<'a>(op: &'a OpSpec, service: &MappingService<'_>) -> StoryBundle<'a> {
    let table_results = service.resolve_tables_for_op(op);
    let mut primary_tables = Vec::new();
    let mut related_tables = Vec::new();
    let mut primary_table_names = Vec::new();
    for r in &table_results {
        match r.value.role {
            TableRole::Primary => {
                primary_tables.push((r.value.full_name.clone(), r.confidence));
                primary_table_names.push(r.value.full_name.clone());
            }
            TableRole::Related => {
                related_tables.push((r.value.full_name.clone(), r.confidence));
            }
        }
    }

    // Primary table set for column + proc resolvers.
    let catalog = service.catalog;
    let primary_refs: Vec<&crate::schema::TableSchema> = primary_tables
        .iter()
        .filter_map(|(name, _)| catalog.find_table(name))
        .collect();

    let proc_results = service.resolve_proc_for_op(op, &primary_table_names);
    let procs: Vec<(String, Confidence)> = proc_results
        .into_iter()
        .map(|p| (p.value.full_name, p.confidence))
        .collect();

    let mut field_bindings = Vec::with_capacity(op.fields.len());
    for f in &op.fields {
        if !matches!(f.location, Location::Body | Location::Path) {
            field_bindings.push(FieldBinding {
                field: f.clone(),
                table: String::new(),
                column: String::new(),
                column_type: String::new(),
                verdict: Verdict::Skipped,
                confidence: Confidence::None,
            });
            continue;
        }
        let r = service.resolve_column(&op.slug, f, &primary_refs);
        field_bindings.push(FieldBinding {
            field: f.clone(),
            table: r.value.table,
            column: r.value.column,
            column_type: r.value.column_type,
            verdict: r.value.verdict,
            confidence: r.confidence,
        });
    }

    let entity_slug = derive_entity_slug(op, &primary_tables);

    StoryBundle {
        op,
        entity_slug,
        primary_tables,
        related_tables,
        procs,
        field_bindings,
    }
}

// ─────────────────────────── user-story .txt writer ───────────────────────────

fn render_user_story_txt(entity: &str, ops: &[&StoryBundle]) -> String {
    let pretty_entity = title_case_entity(entity);
    let mut out = String::new();
    out.push_str(&format!("# {} User Story\n\n", pretty_entity));

    out.push_str("## Entity\n\n");
    out.push_str(&format!("- Name: {}\n", pretty_entity));
    out.push_str(&format!("- Name (plural): {}s\n", pretty_entity));
    if let Some(first) = ops.first() {
        if let Some((t, _)) = first.primary_tables.first() {
            let schema = t
                .split_once('.')
                .map(|(s, _)| format!("[{}]", s))
                .unwrap_or_else(|| "[default]".into());
            out.push_str(&format!("- DB schema: {}\n", schema));
        }
    }
    out.push('\n');

    for op in ops {
        let method = op.op.method.clone().unwrap_or_else(|| "GET".into()).to_uppercase();
        let path = op.op.path.clone().unwrap_or_else(|| op.op.label.clone());
        out.push_str(&format!("## Endpoint: {} {}\n\n", method, path));
        if let Some(s) = &op.op.summary {
            out.push_str(&format!("{}\n\n", s));
        }

        let body_or_path: Vec<&FieldBinding> = op
            .field_bindings
            .iter()
            .filter(|b| matches!(b.field.location, Location::Body | Location::Path))
            .collect();

        if body_or_path.is_empty() {
            out.push_str("_(no body or path fields on this op)_\n\n");
            continue;
        }

        out.push_str("| Field | Type | Required | Permitted values | Format |\n");
        out.push_str("|-------|------|----------|------------------|--------|\n");
        for b in body_or_path {
            let required = if b.field.required { "yes" } else { "no" };
            let ty = b.field.logical_type.clone();
            let format = b.field.format.clone().unwrap_or_else(|| "—".into());
            let permitted = if let Some(desc) = &b.field.description {
                truncate(desc, 80)
            } else {
                "—".into()
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                b.field.name, ty, required, permitted, format
            ));
        }
        out.push('\n');
    }

    out
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.replace('\n', " ").replace('|', "/");
    }
    let clipped: String = s.chars().take(max).collect();
    format!("{}…", clipped.replace('\n', " ").replace('|', "/"))
}

// ─────────────────────────── rich .md writer ───────────────────────────

fn render_rich_story_md(b: &StoryBundle) -> String {
    let method = b.op.method.clone().unwrap_or_else(|| "—".into());
    let path = b.op.path.clone().unwrap_or_else(|| b.op.label.clone());
    let mut out = String::new();

    out.push_str(&format!("# {} {}\n\n", method, path));
    out.push_str(&format!("**Slug:** `{}`\n\n", b.op.slug));
    if let Some(s) = &b.op.summary {
        out.push_str(&format!("{}\n\n", s));
    }

    out.push_str("## Tables\n\n");
    if b.primary_tables.is_empty() && b.related_tables.is_empty() {
        out.push_str("_(no candidate tables matched — the mapping service could not pick a primary table for this op)_\n\n");
    } else {
        out.push_str("| role | table | confidence |\n");
        out.push_str("|------|-------|------------|\n");
        for (name, conf) in &b.primary_tables {
            out.push_str(&format!("| primary | `{}` | {} |\n", name, conf.badge()));
        }
        for (name, conf) in &b.related_tables {
            out.push_str(&format!("| related | `{}` | {} |\n", name, conf.badge()));
        }
        out.push('\n');
    }

    out.push_str("## Procedures\n\n");
    if b.procs.is_empty() {
        out.push_str("_(no procedures in the catalog reference this op's tables — implementer will scaffold new ones)_\n\n");
    } else {
        out.push_str("| proc | confidence |\n");
        out.push_str("|------|------------|\n");
        for (name, conf) in &b.procs {
            out.push_str(&format!("| `{}` | {} |\n", name, conf.badge()));
        }
        out.push('\n');
    }

    out.push_str("## Field → column bindings\n\n");
    let db_fields: Vec<&FieldBinding> = b
        .field_bindings
        .iter()
        .filter(|fb| matches!(fb.field.location, Location::Body | Location::Path))
        .collect();
    if db_fields.is_empty() {
        out.push_str("_(no body or path fields on this op)_\n\n");
    } else {
        out.push_str("| field | api | SQL column | verdict | confidence |\n");
        out.push_str("|-------|-----|------------|---------|------------|\n");
        for fb in db_fields {
            let required = if fb.field.required { "req" } else { "opt" };
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
                Verdict::Missing => ("✗ missing".to_string(), "—".to_string()),
                Verdict::Skipped => ("—".to_string(), "_n/a_".to_string()),
            };
            out.push_str(&format!(
                "| `{}` | {} ({}) | {} | {} | {} |\n",
                fb.field.name,
                fb.field.logical_type,
                required,
                col_cell,
                verdict_mark,
                fb.confidence.badge()
            ));
        }
        out.push('\n');
    }

    out.push_str("## How to read the confidence badges\n\n");
    out.push_str("- `🔒 explicit` — `.forge/mapping.toml` wins. Ship verbatim.\n");
    out.push_str("- `● high` — exact symbol or dominant token match.\n");
    out.push_str("- `◐ medium` — multi-signal heuristic. Review before shipping.\n");
    out.push_str("- `◯ low` — single-signal heuristic. Review carefully.\n");
    out.push_str("- `✗ none` — no mapping produced.\n");

    out
}

// ─────────────────────────── slug + group derivation ───────────────────────────

fn derive_entity_slug(op: &OpSpec, primary_tables: &[(String, Confidence)]) -> String {
    if let Some((t, _)) = primary_tables.first() {
        let bare = t.rsplit('.').next().unwrap_or(t);
        let entity = strip_hungarian(bare);
        if !entity.is_empty() {
            return sanitize_slug_for_entity(&entity);
        }
    }
    if let Some(path) = &op.path {
        let first = path
            .trim_start_matches('/')
            .split('/')
            .next()
            .unwrap_or("")
            .to_string();
        if !first.is_empty() {
            return sanitize_slug_for_entity(&singularise(&first));
        }
    }
    sanitize_slug_for_entity(&op.slug)
}

fn sanitize_slug_for_entity(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '_' || c == '-' || c == ' ' {
            if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
    }
    out.trim_matches('-').to_string()
}

fn strip_hungarian(s: &str) -> String {
    let mut parts = s.splitn(2, '_');
    let first = parts.next().unwrap_or("");
    let rest = parts.next();
    if rest.is_some()
        && (2..=4).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_lowercase())
    {
        rest.unwrap().to_string()
    } else {
        s.to_string()
    }
}

fn singularise(s: &str) -> String {
    if s.ends_with("ies") {
        let mut o = s.to_string();
        o.truncate(o.len() - 3);
        o.push('y');
        return o;
    }
    if s.ends_with('s') && !s.ends_with("ss") {
        return s[..s.len() - 1].to_string();
    }
    s.to_string()
}

fn derive_group(op: &OpSpec) -> String {
    if let Some(path) = &op.path {
        let first = path.trim_start_matches('/').split('/').next().unwrap_or("");
        if !first.is_empty() {
            let mut chars = first.chars();
            let mut out = String::new();
            if let Some(c) = chars.next() {
                out.extend(c.to_uppercase());
            }
            for c in chars {
                out.push(c);
            }
            return out;
        }
    }
    "Uncategorised".into()
}

fn rich_story_filename(op: &OpSpec) -> String {
    let method = op.method.clone().unwrap_or_else(|| "OP".into()).to_uppercase();
    let base = sanitize_slug_for_entity(&op.slug);
    format!("{}-{}", method, base)
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

    fn op_create() -> OpSpec {
        OpSpec {
            slug: "post-cardholders".into(),
            label: "POST /cardholders".into(),
            method: Some("POST".into()),
            path: Some("/cardholders".into()),
            summary: Some("Create a cardholder.".into()),
            fields: vec![
                OpField {
                    name: "last_name".into(),
                    logical_type: "string".into(),
                    location: Location::Body,
                    required: true,
                    max_length: Some(1024),
                    format: None,
                    description: Some("Family name of the cardholder.".into()),
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
    fn emits_one_user_story_file_per_entity_with_endpoint_table() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let report =
            generate_stories(&[op_create()], &svc, tmp.path(), &tmp.path().join(".forge/stories"))
                .unwrap();
        assert_eq!(report.user_story_files.len(), 1);
        let fname = &report.user_story_files[0];
        assert!(fname.ends_with(".txt"));
        let body = std::fs::read_to_string(tmp.path().join("user-stories").join(fname)).unwrap();
        assert!(body.contains("## Entity"));
        assert!(body.contains("## Endpoint: POST /cardholders"));
        assert!(body.contains("| Field |"));
        assert!(body.contains("last_name"));
        assert!(body.contains("nationality"));
    }

    #[test]
    fn emits_rich_story_with_bindings_and_confidence_badges() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let stories_dir = tmp.path().join(".forge/stories");
        let report =
            generate_stories(&[op_create()], &svc, tmp.path(), &stories_dir).unwrap();
        assert_eq!(report.rich_story_files.len(), 1);
        let rel = &report.rich_story_files[0];
        let rich = std::fs::read_to_string(stories_dir.join(rel)).unwrap();
        assert!(rich.contains("# POST /cardholders"));
        assert!(rich.contains("## Tables"));
        assert!(rich.contains("cpf_Client_Profile"));
        assert!(rich.contains("## Field → column bindings"));
        assert!(rich.contains("last_name"));
        // nationality has no match → ✗ missing
        assert!(rich.contains("nationality"));
        assert!(rich.contains("✗ missing"));
        // confidence badge legend
        assert!(rich.contains("explicit"));
    }

    #[test]
    fn override_routes_through_stories_with_explicit_badge() {
        let c = cat();
        let g = Glossary::default();
        let mut o = MappingOverrides::default();
        o.insert_op_table("post-cardholders", "cardholder.cpf_Client_Profile".into());
        o.insert_op_proc("post-cardholders", "cardholder.p_txn_Update_Cardholder".into());
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let stories_dir = tmp.path().join(".forge/stories");
        let report =
            generate_stories(&[op_create()], &svc, tmp.path(), &stories_dir).unwrap();
        let rel = &report.rich_story_files[0];
        let rich = std::fs::read_to_string(stories_dir.join(rel)).unwrap();
        assert!(rich.contains("🔒 explicit"));
        assert!(rich.contains("p_txn_Update_Cardholder"));
    }

    #[test]
    fn groups_ops_by_path_first_segment() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let stories_dir = tmp.path().join(".forge/stories");
        let report =
            generate_stories(&[op_create()], &svc, tmp.path(), &stories_dir).unwrap();
        // One file, grouped under "Cardholders/".
        let rel = &report.rich_story_files[0];
        assert!(rel.starts_with("Cardholders/"), "got: {}", rel);
    }

    #[test]
    fn deterministic_same_inputs_byte_identical() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp1 = tempfile::tempdir().unwrap();
        let tmp2 = tempfile::tempdir().unwrap();
        let r1 = generate_stories(
            &[op_create()],
            &svc,
            tmp1.path(),
            &tmp1.path().join(".forge/stories"),
        )
        .unwrap();
        let svc2 = MappingService::new(&c, &g, &o);
        let r2 = generate_stories(
            &[op_create()],
            &svc2,
            tmp2.path(),
            &tmp2.path().join(".forge/stories"),
        )
        .unwrap();
        assert_eq!(r1.user_story_files, r2.user_story_files);
        let fname = &r1.user_story_files[0];
        let a = std::fs::read_to_string(tmp1.path().join("user-stories").join(fname)).unwrap();
        let b = std::fs::read_to_string(tmp2.path().join("user-stories").join(fname)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn entity_slug_derives_from_primary_table_with_hungarian_stripped() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let report =
            generate_stories(&[op_create()], &svc, tmp.path(), &tmp.path().join(".forge/stories"))
                .unwrap();
        // cpf_Client_Profile → client-profile after hungarian strip + dasher
        let fname = &report.user_story_files[0];
        assert!(
            fname.contains("client") || fname.contains("cardholder"),
            "unexpected entity slug: {}",
            fname
        );
    }

    #[test]
    fn field_with_no_match_shows_missing_in_rich_view_with_none_confidence() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let tmp = tempfile::tempdir().unwrap();
        let stories_dir = tmp.path().join(".forge/stories");
        let _ = generate_stories(&[op_create()], &svc, tmp.path(), &stories_dir).unwrap();
        let rich_files: Vec<_> = std::fs::read_dir(stories_dir.join("Cardholders"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(rich_files.len(), 1);
        let rich = std::fs::read_to_string(&rich_files[0]).unwrap();
        assert!(rich.contains("nationality"));
        assert!(rich.contains("✗ missing"));
        assert!(rich.contains("✗ none"));
    }

    #[test]
    fn query_and_header_fields_skipped_in_user_story_txt() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let mut op = op_create();
        op.fields.push(OpField {
            name: "page_size".into(),
            logical_type: "integer".into(),
            location: Location::Query,
            required: false,
            max_length: None,
            format: None,
            description: None,
        });
        let tmp = tempfile::tempdir().unwrap();
        let report = generate_stories(
            &[op],
            &svc,
            tmp.path(),
            &tmp.path().join(".forge/stories"),
        )
        .unwrap();
        let txt =
            std::fs::read_to_string(tmp.path().join("user-stories").join(&report.user_story_files[0]))
                .unwrap();
        // Query field must not appear in the fields table.
        assert!(!txt.contains("page_size"));
    }
}
