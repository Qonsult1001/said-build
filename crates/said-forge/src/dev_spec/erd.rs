//! ERD derivation. Walk parsed endpoints, harvest entities from request
//! and response bodies, infer relationships from nested object structure.
//!
//! Design choices (per the design doc):
//!   - Top-level request body of a POST/PUT becomes a candidate root
//!     entity named after the URL's last collection segment in
//!     PascalCase. POST `/cardholders` body → entity `Cardholder`.
//!   - Nested object fields → child entities with FK back to parent.
//!   - Array fields → 1:N child entity (collection) with FK to parent.
//!   - Scalar `*Id` fields with UUID format → FK references inferred
//!     from the field name (`businessId` → references `Business`).

use crate::dev_spec::types::{Column, DevSpecEndpoint, DevSpecSchema, Entity, Erd, ForeignKey};
use std::collections::BTreeMap;

pub fn derive_erd(endpoints: &[DevSpecEndpoint]) -> Erd {
    let mut entities: BTreeMap<String, Entity> = BTreeMap::new();
    for ep in endpoints {
        if let Some(body) = &ep.request_body {
            // Special case: `/<collection>/{<x>_id}/transitions` → child
            // entity `<Singular(<collection>)>_Transition` bound to parent.
            if let Some((parent_name, child_name)) = transition_entity_for_path(&ep.path) {
                harvest_entity(&child_name, body, Some(&parent_name), &ep.source_file, &mut entities);
                // Ensure parent entity exists as a root so the FK has a target.
                entities.entry(parent_name.clone()).or_insert_with(|| Entity {
                    name: parent_name.clone(),
                    columns: vec![Column {
                        name: "Id".into(),
                        ty: "uuid".into(),
                        nullable: false,
                        description: Some(format!("Primary key for {parent_name}")),
                    }],
                    primary_key: "Id".into(),
                    foreign_keys: vec![],
                    introduced_by: vec![ep.source_file.clone()],
                });
                continue;
            }
            let root_name = root_entity_name(&ep.path);
            harvest_entity(&root_name, body, None, &ep.source_file, &mut entities);
        }
    }
    Erd {
        entities,
        endpoints: endpoints.to_vec(),
    }
}

/// Detects `/<collection>/{<x>_id}/transitions`. Returns
/// `(ParentEntity, ChildEntity)` when matched, else None.
fn transition_entity_for_path(path: &str) -> Option<(String, String)> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() != 3 {
        return None;
    }
    let (collection, param, sub) = (segs[0], segs[1], segs[2]);
    if !param.starts_with('{') || !param.ends_with('}') {
        return None;
    }
    if sub != "transitions" {
        return None;
    }
    let parent = pascal_singular(collection);
    let child = format!("{}_Transition", parent);
    Some((parent, child))
}

fn root_entity_name(path: &str) -> String {
    // Last non-template segment, pascalised, singular.
    let last = path
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .last()
        .unwrap_or("Entity");
    pascal_singular(last)
}

