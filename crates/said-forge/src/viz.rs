//! Mermaid visualisation — renders existing catalog + mapping-service
//! data as markdown-native `erDiagram` / `flowchart` / `graph`
//! blocks. Zero new runtime deps; same deterministic-output promise
//! as the rest of forge.
//!
//! Three diagram kinds:
//!
//! 1. **ERD** (`render_erd`) — `erDiagram` of every `TableSchema` in
//!    the catalog with its columns, PK marks, and FK relationships.
//!    Drops into Claude Code preview + GitHub rendering for free.
//!
//! 2. **Op dependency graph** (`render_op_dependency`) — `flowchart LR`
//!    for one op showing: op node → primary tables → related tables
//!    (via FK) → procedures. Edges colour-coded by confidence
//!    (explicit=green, high=blue, medium=amber, low=orange, none=red).
//!
//! 3. **Authority flow** (`render_authority_flow`) — `graph TD` of
//!    the workspace's five-level authority stack (Law / Existing /
//!    Agreed / Requested / Wishlist) with the actual directory paths
//!    plugged in.

use crate::directive::OpSpec;
use crate::mapping_service::{Confidence, MappingService, TableRole};
use crate::schema::TableSchema;
use crate::sql_catalog::{SqlCatalog, SqlKind};

// ─────────────────────────── ERD ───────────────────────────

/// Render an ER diagram covering every table in `catalog`. When
/// `schema_filter` is `Some(name)`, only tables in that SQL schema
/// are drawn.
pub fn render_erd(catalog: &SqlCatalog, schema_filter: Option<&str>) -> String {
    let tables: Vec<&TableSchema> = catalog
        .tables
        .iter()
        .filter(|t| {
            schema_filter
                .map(|s| t.schema.as_deref() == Some(s))
                .unwrap_or(true)
        })
        .collect();

    let mut out = String::new();
    out.push_str("```mermaid\nerDiagram\n");
    if tables.is_empty() {
        out.push_str("    %% no tables matched filter\n");
        out.push_str("```\n");
        return out;
    }

    // Emit each table as an entity with its columns.
    for t in &tables {
        let ident = erd_ident(&t.full_name());
        out.push_str(&format!("    {} {{\n", ident));
        for c in &t.columns {
            let pk_mark = if t.primary_key.iter().any(|p| p == &c.name) {
                " PK"
            } else if t
                .foreign_keys
                .iter()
                .any(|fk| fk.column == c.name)
            {
                " FK"
            } else {
                ""
            };
            let ty = c.sql_type.to_ascii_uppercase();
            out.push_str(&format!(
                "        {} {}{}\n",
                ty,
                erd_column_ident(&c.name),
                pk_mark
            ));
        }
        out.push_str("    }\n");
    }

    // Relationships — one edge per FK where the target table is
    // present in the filtered set. Cardinality: FK side is many,
    // referenced side is one (many-to-one).
    for t in &tables {
        for fk in &t.foreign_keys {
            let target_full = match &fk.ref_schema {
                Some(s) => format!("{}.{}", s, fk.ref_table),
                None => fk.ref_table.clone(),
            };
            if !tables.iter().any(|t2| t2.full_name() == target_full) {
                continue;
            }
            let src = erd_ident(&t.full_name());
            let dst = erd_ident(&target_full);
            out.push_str(&format!(
                "    {} }}o--|| {} : \"{}\"\n",
                src, dst, fk.column
            ));
        }
    }

    out.push_str("```\n");
    out
}

fn erd_ident(full_name: &str) -> String {
    // Mermaid ERD idents allow [A-Za-z0-9_]. Replace any '.' with '_'.
    full_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn erd_column_ident(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

// ─────────────────────────── op dependency graph ───────────────────────────

/// Render a flowchart for one op showing op → tables → procs with
/// confidence-tinted edges.
pub fn render_op_dependency(op: &OpSpec, service: &MappingService<'_>) -> String {
    let mut out = String::new();
    out.push_str("```mermaid\nflowchart LR\n");

    let op_id = format!("op_{}", flowchart_ident(&op.slug));
    let method = op.method.clone().unwrap_or_else(|| "—".into());
    let path = op.path.clone().unwrap_or_else(|| op.label.clone());
    out.push_str(&format!(
        "    {}[\"{} {}\"]\n",
        op_id,
        mermaid_escape(&method),
        mermaid_escape(&path)
    ));
    out.push_str(&format!("    class {} op;\n", op_id));

    let table_results = service.resolve_tables_for_op(op);
    if table_results.is_empty() {
        out.push_str("    none[\"no matching tables — escalate to schema owner\"]\n");
        out.push_str(&format!("    {} --> none\n", op_id));
        out.push_str("    classDef op fill:#eef,stroke:#88f;\n");
        out.push_str("```\n");
        return out;
    }

    let mut primary_ids = Vec::new();
    let mut seen_tables = std::collections::BTreeSet::new();
    let mut table_names: Vec<String> = Vec::new();
    for r in &table_results {
        if !seen_tables.insert(r.value.full_name.clone()) {
            continue;
        }
        let ident = format!("t_{}", flowchart_ident(&r.value.full_name));
        let role_label = match r.value.role {
            TableRole::Primary => "primary",
            TableRole::Related => "related",
        };
        out.push_str(&format!(
            "    {}[\"{}\\n({})\"]\n",
            ident,
            mermaid_escape(&r.value.full_name),
            role_label
        ));
        let class = confidence_class(r.confidence);
        out.push_str(&format!("    class {} {};\n", ident, class));
        let edge = confidence_edge(r.confidence);
        out.push_str(&format!("    {} {} {}\n", op_id, edge, ident));
        if r.value.role == TableRole::Primary {
            primary_ids.push(r.value.full_name.clone());
        }
        table_names.push(r.value.full_name.clone());
    }

    // Procedures feeding from primary tables.
    let proc_results = service.resolve_proc_for_op(op, &primary_ids);
    for p in &proc_results {
        let pident = format!("p_{}", flowchart_ident(&p.value.full_name));
        out.push_str(&format!(
            "    {}[(\"{}\")]\n",
            pident,
            mermaid_escape(&p.value.full_name)
        ));
        let class = confidence_class(p.confidence);
        out.push_str(&format!("    class {} {};\n", pident, class));
        let edge = confidence_edge(p.confidence);
        // Link proc to every table it references that's in scope.
        for (table, _ops) in &p.value.table_ops {
            if let Some(target) = table_names.iter().find(|tn| tn.ends_with(table) || *tn == table) {
                let tident = format!("t_{}", flowchart_ident(target));
                out.push_str(&format!("    {} {} {}\n", tident, edge, pident));
            }
        }
    }

    // classDefs — one per confidence tier plus op.
    out.push_str("    classDef op fill:#eef,stroke:#88f,stroke-width:2px;\n");
    out.push_str("    classDef explicit fill:#dfd,stroke:#080;\n");
    out.push_str("    classDef high fill:#def,stroke:#08c;\n");
    out.push_str("    classDef medium fill:#ffe8bf,stroke:#c80;\n");
    out.push_str("    classDef low fill:#fde,stroke:#c40;\n");
    out.push_str("    classDef none_ fill:#fdd,stroke:#c00;\n");

    out.push_str("```\n");
    out
}

fn confidence_class(c: Confidence) -> &'static str {
    match c {
        Confidence::Explicit => "explicit",
        Confidence::High => "high",
        Confidence::Medium => "medium",
        Confidence::Low => "low",
        Confidence::None => "none_",
    }
}

fn confidence_edge(c: Confidence) -> &'static str {
    // Solid arrow for strong evidence, dotted for weak.
    match c {
        Confidence::Explicit | Confidence::High => "-->",
        Confidence::Medium => "-->",
        Confidence::Low => "-.->",
        Confidence::None => "-.->",
    }
}