fn pascal_singular(word: &str) -> String {
    let pascal: String = word
        .split(|c: char| c == '-' || c == '_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect();
    // Naive de-pluraliser. Good enough for the contract names.
    if let Some(stem) = pascal.strip_suffix("ies") {
        format!("{stem}y")
    } else if pascal.ends_with('s') && pascal.len() > 1 {
        pascal[..pascal.len() - 1].to_string()
    } else {
        pascal
    }
}

fn harvest_entity(
    name: &str,
    schema: &DevSpecSchema,
    parent: Option<&str>,
    source: &str,
    out: &mut BTreeMap<String, Entity>,
) {
    if schema.ty != "object" {
        return;
    }
    let mut columns: Vec<Column> = Vec::new();
    let mut fks: Vec<ForeignKey> = Vec::new();
    let mut pk = "Id".to_string();
    if let Some(parent_name) = parent {
        // Child entities get a synthetic PK and an FK back to parent.
        pk = format!("{name}Id");
        columns.push(Column {
            name: pk.clone(),
            ty: "uuid".into(),
            nullable: false,
            description: Some(format!("Primary key for {name}")),
        });
        let parent_fk_col = format!("{parent_name}Id");
        columns.push(Column {
            name: parent_fk_col.clone(),
            ty: "uuid".into(),
            nullable: false,
            description: Some(format!("Foreign key to {parent_name}")),
        });
        fks.push(ForeignKey {
            column: parent_fk_col,
            references_entity: parent_name.to_string(),
            references_column: format!("{parent_name}Id"),
        });
    }
    // Helper: only push a column if no existing column collides
    // (case-insensitive). SQL Server treats column names case-
    // insensitively so `[AcquirerId]` and `[acquirerId]` both fail
    // CREATE TABLE if they appear in the same column list. This dedup
    // also catches the synthetic-PK vs same-named-scalar collision
    // for child entities (synthetic `AcquirerId` PK + a body field
    // `acquirerId` from the same Dev Spec).
    let push_unique = |columns: &mut Vec<Column>, col: Column| {
        if !columns.iter().any(|c| c.name.eq_ignore_ascii_case(&col.name)) {
            columns.push(col);
        }
    };
    let push_unique_fk = |fks: &mut Vec<ForeignKey>, fk: ForeignKey| {
        if !fks.iter().any(|f| f.column.eq_ignore_ascii_case(&fk.column)) {
            fks.push(fk);
        }
    };
    for (field_name, field_schema) in &schema.properties {
        let pascal_name = pascal_field(field_name);
        match field_schema.ty.as_str() {
            "object" => {
                // Nested entity. Recurse.
                harvest_entity(&pascal_name, field_schema, Some(name), source, out);
                // Parent gets an FK column.
                let fk_col = format!("{pascal_name}Id");
                let ref_col = format!("{pascal_name}Id");
                push_unique(&mut columns, Column {
                    name: fk_col.clone(),
                    ty: "uuid".into(),
                    nullable: true,
                    description: field_schema.description.clone(),
                });
                push_unique_fk(&mut fks, ForeignKey {
                    column: fk_col,
                    references_entity: pascal_name,
                    references_column: ref_col,
                });
            }
            "array" => {
                if let Some(item) = &field_schema.items {
                    if item.ty == "object" {
                        let child_name = pascal_singular(&pascal_name);
                        harvest_entity(&child_name, item, Some(name), source, out);
                    }
                }
            }
            "string" | "integer" | "boolean" | "number" => {
                let mut ty = match (field_schema.ty.as_str(), field_schema.format.as_deref()) {
                    ("string", Some("uuid")) => "uuid".to_string(),
                    ("string", Some("date-time")) => "datetime".to_string(),
                    ("string", _) => "string(255)".to_string(),
                    ("integer", _) => "int".to_string(),
                    ("boolean", _) => "bit".to_string(),
                    ("number", _) => "decimal(18,4)".to_string(),
                    _ => "string(255)".to_string(),
                };
                // FK columns (`*Id`) reference UUID PKs — force the
                // type to `uuid` even when Dev Spec didn't annotate
                // `Format: uuid`. Without this, the FK constraint
                // fails at deploy with a type-mismatch error.
                if pascal_name.ends_with("Id") && pascal_name != "Id" {
                    ty = "uuid".to_string();
                }
                push_unique(&mut columns, Column {
                    name: pascal_name.clone(),
                    ty,
                    nullable: pascal_name != pk,
                    description: field_schema.description.clone(),
                });
                // Inferred FK from name pattern: `<x>Id` references `<X>`.
                if pascal_name.ends_with("Id")
                    && pascal_name != pk
                    && pascal_name != "Id"
                {
                    let referenced = pascal_name.trim_end_matches("Id").to_string();
                    if !referenced.is_empty() && referenced != name {
                        push_unique_fk(&mut fks, ForeignKey {
                            column: pascal_name.clone(),
                            references_entity: referenced.clone(),
                            references_column: format!("{referenced}Id"),
                        });
                    }
                }
            }
            _ => {
                // Unknown type — store as nullable string for later
                // human review.
                push_unique(&mut columns, Column {
                    name: pascal_name.clone(),
                    ty: "string(255)".into(),
                    nullable: true,
                    description: field_schema.description.clone(),
                });
            }
        }
    }
    // Add Id column for root entities (when not already added as PK).
    if parent.is_none() && !columns.iter().any(|c| c.name == pk) {
        columns.insert(
            0,
            Column {
                name: pk.clone(),
                ty: "uuid".into(),
                nullable: false,
                description: Some(format!("Primary key for {name}")),
            },
        );
    }

    out.entry(name.to_string())
        .and_modify(|e| {
            // Merge: union of columns by name. Existing wins on conflict.
            // Comparison is case-insensitive because SQL Server treats
            // column identifiers case-insensitively, so e.g. `UserName`
            // and `Username` collide at deploy time. `pascal_field`
            // already canonicalises separator-style differences; this
            // catches the residual case-only collisions (e.g. `userName`
            // -> `UserName` vs `username` -> `Username`, which the
            // normaliser cannot disambiguate without a dictionary).
            for col in &columns {
                if !e.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&col.name)) {
                    e.columns.push(col.clone());
                }
            }
            for fk in &fks {
                if !e.foreign_keys.iter().any(|f| f.column.eq_ignore_ascii_case(&fk.column)) {
                    e.foreign_keys.push(fk.clone());
                }
            }
            e.introduced_by.push(source.to_string());
        })
        .or_insert(Entity {
            name: name.to_string(),
            columns,
            primary_key: pk,
            foreign_keys: fks,
            introduced_by: vec![source.to_string()],
        });
}