fn flowchart_ident(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn mermaid_escape(s: &str) -> String {
    // Mermaid node labels in quoted form tolerate most chars; just
    // escape double-quotes + backticks.
    s.replace('"', "'").replace('`', "'")
}

// ─────────────────────────── authority flow ───────────────────────────

/// Static authority flow diagram — the five tiers with the actual
/// workspace directory paths plugged in. `paths` is rendered
/// verbatim next to each tier; missing entries get a `—`.
pub fn render_authority_flow(paths: &AuthorityPaths) -> String {
    let mut out = String::new();
    out.push_str("```mermaid\ngraph TD\n");
    out.push_str(&format!(
        "    law[\"LAW · SQL ground truth\\n{}\"]\n",
        display_or_dash(paths.law.as_deref())
    ));
    out.push_str(&format!(
        "    existing[\"EXISTING · C# (feapiTxnGlobal)\\n{}\"]\n",
        display_or_dash(paths.existing.as_deref())
    ));
    out.push_str(&format!(
        "    agreed[\"AGREED · Business stories\\n{}\"]\n",
        display_or_dash(paths.agreed.as_deref())
    ));
    out.push_str(&format!(
        "    requested[\"REQUESTED · Dev Planning\\n{}\"]\n",
        display_or_dash(paths.requested.as_deref())
    ));
    out.push_str(&format!(
        "    wishlist[\"WISHLIST · OpenAPI\\n{}\"]\n",
        display_or_dash(paths.wishlist.as_deref())
    ));
    out.push_str("    law --> existing\n");
    out.push_str("    existing --> agreed\n");
    out.push_str("    agreed --> requested\n");
    out.push_str("    requested --> wishlist\n");
    out.push_str("    classDef law fill:#dfd,stroke:#080;\n");
    out.push_str("    classDef existing fill:#def,stroke:#08c;\n");
    out.push_str("    classDef agreed fill:#def,stroke:#08c;\n");
    out.push_str("    classDef requested fill:#ffe8bf,stroke:#c80;\n");
    out.push_str("    classDef wishlist fill:#fde,stroke:#c40;\n");
    out.push_str("    class law law;\n");
    out.push_str("    class existing existing;\n");
    out.push_str("    class agreed agreed;\n");
    out.push_str("    class requested requested;\n");
    out.push_str("    class wishlist wishlist;\n");
    out.push_str("```\n");
    out
}

#[derive(Debug, Clone, Default)]
pub struct AuthorityPaths {
    pub law: Option<String>,
    pub existing: Option<String>,
    pub agreed: Option<String>,
    pub requested: Option<String>,
    pub wishlist: Option<String>,
}

fn display_or_dash(s: Option<&str>) -> String {
    match s {
        Some(v) if !v.trim().is_empty() => mermaid_escape(v),
        _ => "—".into(),
    }
}

// ─────────────────────────── composite writer ───────────────────────────

/// Compose a single `viz.md` document with all three diagrams plus
/// a short narrative. Intended for `<workspace>/.forge/viz.md` so the
/// developer can eyeball the whole picture at once.
pub fn render_viz_document(
    catalog: &SqlCatalog,
    service: &MappingService<'_>,
    ops: &[OpSpec],
    authority: &AuthorityPaths,
) -> String {
    let mut out = String::new();
    out.push_str("# Forge visualisation\n\n");
    out.push_str("_Auto-generated from the current SQL catalog + mapping service. Re-run `said forge viz` after any schema or directive change._\n\n");

    out.push_str("## Authority flow\n\n");
    out.push_str("Five tiers of truth feeding forge. Higher tiers override lower.\n\n");
    out.push_str(&render_authority_flow(authority));

    out.push_str("\n## Entity relationship diagram\n\n");
    let schemas = distinct_schemas(catalog);
    out.push_str(&format!(
        "{} schemas · {} tables · {} FKs\n\n",
        schemas.len(),
        catalog.tables.len(),
        catalog
            .tables
            .iter()
            .map(|t| t.foreign_keys.len())
            .sum::<usize>()
    ));
    // If we have more than one schema, emit one ERD per schema so
    // Mermaid doesn't choke on 1000-node diagrams. Single-schema
    // workspaces get the full view.
    if schemas.len() <= 1 {
        out.push_str(&render_erd(catalog, None));
    } else {
        for schema in &schemas {
            out.push_str(&format!("\n### `{}`\n\n", schema));
            out.push_str(&render_erd(catalog, Some(schema)));
        }
    }

    out.push_str("\n## Op dependency graphs\n\n");
    out.push_str(&format!(
        "One flowchart per directive op. Edges tinted by MappingService confidence.\n\n"
    ));
    if ops.is_empty() {
        out.push_str("_(no ops — run `said forge sync` to ingest the directive first)_\n");
    } else {
        for op in ops {
            let method = op.method.clone().unwrap_or_else(|| "OP".into());
            let path = op.path.clone().unwrap_or_else(|| op.label.clone());
            out.push_str(&format!("\n### `{} {}`\n\n", method, path));
            out.push_str(&render_op_dependency(op, service));
        }
    }

    out.push_str("\n## Catalog summary\n\n");
    out.push_str("| kind | count |\n|------|-------|\n");
    out.push_str(&format!("| tables | {} |\n", catalog.tables.len()));
    out.push_str(&format!(
        "| procedures | {} |\n",
        catalog
            .objects
            .iter()
            .filter(|o| matches!(o.kind, SqlKind::Procedure))
            .count()
    ));
    out.push_str(&format!(
        "| views | {} |\n",
        catalog
            .objects
            .iter()
            .filter(|o| matches!(o.kind, SqlKind::View))
            .count()
    ));
    out.push_str(&format!(
        "| triggers | {} |\n",
        catalog
            .objects
            .iter()
            .filter(|o| matches!(o.kind, SqlKind::Trigger))
            .count()
    ));
    out.push_str(&format!(
        "| functions | {} |\n",
        catalog
            .objects
            .iter()
            .filter(|o| matches!(o.kind, SqlKind::Function))
            .count()
    ));
    out
}

fn distinct_schemas(catalog: &SqlCatalog) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> = Default::default();
    for t in &catalog.tables {
        if let Some(s) = &t.schema {
            set.insert(s.clone());
        }
    }
    set.into_iter().collect()
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
        let business = parse_create_table(
            r#"CREATE TABLE [cardholder].[cpf_Client_Profile] (
                [cpf_Profile_Id] UNIQUEIDENTIFIER NOT NULL,
                [cpf_Last_Name]  NVARCHAR (1024)  NULL,
                [col_Code]       VARCHAR (2)      NOT NULL,
                CONSTRAINT [PK_cpf] PRIMARY KEY ([cpf_Profile_Id]),
                CONSTRAINT [FK_col] FOREIGN KEY ([col_Code]) REFERENCES [lookups].[col_Country_Lookup] ([col_Code])
            );"#,
        )
        .unwrap();
        let lookup = parse_create_table(
            r#"CREATE TABLE [lookups].[col_Country_Lookup] (
                [col_Code]        VARCHAR (2) NOT NULL,
                [col_Description] NVARCHAR (255) NULL,
                CONSTRAINT [PK_col] PRIMARY KEY ([col_Code])
            );"#,
        )
        .unwrap();
        SqlCatalog {
            tables: vec![business, lookup],
            objects: Vec::new(),
        }
    }

    fn op() -> OpSpec {
        OpSpec {
            slug: "post-cardholders".into(),
            label: "POST /cardholders".into(),
            method: Some("POST".into()),
            path: Some("/cardholders".into()),
            summary: None,
            fields: vec![OpField {
                name: "last_name".into(),
                logical_type: "string".into(),
                location: Location::Body,
                required: true,
                max_length: Some(1024),
                format: None,
                description: None,
            }],
            adapter: "openapi".into(),
            source: "test".into(),
        }
    }

    #[test]
    fn erd_emits_mermaid_block_with_entities_and_pk_fk_marks() {
        let c = cat();
        let md = render_erd(&c, None);
        assert!(md.starts_with("```mermaid\nerDiagram\n"));
        assert!(md.contains("cardholder_cpf_Client_Profile {"));
        assert!(md.contains("lookups_col_Country_Lookup {"));
        assert!(md.contains("UNIQUEIDENTIFIER cpf_Profile_Id PK"));
        assert!(md.contains("VARCHAR col_Code FK"));
        // FK relationship line.
        assert!(md.contains("}o--||"));
        assert!(md.trim_end().ends_with("```"));
    }

    #[test]
    fn erd_filter_limits_to_one_schema() {
        let c = cat();
        let md = render_erd(&c, Some("lookups"));
        assert!(md.contains("lookups_col_Country_Lookup"));
        assert!(!md.contains("cardholder_cpf_Client_Profile"));
    }

    #[test]
    fn op_dependency_renders_op_tables_and_confidence_classes() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let md = render_op_dependency(&op(), &svc);
        assert!(md.starts_with("```mermaid\nflowchart LR\n"));
        assert!(md.contains("op_post_cardholders"));
        assert!(md.contains("cpf_Client_Profile"));
        // Related table via FK.
        assert!(md.contains("col_Country_Lookup"));
        // classDefs present.
        assert!(md.contains("classDef op"));
        assert!(md.contains("classDef high"));
        assert!(md.trim_end().ends_with("```"));
    }

    #[test]
    fn op_dependency_handles_no_tables_matched() {
        let c = SqlCatalog::default();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let md = render_op_dependency(&op(), &svc);
        assert!(md.contains("no matching tables"));
    }

    #[test]
    fn authority_flow_renders_five_tier_graph_with_paths() {
        let paths = AuthorityPaths {
            law: Some("1-ground-truth/".into()),
            existing: Some("dt/TXN/feapiTxnGlobal".into()),
            agreed: Some("dt/Additional/User-Stories".into()),
            requested: Some("4-expectations/Dev Planning".into()),
            wishlist: Some("4-expectations/openapi.yaml".into()),
        };
        let md = render_authority_flow(&paths);
        assert!(md.contains("LAW"));
        assert!(md.contains("EXISTING"));
        assert!(md.contains("AGREED"));
        assert!(md.contains("REQUESTED"));
        assert!(md.contains("WISHLIST"));
        assert!(md.contains("1-ground-truth/"));
        assert!(md.contains("feapiTxnGlobal"));
    }

    #[test]
    fn authority_flow_shows_dash_for_missing_paths() {
        let paths = AuthorityPaths::default();
        let md = render_authority_flow(&paths);
        // Each tier should surface the em-dash placeholder.
        assert!(md.matches('—').count() >= 5);
    }

    #[test]
    fn composite_viz_document_includes_all_sections() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc = MappingService::new(&c, &g, &o);
        let paths = AuthorityPaths {
            law: Some("sql/".into()),
            ..Default::default()
        };
        let md = render_viz_document(&c, &svc, &[op()], &paths);
        assert!(md.contains("# Forge visualisation"));
        assert!(md.contains("## Authority flow"));
        assert!(md.contains("## Entity relationship diagram"));
        assert!(md.contains("## Op dependency graphs"));
        assert!(md.contains("## Catalog summary"));
        // Multi-schema case emits per-schema ERD sub-headings.
        assert!(md.contains("### `cardholder`"));
        assert!(md.contains("### `lookups`"));
    }

    #[test]
    fn deterministic_output_same_inputs_byte_identical() {
        let c = cat();
        let g = Glossary::default();
        let o = MappingOverrides::default();
        let svc1 = MappingService::new(&c, &g, &o);
        let svc2 = MappingService::new(&c, &g, &o);
        let paths = AuthorityPaths::default();
        let ops_vec = vec![op()];
        let a = render_viz_document(&c, &svc1, &ops_vec, &paths);
        let b = render_viz_document(&c, &svc2, &ops_vec, &paths);
        assert_eq!(a, b);
    }

    #[test]
    fn ident_sanitisation_replaces_dots_and_non_alnum() {
        assert_eq!(erd_ident("cardholder.cpf_Client_Profile"), "cardholder_cpf_Client_Profile");
        assert_eq!(flowchart_ident("post-cardholders"), "post_cardholders");
        assert_eq!(flowchart_ident("GET /v1/foo"), "GET__v1_foo");
    }
}