/// Convert any field-name convention (camelCase, snake_case, kebab-case,
/// SCREAMING_SNAKE) to canonical PascalCase. Two inputs that differ
/// only in case/separator style normalise to the same output, so two
/// endpoints contributing `userName` and `Username` produce the same
/// column and dedup correctly in `harvest_entity`.
pub fn pascal_field(name: &str) -> String {
    // Split on word boundaries: underscores, hyphens, and case
    // transitions (camelCase -> camel + Case).
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c == '_' || c == '-' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if c.is_ascii_uppercase() && prev_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(c);
        prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
    }
    if !current.is_empty() {
        words.push(current);
    }
    // Lowercase each word, capitalise first letter.
    words
        .into_iter()
        .filter(|w| !w.is_empty())
        .map(|w| {
            let lower = w.to_ascii_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Canonical JSON serialiser. Pretty-printed (2-space) with sorted
/// keys (BTreeMap fields already are; we use serde_json's default
/// which preserves their order).
pub fn to_canonical_json(erd: &Erd) -> Result<String, String> {
    serde_json::to_string_pretty(erd).map_err(|e| format!("serialize erd: {e}"))
}

/// Render the ERD as a Mermaid `erDiagram` fenced block. Suitable for
/// inclusion in markdown reports.
pub fn render_mermaid(erd: &Erd) -> String {
    let mut out = String::from("```mermaid\nerDiagram\n");

    // Pass 1: relationships emitted BEFORE entity blocks.
    // Domain-ERD aesthetic: parent ||--o{ child : "label".
    for entity in erd.entities.values() {
        for fk in &entity.foreign_keys {
            let label = relationship_label(&entity.name, &fk.references_entity);
            out.push_str(&format!(
                "    {} ||--o{{ {} : \"{}\"\n",
                upper_snake(&fk.references_entity),
                upper_snake(&entity.name),
                label,
            ));
        }
    }

    // Blank line between sections.
    out.push('\n');

    // Pass 2: entity column blocks.
    for entity in erd.entities.values() {
        out.push_str(&format!("    {} {{\n", upper_snake(&entity.name)));
        for col in &entity.columns {
            let pk_marker = if col.name == entity.primary_key { " PK" } else { "" };
            let fk_marker = if entity.foreign_keys.iter().any(|fk| fk.column == col.name) {
                " FK"
            } else {
                ""
            };
            let mermaid_type = sql_type_to_mermaid(&col.ty);
            out.push_str(&format!(
                "        {} {}{}{}\n",
                mermaid_type, col.name, pk_marker, fk_marker
            ));
        }
        out.push_str("    }\n");
    }

    out.push_str("```\n");
    out
}

/// PascalCase entity → UPPER_SNAKE display name.
/// `Cardholder` → `CARDHOLDER`
/// `BinSponsor` → `BIN_SPONSOR`
/// `Cardholder_Transition` → `CARDHOLDER_TRANSITION`
fn upper_snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c == '_' {
            out.push('_');
            continue;
        }
        if c.is_ascii_uppercase() && i > 0 {
            // Insert separator only when previous character was lowercase
            // (camel boundary), not right after we just emitted '_'.
            if !out.ends_with('_') {
                out.push('_');
            }
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// Relationship label rule: lifecycle when the owning (child) entity is a
/// transition entity (`<Parent>_Transition`), `has` otherwise.
fn relationship_label(owning_entity: &str, _references_entity: &str) -> &'static str {
    if owning_entity.ends_with("Transition") || owning_entity.contains("_Transition") {
        "lifecycle"
    } else {
        "has"
    }
}

fn sql_type_to_mermaid(ty: &str) -> &str {
    match ty {
        "uuid" => "uniqueidentifier",
        "datetime" => "datetime",
        "int" => "int",
        "bit" => "bit",
        t if t.starts_with("string") => "nvarchar",
        t if t.starts_with("decimal") => "decimal",
        _ => "string",
    }
}
