//! Database-first API generator — walks the SqlCatalog and emits a
//! complete OpenAPI 3.1 spec.
//!
//! Forge treats SQL as the only authoritative description of the
//! system. Every stored procedure shaped like `p_txn_<Verb>_<Entity>`
//! is one API operation; its `OPENJSON` body extraction defines the
//! request shape; its NOT NULL columns + the FK graph + lookup seed
//! data define what's required and what's enumerable; its
//! `RAISERROR` + `prc_Code` calls define the error catalogue.
//!
//! The output is `5-deliverables/api-specification.generated.yml` —
//! a self-contained OpenAPI 3.1 document the client can compare
//! against their wishlist (whatever shape it lives in).
//!
//! Verb mapping (locked convention — documented in the generated
//! `info.description`):
//!
//! | Proc name pattern              | HTTP verb | Path shape                              |
//! |--------------------------------|-----------|-----------------------------------------|
//! | `p_txn_Create_<Entity>`        | POST      | `/<entity>`                             |
//! | `p_txn_Update_<Entity>`        | PUT       | `/<entity>` (full replacement)          |
//! | `p_txn_Update_<Entity>_Status` | POST      | `/<entity>/{id}/transitions`            |
//! | `p_txn_Get_All_<Entity>_Details` | GET     | `/<entity>` (list)                      |
//! | `p_txn_Get_<Entity>_Details`   | GET       | `/<entity>/{id}`                        |
//! | `p_txn_Get_<Entity>_Cards`     | GET       | `/<entity>/{id}/cards`                  |
//! | `p_txn_Get_<Entity>_All_Account_Details` | GET | `/<entity>/{id}/accounts`           |
//! | `p_txn_Get_<Entity>_Transitions` | GET     | `/<entity>/{id}/transitions`            |
//! | `p_txn_Get_<Entity>_Transition`  | GET     | `/<entity>/transitions/{id}`            |
//!
//! Procs that don't match any pattern get a `paths./internal/<name>`
//! entry tagged `internal: true` — they're surfaced for review but
//! not treated as public API.

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::path::Path;

use crate::comparison::{build_error_code_index, ErrorCodeIndex};
use crate::proc_analysis::{analyse_proc, JsonKey, ProcAnalysis};
use crate::sql_catalog::{SqlCatalog, SqlKind, SqlObject};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedApi {
    pub epic: String,
    pub op_count: usize,
    pub orphan_proc_count: usize,
    /// The full OpenAPI document as YAML text.
    pub yaml: String,
}

/// Emit an OpenAPI 3.1 spec covering every procedure in `sql_schema`.
/// Per-lookup-table closed-enum values supplied by the optional
/// sandbox verification pass. Key is the full lookup table name
/// (e.g. `lookups.cps_Cardholder_Profile_Status`), value is the list
/// of allowed codes. Only tables with ≤ MAX_ENUM_ROWS are passed
/// through; larger tables stay open in the spec.
pub type EnumOverrides = BTreeMap<String, Vec<String>>;

pub fn generate_openapi(
    epic: &str,
    sql_schema: &str,
    catalog: &SqlCatalog,
    workspace_root: &Path,
    enum_overrides: Option<&EnumOverrides>,
) -> GeneratedApi {
    let err_index = build_error_code_index(workspace_root);
    let empty_enums: EnumOverrides = BTreeMap::new();
    let enum_overrides = enum_overrides.unwrap_or(&empty_enums);

    // Schema scope: empty `sql_schema` ("" or "*") = walk every
    // schema the catalog has — used by clients without a curated
    // module slice (e.g. Vivere). Otherwise restrict to the named
    // schema (legacy single-module behavior, e.g. "cardholder").
    let all_schemas = sql_schema.is_empty() || sql_schema == "*";
    let in_scope_procs: Vec<&SqlObject> = catalog
        .objects
        .iter()
        .filter(|o| o.kind == SqlKind::Procedure)
        .filter(|o| {
            if all_schemas {
                return true;
            }
            o.schema
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case(sql_schema))
                .unwrap_or(false)
        })
        .collect();

    let mut paths: Vec<(String, Value)> = Vec::new();
    let mut orphan_count = 0usize;
    let mut op_count = 0usize;

    for proc in &in_scope_procs {
        let analysis = analyse_proc(proc, workspace_root);
        // Re-read the proc body so we can run the lookup-hint scanner.
        // analyse_proc already opens this file; the second read is
        // negligible (procs are tens of KB max).
        let proc_body = std::fs::read_to_string(workspace_root.join(&proc.source_path))
            .unwrap_or_default();
        let lookup_hints = analyse_lookup_hints(&proc.full_name(), &proc_body, catalog);
        match map_verb(&proc.name) {
            Some(map) => {
                op_count += 1;
                let path = compose_path(&map, sql_schema);
                let path_item = build_path_item(
                    &map,
                    &analysis,
                    proc,
                    catalog,
                    &lookup_hints,
                    enum_overrides,
                    &err_index,
                );
                merge_path(&mut paths, &path, &map.method, path_item);
            }
            None => {
                orphan_count += 1;
                let path = format!("/internal/{}", proc.name);
                let path_item = build_internal_item(&analysis, proc, &err_index);
                merge_path(&mut paths, &path, "x-internal", path_item);
            }
        }
    }

    // Sort paths alphabetically for deterministic output.
    paths.sort_by(|a, b| a.0.cmp(&b.0));

    // Merge operations per path. Multiple `paths` entries with the same
    // path key carry different methods (e.g. `/cardholders` POST + GET).
    // A naive `insert` would overwrite, dropping all but the last verb.
    // Walk the list and merge the method maps for each path key.
    let mut paths_map = Mapping::new();
    for (k, v) in paths {
        let key = Value::String(k);
        match (paths_map.get_mut(&key), v) {
            (Some(Value::Mapping(existing_map)), Value::Mapping(new_map)) => {
                for (mk, mv) in new_map {
                    existing_map.insert(mk, mv);
                }
            }
            (_, v) => {
                paths_map.insert(key, v);
            }
        }
    }

    let mut info = Mapping::new();
    info.insert(
        Value::String("title".into()),
        Value::String(format!("{} (Generated from SQL)", epic)),
    );
    info.insert(Value::String("version".into()), Value::String("0.1.0".into()));
    info.insert(
        Value::String("description".into()),
        Value::String(info_description()),
    );

    let mut doc = Mapping::new();
    doc.insert(Value::String("openapi".into()), Value::String("3.1.0".into()));
    doc.insert(Value::String("info".into()), Value::Mapping(info));
    doc.insert(
        Value::String("components".into()),
        Value::Mapping(common_components(&err_index)),
    );
    doc.insert(Value::String("paths".into()), Value::Mapping(paths_map));

    let yaml = serde_yaml::to_string(&Value::Mapping(doc)).unwrap_or_default();

    GeneratedApi {
        epic: epic.to_string(),
        op_count,
        orphan_proc_count: orphan_count,
        yaml,
    }
}

fn info_description() -> String {
    "Auto-generated by forge from the SQL catalog. SQL is the only source of \
     truth used by this generator — every operation, parameter, request body \
     field, and response field below was derived from a stored procedure \
     body or a table definition under `1-ground-truth/`. Lookup-validated \
     fields carry a runtime hint (`x-validated-by`) pointing at the lookup \
     table the proc validates against; the spec does NOT enumerate values \
     because lookup contents are runtime-mutable, not version-controlled \
     here. Header parameters and content-types invented by REST convention \
     (e.g. `x-api-version`, `Content-Type`) are deliberately omitted — \
     forge only documents what the proc signature declares. The 'wishlist' \
     OpenAPI spec in `4-expectations/` should be diffed against this \
     document; discrepancies indicate either a missing implementation or \
     an incorrectly-documented behaviour."
        .into()
}

// ─────────────────────────── verb mapping ───────────────────────────

#[derive(Debug, Clone)]
struct VerbMap {
    method: String,
    /// `entity-singular` token for the path (e.g. "cardholder").
    entity_path: String,
    /// Optional sub-resource segment ("transitions", "cards", "accounts").
    sub_resource: Option<String>,
    /// Whether the route ends in an id parameter.
    has_id: bool,
    /// Whether the id parameter is for the sub-resource (e.g. transitionId)
    /// vs the entity (cardholderId).
    sub_id: bool,
    /// Stored proc full name for the description.
    summary_hint: String,
}

/// Translate a stored-proc name into its HTTP verb + path shape.
/// Returns None when the name doesn't match the convention — the
/// caller treats those as internal helpers and routes them under
/// `/internal/<name>`.
fn map_verb(proc_name: &str) -> Option<VerbMap> {
    let lower = proc_name.to_ascii_lowercase();
    // Strip the standard prefix.
    let rest = lower
        .strip_prefix("p_txn_")
        .or_else(|| lower.strip_prefix("p_dte_"))
        .unwrap_or(&lower);

    let tokens: Vec<&str> = rest.split('_').collect();
    if tokens.is_empty() {
        return None;
    }

    let summary_hint = format!("From `{}`", proc_name);

    // Update_<Entity>_Status → POST /<entity>/{id}/transitions
    if let Some(_pos) = find_subseq(&tokens, &["update"]) {
        if tokens.last() == Some(&"status") {
            let entity = entity_from_tokens(&tokens, &["update"], &["status"]);
            if let Some(entity) = entity {
                return Some(VerbMap {
                    method: "post".into(),
                    entity_path: entity,
                    sub_resource: Some("transitions".into()),
                    has_id: true,
                    sub_id: false,
                    summary_hint,
                });
            }
        }
        let entity = entity_from_tokens(&tokens, &["update"], &[]);
        if let Some(entity) = entity {
            return Some(VerbMap {
                method: "put".into(),
                entity_path: entity,
                sub_resource: None,
                has_id: false,
                sub_id: false,
                summary_hint,
            });
        }
    }

    if find_subseq(&tokens, &["create"]).is_some() {
        // Create_<Entity>_Transition → POST /<entity>/{id}/transitions
        if tokens.last() == Some(&"transition") {
            let entity = entity_from_tokens(&tokens, &["create"], &["transition"]);
            if let Some(entity) = entity {
                return Some(VerbMap {
                    method: "post".into(),
                    entity_path: entity,
                    sub_resource: Some("transitions".into()),
                    has_id: true,
                    sub_id: false,
                    summary_hint,
                });
            }
        }
        let entity = entity_from_tokens(&tokens, &["create"], &[]);
        if let Some(entity) = entity {
            return Some(VerbMap {
                method: "post".into(),
                entity_path: entity,
                sub_resource: None,
                has_id: false,
                sub_id: false,
                summary_hint,
            });
        }
    }

    // Get_*
    if find_subseq(&tokens, &["get"]).is_some() {
        // Get_All_<Entity>_Details → GET /<entity> (list)
        if tokens.contains(&"all") && tokens.last() == Some(&"details") {
            // "Get_All_<Entity>_Details" or "Get_<Entity>_All_Account_Details"
            if let Some(idx) = tokens.iter().position(|t| *t == "all") {
                if tokens.get(idx + 1) == Some(&"account") {
                    // Get_<Entity>_All_Account_Details
                    let entity = first_entity_token(&tokens);
                    if let Some(entity) = entity {
                        return Some(VerbMap {
                            method: "get".into(),
                            entity_path: entity,
                            sub_resource: Some("accounts".into()),
                            has_id: true,
                            sub_id: false,
                            summary_hint,
                        });
                    }
                }
            }
            let entity = entity_from_tokens(&tokens, &["get", "all"], &["details"]);
            if let Some(entity) = entity {
                return Some(VerbMap {
                    method: "get".into(),
                    entity_path: entity,
                    sub_resource: None,
                    has_id: false,
                    sub_id: false,
                    summary_hint,
                });
            }
        }
        // Get_<Entity>_Cards → GET /<entity>/{id}/cards
        if tokens.last() == Some(&"cards") {
            let entity = entity_from_tokens(&tokens, &["get"], &["cards"]);
            if let Some(entity) = entity {
                return Some(VerbMap {
                    method: "get".into(),
                    entity_path: entity,
                    sub_resource: Some("cards".into()),
                    has_id: true,
                    sub_id: false,
                    summary_hint,
                });
            }
        }
        // Get_<Entity>_Transitions → GET /<entity>/{id}/transitions (list)
        if tokens.last() == Some(&"transitions") {
            let entity = entity_from_tokens(&tokens, &["get"], &["transitions"]);
            if let Some(entity) = entity {
                return Some(VerbMap {
                    method: "get".into(),
                    entity_path: entity,
                    sub_resource: Some("transitions".into()),
                    has_id: true,
                    sub_id: false,
                    summary_hint,
                });
            }
        }
        // Get_<Entity>_Transition → GET /<entity>/transitions/{transitionId}
        if tokens.last() == Some(&"transition") {
            let entity = entity_from_tokens(&tokens, &["get"], &["transition"]);
            if let Some(entity) = entity {
                return Some(VerbMap {
                    method: "get".into(),
                    entity_path: entity,
                    sub_resource: Some("transitions".into()),
                    has_id: true,
                    sub_id: true,
                    summary_hint,
                });
            }
        }
        // Get_<Entity>_Details → GET /<entity>/{id}
        if tokens.last() == Some(&"details") {
            let entity = entity_from_tokens(&tokens, &["get"], &["details"]);
            if let Some(entity) = entity {
                return Some(VerbMap {
                    method: "get".into(),
                    entity_path: entity,
                    sub_resource: None,
                    has_id: true,
                    sub_id: false,
                    summary_hint,
                });
            }
        }
        // Get_<Entity> → GET /<entity>
        let entity = entity_from_tokens(&tokens, &["get"], &[]);
        if let Some(entity) = entity {
            return Some(VerbMap {
                method: "get".into(),
                entity_path: entity,
                sub_resource: None,
                has_id: false,
                sub_id: false,
                summary_hint,
            });
        }
    }

    None
}

fn find_subseq(tokens: &[&str], needle: &[&str]) -> Option<usize> {
    if needle.is_empty() || tokens.is_empty() {
        return None;
    }
    tokens
        .windows(needle.len())
        .position(|w| w.iter().zip(needle).all(|(a, b)| a == b))
}

fn entity_from_tokens(
    tokens: &[&str],
    leading: &[&str],
    trailing: &[&str],
) -> Option<String> {
    let lead_len = leading.len();
    let trail_len = trailing.len();
    if tokens.len() < lead_len + trail_len + 1 {
        return None;
    }
    let middle = &tokens[lead_len..tokens.len() - trail_len];
    if middle.is_empty() {
        return None;
    }
    // Skip a leading "client" word ("Client_Profile" → "client-profile").
    let joined = middle.join("-");
    Some(joined)
}

fn first_entity_token(tokens: &[&str]) -> Option<String> {
    tokens.iter().find(|t| **t != "get" && **t != "all").map(|s| s.to_string())
}

fn compose_path(map: &VerbMap, sql_schema: &str) -> String {
    // First-cut path emission. The fitter pass (when
    // `--verify-against-sandbox` is on) overwrites these from the
    // database's API registry (`lookups.ars_Api_Rule_Settings`), so
    // any singular/plural mismatch with the registry gets corrected
    // automatically. Here we just keep the proc-name's casing.
    let entity_root = if map.entity_path.is_empty() {
        sql_schema.to_string()
    } else {
        singularise(&map.entity_path)
    };
    let id_segment = if map.has_id {
        if map.sub_id {
            "/{transitionId}".to_string()
        } else {
            "/{id}".to_string()
        }
    } else {
        String::new()
    };
    match (&map.sub_resource, map.has_id, map.sub_id) {
        (Some(sub), true, true) => format!("/{}/{}/{}", entity_root, sub, "{transitionId}"),
        (Some(sub), true, false) => format!("/{}/{{id}}/{}", entity_root, sub),
        (Some(sub), false, _) => format!("/{}/{}", entity_root, sub),
        (None, _, _) => format!("/{}{}", entity_root, id_segment),
    }
}

fn singularise(s: &str) -> String {
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

// ─────────────────────────── path item / operation ───────────────────────────

fn build_path_item(
    map: &VerbMap,
    analysis: &ProcAnalysis,
    proc: &SqlObject,
    catalog: &SqlCatalog,
    lookups: &BTreeMap<String, LookupHint>,
    enum_overrides: &EnumOverrides,
    err_index: &ErrorCodeIndex,
) -> Value {
    let mut op = Mapping::new();
    op.insert(
        Value::String("summary".into()),
        Value::String(format!("{} ({})", verb_summary(&map.method), map.summary_hint)),
    );
    op.insert(
        Value::String("description".into()),
        Value::String(describe_op_semantics(analysis, &map.method)),
    );
    op.insert(
        Value::String("operationId".into()),
        Value::String(operation_id(map, &proc.name)),
    );
    // Tag: prefer the path's entity segment (e.g. `/cardholder/{id}`
    // → `Cardholder`, `/card-pin-change-request` → `CardPinChangeRequest`).
    // Falls back to schema-name when the path is `/internal/...` or
    // doesn't have a recognisable entity. This gives a per-entity
    // grouping in tools like Swagger UI rather than dumping every op
    // under a single `Dbo` tag.
    op.insert(
        Value::String("tags".into()),
        Value::Sequence(vec![Value::String(tag_for_op(
            map,
            proc.schema.as_deref().unwrap_or("default"),
        ))]),
    );

    // Parameters: id (path), x-api-version + RequestId + CorrelationId
    // (refs to the components.parameters block).
    let params = build_parameters(map);
    if !params.is_empty() {
        op.insert(Value::String("parameters".into()), Value::Sequence(params));
    }

    // Body: only for verbs that take one (POST/PUT/PATCH).
    if matches!(map.method.as_str(), "post" | "put" | "patch")
        && !analysis.json_keys.is_empty()
    {
        op.insert(
            Value::String("requestBody".into()),
            build_request_body(analysis, proc, catalog, lookups, enum_overrides),
        );
    }

    op.insert(
        Value::String("responses".into()),
        build_responses(analysis, err_index),
    );

    let mut item = Mapping::new();
    item.insert(Value::String(map.method.clone()), Value::Mapping(op));
    Value::Mapping(item)
}

fn verb_summary(method: &str) -> &'static str {
    match method {
        "post" => "Create",
        "put" => "Update (full replacement)",
        "patch" => "Patch (partial update)",
        "get" => "Retrieve",
        "delete" => "Delete",
        _ => "Operation",
    }
}

fn describe_op_semantics(a: &ProcAnalysis, method: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("Implemented by `{}`.", a.full_name));
    if !a.destructive_updates.is_empty() && method == "put" {
        let tables: Vec<String> = a
            .destructive_updates
            .iter()
            .map(|d| format!("`{}`", d.target_table))
            .collect();
        parts.push(format!(
            "Full-replacement semantics: missing body fields write NULL into {}. \
             The client must send the complete object on every call.",
            tables.join(", ")
        ));
    }
    if !a.conditional_deletes.is_empty() {
        let tables: Vec<String> = a
            .conditional_deletes
            .iter()
            .map(|c| format!("`{}` (when `{}` absent)", c.target_table, c.guarded_by))
            .collect();
        parts.push(format!(
            "⚠ Destructive on missing fields — DELETEs rows from {}.",
            tables.join(", ")
        ));
    }
    parts.join(" ")
}

fn operation_id(map: &VerbMap, proc_name: &str) -> String {
    let mut id = String::new();
    id.push_str(&map.method);
    let cleaned: String = proc_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    id.push('_');
    id.push_str(&cleaned);
    id
}

fn tag_for_schema(schema: &str) -> String {
    let cap: String = schema
        .chars()
        .enumerate()
        .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
        .collect();
    cap
}

/// Derive a per-entity tag from the op's `entity_path` (e.g.
/// `cardholder` → `Cardholder`, `card-pin-change-request` →
/// `CardPinChangeRequest`). Falls back to the SQL schema name when
/// the entity_path is empty.
fn tag_for_op(map: &VerbMap, schema: &str) -> String {
    let entity = if map.entity_path.is_empty() {
        return tag_for_schema(schema);
    } else {
        map.entity_path.as_str()
    };
    entity
        .split('-')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn build_parameters(map: &VerbMap) -> Vec<Value> {
    let mut out = Vec::new();
    // Only emit headers that exist as proc parameters (`@uRequestId`,
    // `@uCorrelationId`). `x-api-version` and `Content-Type` are
    // controller / framework concerns, not SQL-derivable, so we
    // deliberately omit them.
    out.push(ref_param("RequestId"));
    out.push(ref_param("CorrelationId"));
    if map.has_id {
        let name = if map.sub_id { "transitionId" } else { "id" };
        let mut p = Mapping::new();
        p.insert(Value::String("name".into()), Value::String(name.into()));
        p.insert(Value::String("in".into()), Value::String("path".into()));
        p.insert(Value::String("required".into()), Value::Bool(true));
        let mut sch = Mapping::new();
        sch.insert(Value::String("type".into()), Value::String("string".into()));
        sch.insert(Value::String("format".into()), Value::String("uuid".into()));
        p.insert(Value::String("schema".into()), Value::Mapping(sch));
        p.insert(
            Value::String("description".into()),
            Value::String(format!("UUID identifier for the {}", name)),
        );
        out.push(Value::Mapping(p));
    }
    out
}

fn ref_param(name: &str) -> Value {
    let mut m = Mapping::new();
    m.insert(
        Value::String("$ref".into()),
        Value::String(format!("#/components/parameters/{}", name)),
    );
    Value::Mapping(m)
}

fn build_request_body(
    a: &ProcAnalysis,
    proc: &SqlObject,
    catalog: &SqlCatalog,
    lookups: &BTreeMap<String, LookupHint>,
    enum_overrides: &EnumOverrides,
) -> Value {
    // Required fields = JSON keys whose source variable is mentioned
    // by a destructive UPDATE on a column that's NOT NULL no-default
    // in the catalog. Cheap approximation: any field whose path leads
    // to a NOT NULL column declared in the touched primary tables.
    let required = compute_required_fields(a, proc, catalog);

    let schema =
        build_object_schema_from_keys(&a.json_keys, &required, lookups, enum_overrides);

    let mut content_json = Mapping::new();
    content_json.insert(Value::String("schema".into()), schema);

    let mut content = Mapping::new();
    content.insert(
        Value::String("application/json".into()),
        Value::Mapping(content_json),
    );

    let mut body = Mapping::new();
    body.insert(Value::String("required".into()), Value::Bool(true));
    body.insert(Value::String("content".into()), Value::Mapping(content));
    body.insert(
        Value::String("description".into()),
        Value::String(format!(
            "Request body extracted from `{}` `OPENJSON` clause.",
            a.full_name
        )),
    );
    Value::Mapping(body)
}

fn compute_required_fields(
    a: &ProcAnalysis,
    proc: &SqlObject,
    catalog: &SqlCatalog,
) -> std::collections::BTreeSet<String> {
    // Start with the union of NOT NULL no-default columns across every
    // primary table the proc updates / inserts into. Then map each
    // column to its JSON path by matching the SET assignment's source
    // variable to a JSON key whose destination column is the same
    // (Hungarian-stripped name).
    let mut required: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let touched: Vec<String> = proc
        .ops
        .iter()
        .map(|to| to.table.clone())
        .collect();
    let mut not_null: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for t in &catalog.tables {
        let full = t.full_name();
        if !touched.iter().any(|x| x.eq_ignore_ascii_case(&full) || x.eq_ignore_ascii_case(&t.name))
        {
            continue;
        }
        for c in &t.columns {
            if !c.nullable && !c.has_default && !c.identity && !c.is_computed {
                not_null.insert(strip_hungarian(&c.name));
            }
        }
    }
    for k in &a.json_keys {
        let last = k
            .json_path
            .rsplit('.')
            .next()
            .unwrap_or("")
            .trim_start_matches('$');
        let normal = camel_to_snake(last);
        if not_null.contains(&normal) || not_null.contains(last) {
            required.insert(json_path_root(&k.json_path));
        }
    }
    required
}

fn json_path_root(p: &str) -> String {
    // For required-field computation we mark the top-level key. The
    // OpenAPI required[] list lives at the top of the body schema.
    let trimmed = p.trim_start_matches('$').trim_start_matches('.');
    trimmed
        .split('.')
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i != 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn strip_hungarian(name: &str) -> String {
    let mut parts = name.splitn(2, '_');
    let first = parts.next().unwrap_or("");
    let rest = parts.next();
    if rest.is_some()
        && (2..=4).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_lowercase())
    {
        rest.unwrap().to_ascii_lowercase()
    } else {
        name.to_ascii_lowercase()
    }
}

/// Build a JSON-Schema object from the list of OPENJSON keys. Nested
/// `$.foo.bar` paths build nested objects.
fn build_object_schema_from_keys(
    keys: &[JsonKey],
    required_top: &std::collections::BTreeSet<String>,
    lookups: &BTreeMap<String, LookupHint>,
    enum_overrides: &EnumOverrides,
) -> Value {
    // Group by top-level key. Each top-level field is either a leaf
    // type or an object whose properties come from nested paths.
    let mut groups: BTreeMap<String, Vec<&JsonKey>> = BTreeMap::new();
    for k in keys {
        let trimmed = k.json_path.trim_start_matches('$').trim_start_matches('.');
        let top = trimmed.split('.').next().unwrap_or(trimmed).to_string();
        groups.entry(top).or_default().push(k);
    }
    let mut props = Mapping::new();
    let mut required_list: Vec<Value> = Vec::new();
    for (top, items) in &groups {
        let value = if items.len() == 1 && items[0].json_path.matches('.').count() <= 1 {
            schema_for_json_key(items[0], top, lookups, enum_overrides)
        } else {
            // Nested object — collect each child's leaf path.
            build_nested_object(top, items, lookups, enum_overrides)
        };
        props.insert(Value::String(top.clone()), value);
        if required_top.contains(top) {
            required_list.push(Value::String(top.clone()));
        }
    }

    let mut schema = Mapping::new();
    schema.insert(Value::String("type".into()), Value::String("object".into()));
    schema.insert(Value::String("properties".into()), Value::Mapping(props));
    if !required_list.is_empty() {
        schema.insert(Value::String("required".into()), Value::Sequence(required_list));
    }
    Value::Mapping(schema)
}

fn build_nested_object(
    top: &str,
    items: &[&JsonKey],
    lookups: &BTreeMap<String, LookupHint>,
    enum_overrides: &EnumOverrides,
) -> Value {
    let mut nested_props = Mapping::new();
    for k in items {
        let trimmed = k.json_path.trim_start_matches('$').trim_start_matches('.');
        let mut comps = trimmed.split('.');
        let _ = comps.next(); // skip top
        let leaf = comps.collect::<Vec<_>>().join(".");
        if leaf.is_empty() {
            // Fallthrough for a top-level path that we mistakenly
            // grouped — treat the whole object as a leaf.
            return schema_for_json_key(k, top, lookups, enum_overrides);
        }
        let leaf_root = leaf.split('.').next().unwrap_or(&leaf).to_string();
        let nested_value = schema_for_json_key(k, &leaf_root, lookups, enum_overrides);
        nested_props.insert(Value::String(leaf_root), nested_value);
    }
    let mut schema = Mapping::new();
    schema.insert(Value::String("type".into()), Value::String("object".into()));
    schema.insert(Value::String("properties".into()), Value::Mapping(nested_props));
    Value::Mapping(schema)
}

/// Render a JSON Schema fragment from a JsonKey.
///
/// - `is_array_iteration` → `type: array, items: { type: string, format: uuid }`
/// - `is_json` → `type: object, additionalProperties: true`
/// - otherwise → fall through to `schema_for_sql_type`.
fn schema_for_json_key(
    key: &JsonKey,
    field_name: &str,
    lookups: &BTreeMap<String, LookupHint>,
    enum_overrides: &EnumOverrides,
) -> Value {
    if key.is_array_iteration {
        // The proc iterates `OPENJSON(@jRequest, '$.<path>')`. We
        // assume the most common shape: an array of UUID strings.
        // Without seeing the loop body's TRY_CONVERT we can't be
        // sure; UUID is the common case in this codebase.
        let mut items = Mapping::new();
        items.insert(Value::String("type".into()), Value::String("string".into()));
        items.insert(Value::String("format".into()), Value::String("uuid".into()));
        let mut s = Mapping::new();
        s.insert(Value::String("type".into()), Value::String("array".into()));
        s.insert(Value::String("items".into()), Value::Mapping(items));
        s.insert(
            Value::String("description".into()),
            Value::String(format!(
                "Array of identifiers iterated by `OPENJSON(@jRequest, '{}')` in the proc.",
                key.json_path
            )),
        );
        return Value::Mapping(s);
    }
    if key.is_json {
        // The WITH clause flagged this as `AS JSON` — payload is a
        // structured object/array, not a string.
        let mut s = Mapping::new();
        s.insert(Value::String("type".into()), Value::String("object".into()));
        s.insert(
            Value::String("additionalProperties".into()),
            Value::Bool(true),
        );
        s.insert(
            Value::String("description".into()),
            Value::String(
                "Open-shape JSON object (proc reads via `AS JSON` modifier — \
                 stored as `NVARCHAR(MAX)` but treated as structured JSON)."
                    .into(),
            ),
        );
        return Value::Mapping(s);
    }
    schema_for_sql_type(&key.sql_type, field_name, lookups, enum_overrides)
}

fn schema_for_sql_type(
    sql_type: &str,
    field_name: &str,
    lookups: &BTreeMap<String, LookupHint>,
    enum_overrides: &EnumOverrides,
) -> Value {
    let upper = sql_type.to_ascii_uppercase();
    let base_type = upper
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or("")
        .to_string();
    let mut s = Mapping::new();

    // Lookup-validated field → emit `description` with the format
    // hint and `x-validated-by` pointing at the lookup table. The
    // OpenAPI type stays as `string` (or whatever the SQL type maps
    // to below) — we DO NOT enumerate values, because the lookup
    // table is mutable at runtime in this codebase. The client
    // gets format guidance + the table name to consult; runtime
    // validation is the proc's job.
    if let Some(hint) = lookups.get(field_name) {
        s.insert(Value::String("type".into()), Value::String("string".into()));
        s.insert(
            Value::String("description".into()),
            Value::String(hint.format_hint.clone()),
        );
        s.insert(
            Value::String("x-validated-by".into()),
            Value::String(hint.validated_by.clone()),
        );
        s.insert(
            Value::String("x-matched-column".into()),
            Value::String(hint.matched_column.clone()),
        );
        // Closed-set enum from sandbox verification — present only
        // when the verify pass found a small lookup table (≤25 rows)
        // and pulled its values. We attach the enum AND keep the
        // x-validated-by hint so consumers know it's runtime-mutable
        // truth, captured at this snapshot.
        if let Some(values) = enum_overrides.get(&hint.validated_by) {
            if !values.is_empty() {
                s.insert(
                    Value::String("enum".into()),
                    Value::Sequence(
                        values.iter().cloned().map(Value::String).collect(),
                    ),
                );
                s.insert(
                    Value::String("x-enum-source".into()),
                    Value::String(format!(
                        "Verified live in sandbox (`{}` rows fetched)",
                        values.len()
                    )),
                );
            }
        }
        // Length cap from VARCHAR/NVARCHAR.
        if let Some(open) = sql_type.find('(') {
            if let Some(close) = sql_type.find(')') {
                let inside = &sql_type[open + 1..close];
                if let Ok(n) = inside.trim().parse::<u32>() {
                    s.insert(
                        Value::String("maxLength".into()),
                        Value::Number(serde_yaml::Number::from(n)),
                    );
                }
            }
        }
        return Value::Mapping(s);
    }

    match base_type.as_str() {
        "UNIQUEIDENTIFIER" => {
            s.insert(Value::String("type".into()), Value::String("string".into()));
            s.insert(Value::String("format".into()), Value::String("uuid".into()));
        }
        "INT" | "BIGINT" | "SMALLINT" | "TINYINT" => {
            s.insert(Value::String("type".into()), Value::String("integer".into()));
        }
        "DECIMAL" | "NUMERIC" | "FLOAT" | "REAL" | "MONEY" | "SMALLMONEY" => {
            s.insert(Value::String("type".into()), Value::String("number".into()));
        }
        "BIT" => {
            s.insert(Value::String("type".into()), Value::String("boolean".into()));
        }
        "DATETIME" | "DATETIME2" | "DATETIMEOFFSET" | "SMALLDATETIME" => {
            s.insert(Value::String("type".into()), Value::String("string".into()));
            s.insert(Value::String("format".into()), Value::String("date-time".into()));
        }
        "DATE" => {
            s.insert(Value::String("type".into()), Value::String("string".into()));
            s.insert(Value::String("format".into()), Value::String("date".into()));
        }
        _ => {
            s.insert(Value::String("type".into()), Value::String("string".into()));
        }
    }
    // Length cap for VARCHAR / NVARCHAR.
    if let Some(open) = sql_type.find('(') {
        if let Some(close) = sql_type.find(')') {
            let inside = &sql_type[open + 1..close];
            if let Ok(n) = inside.trim().parse::<u32>() {
                s.insert(
                    Value::String("maxLength".into()),
                    Value::Number(serde_yaml::Number::from(n)),
                );
            }
        }
    }
    Value::Mapping(s)
}

fn build_responses(analysis: &ProcAnalysis, err_index: &ErrorCodeIndex) -> Value {
    let mut r = Mapping::new();

    // 200 OK — success envelope.
    let mut ok_body = Mapping::new();
    ok_body.insert(
        Value::String("description".into()),
        Value::String("Operation completed successfully.".into()),
    );
    let mut ok_content = Mapping::new();
    let mut ok_json = Mapping::new();
    ok_json.insert(
        Value::String("schema".into()),
        ref_schema("BaseResponseModel"),
    );
    ok_content.insert(Value::String("application/json".into()), Value::Mapping(ok_json));
    ok_body.insert(Value::String("content".into()), Value::Mapping(ok_content));
    r.insert(Value::String("200".into()), Value::Mapping(ok_body));

    // 400 — error catalogue from the proc.
    let mut err_body = Mapping::new();
    err_body.insert(
        Value::String("description".into()),
        Value::String("Validation or business-rule error.".into()),
    );
    let mut err_content = Mapping::new();
    let mut err_json = Mapping::new();
    err_json.insert(
        Value::String("schema".into()),
        ref_schema("BaseResponseModel"),
    );
    if !analysis.error_codes.is_empty() {
        let mut examples = Mapping::new();
        for code in &analysis.error_codes {
            let desc = err_index
                .by_code
                .get(code)
                .cloned()
                .unwrap_or_else(|| "(no description harvested from inline comments)".into());
            let mut ex = Mapping::new();
            ex.insert(
                Value::String("summary".into()),
                Value::String(format!("Error code {}", code)),
            );
            let mut value_obj = Mapping::new();
            value_obj.insert(
                Value::String("error".into()),
                Value::Mapping({
                    let mut m = Mapping::new();
                    m.insert(Value::String("code".into()), Value::String(code.clone()));
                    m.insert(Value::String("description".into()), Value::String(desc));
                    m
                }),
            );
            ex.insert(Value::String("value".into()), Value::Mapping(value_obj));
            examples.insert(Value::String(format!("error_{}", code)), Value::Mapping(ex));
        }
        err_json.insert(Value::String("examples".into()), Value::Mapping(examples));
    }
    err_content.insert(
        Value::String("application/json".into()),
        Value::Mapping(err_json),
    );
    err_body.insert(Value::String("content".into()), Value::Mapping(err_content));
    r.insert(Value::String("400".into()), Value::Mapping(err_body));

    Value::Mapping(r)
}

fn ref_schema(name: &str) -> Value {
    let mut m = Mapping::new();
    m.insert(
        Value::String("$ref".into()),
        Value::String(format!("#/components/schemas/{}", name)),
    );
    Value::Mapping(m)
}

fn build_internal_item(
    a: &ProcAnalysis,
    proc: &SqlObject,
    err_index: &ErrorCodeIndex,
) -> Value {
    // Internal-only — not a public API. Single x-internal entry.
    let mut op = Mapping::new();
    op.insert(
        Value::String("summary".into()),
        Value::String(format!(
            "Internal procedure (no public verb mapping): {}",
            proc.name
        )),
    );
    op.insert(
        Value::String("description".into()),
        Value::String(format!(
            "Surfaced by forge for review. {} {}",
            describe_op_semantics(a, ""),
            "Add a public route mapping by renaming the proc to follow \
             `p_txn_<Verb>_<Entity>` convention, or document why this is \
             internal-only."
        )),
    );
    op.insert(Value::String("x-forge-internal".into()), Value::Bool(true));
    op.insert(
        Value::String("responses".into()),
        build_responses(a, err_index),
    );
    let mut item = Mapping::new();
    item.insert(Value::String("post".into()), Value::Mapping(op));
    Value::Mapping(item)
}

fn merge_path(paths: &mut Vec<(String, Value)>, path: &str, method: &str, item: Value) {
    let new_op = match item {
        Value::Mapping(mut m) => m.remove(&Value::String(method.into())).unwrap_or(Value::Null),
        other => other,
    };
    if let Some(existing) = paths.iter_mut().find(|(k, _)| k == path) {
        if let Value::Mapping(map) = &mut existing.1 {
            map.insert(Value::String(method.into()), new_op);
        }
    } else {
        let mut item = Mapping::new();
        item.insert(Value::String(method.into()), new_op);
        paths.push((path.to_string(), Value::Mapping(item)));
    }
}

// ─────────────────────────── components ───────────────────────────

fn common_components(_err: &ErrorCodeIndex) -> Mapping {
    let mut params = Mapping::new();
    // Only RequestId + CorrelationId — they exist as `@uRequestId`
    // and `@uCorrelationId` parameters on every dt proc, so they're
    // SQL-derivable. CorrelationId is optional because the proc
    // accepts NULL (caller defaults to NEWID() when missing).
    params.insert(
        Value::String("RequestId".into()),
        header_param(
            "RequestId",
            "Unique identifier for this API request — bound to `@uRequestId UNIQUEIDENTIFIER` in the proc.",
            true,
        ),
    );
    params.insert(
        Value::String("CorrelationId".into()),
        header_param(
            "CorrelationId",
            "Correlation identifier for tracing across services — bound to `@uCorrelationId UNIQUEIDENTIFIER` in the proc. Required by the proc signature; the C# controller layer may default it when absent, but the SQL contract treats it as required.",
            true,
        ),
    );

    let mut schemas = Mapping::new();
    schemas.insert(
        Value::String("BaseResponseModel".into()),
        base_response_schema(),
    );

    let mut components = Mapping::new();
    components.insert(Value::String("parameters".into()), Value::Mapping(params));
    components.insert(Value::String("schemas".into()), Value::Mapping(schemas));
    components
}

fn header_param(name: &str, description: &str, required: bool) -> Value {
    let mut p = Mapping::new();
    p.insert(Value::String("name".into()), Value::String(name.into()));
    p.insert(Value::String("in".into()), Value::String("header".into()));
    p.insert(Value::String("required".into()), Value::Bool(required));
    p.insert(
        Value::String("description".into()),
        Value::String(description.into()),
    );
    let mut sch = Mapping::new();
    sch.insert(Value::String("type".into()), Value::String("string".into()));
    p.insert(Value::String("schema".into()), Value::Mapping(sch));
    Value::Mapping(p)
}

fn base_response_schema() -> Value {
    // Mirrors the BaseResponseModel<T> the C# layer wraps every response in.
    let mut props = Mapping::new();
    props.insert(Value::String("requestId".into()), uuid_string_schema());
    props.insert(Value::String("correlationId".into()), uuid_string_schema());
    props.insert(Value::String("responseId".into()), uuid_string_schema());
    props.insert(Value::String("dateTime".into()), datetime_string_schema());
    props.insert(Value::String("result".into()), object_schema_nullable());
    props.insert(Value::String("error".into()), error_schema());

    let mut schema = Mapping::new();
    schema.insert(Value::String("type".into()), Value::String("object".into()));
    schema.insert(Value::String("properties".into()), Value::Mapping(props));
    schema.insert(
        Value::String("required".into()),
        Value::Sequence(vec![
            Value::String("requestId".into()),
            Value::String("correlationId".into()),
            Value::String("responseId".into()),
            Value::String("dateTime".into()),
        ]),
    );
    Value::Mapping(schema)
}

fn uuid_string_schema() -> Value {
    let mut s = Mapping::new();
    s.insert(Value::String("type".into()), Value::String("string".into()));
    s.insert(Value::String("format".into()), Value::String("uuid".into()));
    Value::Mapping(s)
}

fn datetime_string_schema() -> Value {
    let mut s = Mapping::new();
    s.insert(Value::String("type".into()), Value::String("string".into()));
    s.insert(Value::String("format".into()), Value::String("date-time".into()));
    Value::Mapping(s)
}

fn object_schema_nullable() -> Value {
    let mut s = Mapping::new();
    s.insert(Value::String("type".into()), Value::String("object".into()));
    s.insert(Value::String("nullable".into()), Value::Bool(true));
    Value::Mapping(s)
}

fn error_schema() -> Value {
    let mut props = Mapping::new();
    let mut code = Mapping::new();
    code.insert(Value::String("type".into()), Value::String("string".into()));
    props.insert(Value::String("code".into()), Value::Mapping(code));
    let mut desc = Mapping::new();
    desc.insert(Value::String("type".into()), Value::String("string".into()));
    props.insert(Value::String("description".into()), Value::Mapping(desc));

    let mut schema = Mapping::new();
    schema.insert(Value::String("type".into()), Value::String("object".into()));
    schema.insert(Value::String("nullable".into()), Value::Bool(true));
    schema.insert(Value::String("properties".into()), Value::Mapping(props));
    Value::Mapping(schema)
}

// ─────────────────────────── lookup format hints ───────────────────────────

/// Per-proc index of body-variable → lookup-validation hint.
///
/// Built by scanning the proc body for the canonical pattern:
///
/// ```sql
/// SELECT @var = some_col
/// FROM lookups.<table>
/// WHERE <table>.<alt_key_col> = @var
/// ```
///
/// The `alt_key_col` name (e.g. `col_Alpha_2_Code`, `lsl_ISO_639_1`)
/// tells us the format the API accepts. Forge maps the column name
/// to a human-readable format hint that becomes the OpenAPI
/// `description`. The lookup table goes into `x-validated-by`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LookupHintIndex {
    /// `proc full_name → variable_name → hint`.
    pub by_proc: BTreeMap<String, BTreeMap<String, LookupHint>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookupHint {
    /// e.g. `lookups.col_Country_Lookup`
    pub validated_by: String,
    /// e.g. `col_Alpha_2_Code`
    pub matched_column: String,
    /// Human-readable format hint, e.g. "ISO 3166-1 alpha-2 (e.g. ZA)".
    pub format_hint: String,
}

/// Build a per-proc map of `OPENJSON column-alias → LookupHint`.
///
/// The chain we follow is:
/// 1. `OPENJSON(@jRequest) WITH (<alias> <SQL_TYPE> '<json_path>')`
///    declares an alias bound to a JSON path.
/// 2. `SELECT @<var> = [<alias>] FROM OPENJSON(...)` binds that
///    alias to a local variable.
/// 3. Later `WHERE <table>.<alt_key_col> = @<var>` (or the reverse
///    `@<var> = <col> FROM lookups.<table> WHERE …`) tells us the
///    column the proc validates the value against.
///
/// The result is keyed BOTH on the alias AND the JSON-path leaf.
/// Real procs often declare `billing_country VARCHAR '$.billingAddress.country'`
/// — the renderer needs the leaf (`country`) so the nested object
/// schema picks up the hint, while the alias key supports anyone
/// who'd want to look up by the SQL-side name.
pub fn analyse_lookup_hints(
    _proc_full_name: &str,
    body: &str,
    catalog: &SqlCatalog,
) -> BTreeMap<String, LookupHint> {
    let alias_to_var = extract_alias_var_bindings(body);
    let var_to_hint = extract_var_lookup_hints(body, catalog);
    let alias_to_path = extract_openjson_alias_to_path(body);

    let mut out: BTreeMap<String, LookupHint> = BTreeMap::new();
    for (alias, var) in &alias_to_var {
        if let Some(hint) = var_to_hint.get(var) {
            out.entry(alias.clone()).or_insert_with(|| hint.clone());
            // Also key by JSON-path leaf so nested-schema rendering
            // (where the property name is the leaf, not the alias)
            // can pick up the same hint.
            if let Some(path) = alias_to_path.get(alias) {
                let leaf = path
                    .trim_start_matches('$')
                    .trim_start_matches('.')
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !leaf.is_empty() && leaf != *alias {
                    out.entry(leaf).or_insert_with(|| hint.clone());
                }
            }
        }
    }
    out
}

/// Parse the `OPENJSON(@jRequest) WITH (<alias> TYPE '<json_path>')`
/// clause and produce `alias → json_path` map. Reuses the same
/// scanner shape as `proc_analysis::extract_json_keys` but exposes
/// the alias name (which `JsonKey` doesn't carry).
fn extract_openjson_alias_to_path(body: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let lower = body.to_ascii_lowercase();
    let mut idx = 0usize;
    while let Some(rel) = lower[idx..].find("openjson") {
        let abs = idx + rel;
        // Find WITH after OPENJSON.
        let after_lower = &lower[abs..];
        let with_rel = after_lower.find("with");
        let Some(with_rel) = with_rel else {
            idx = abs + 8;
            continue;
        };
        let with_abs = abs + with_rel;
        let after_with = &body[with_abs..];
        let Some(open_rel) = after_with.find('(') else {
            idx = with_abs + 4;
            continue;
        };
        let bytes = after_with.as_bytes();
        let mut depth = 0i32;
        let mut close_rel = None;
        for (i, b) in bytes.iter().enumerate().skip(open_rel) {
            match *b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_rel = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_rel) = close_rel else {
            break;
        };
        let inside = &after_with[open_rel + 1..close_rel];
        // Each entry: `<alias> <SQL_TYPE> '<path>' [AS JSON]`.
        for entry in inside.split(',') {
            let line = entry.trim();
            if line.is_empty() {
                continue;
            }
            let Some(q1) = line.find('\'') else {
                continue;
            };
            let after_q1 = &line[q1 + 1..];
            let Some(q2_rel) = after_q1.find('\'') else {
                continue;
            };
            let path = after_q1[..q2_rel].to_string();
            let prefix = line[..q1].trim();
            let first_ws = prefix.find(char::is_whitespace).unwrap_or(prefix.len());
            let alias = prefix[..first_ws].trim().to_string();
            if !alias.is_empty() && !path.is_empty() {
                out.insert(alias, path);
            }
        }
        idx = with_abs + close_rel;
    }
    out
}

/// Find every `SELECT @<var> = [<alias>]` (or `@<var> = <alias>`)
/// inside an OPENJSON SELECT block. Returns alias → variable.
fn extract_alias_var_bindings(body: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    // Scan every line for `@var = [alias]` or `@var = alias` patterns.
    // Conservative — we don't try to bound by SELECT/FROM blocks; the
    // alias names are unique enough within a proc.
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('@') {
            continue;
        }
        let Some(eq_idx) = trimmed.find('=') else {
            continue;
        };
        let var = trimmed[..eq_idx]
            .trim()
            .trim_end_matches(',')
            .to_string();
        if !var.starts_with('@') {
            continue;
        }
        let rest = trimmed[eq_idx + 1..].trim().trim_end_matches(',');
        let rest_clean = rest.replace('[', "").replace(']', "");
        // The RHS we want is a bare alias identifier — no parens, no
        // quotes, no @vars. Reject anything fancy.
        let alias = rest_clean.trim().to_string();
        if alias.is_empty() {
            continue;
        }
        if alias.starts_with('@') || alias.contains('(') || alias.contains('\'') {
            continue;
        }
        // Take only the leading identifier token.
        let leading: String = alias
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if leading.is_empty() {
            continue;
        }
        // Skip cases where the RHS is a function call like `GETUTCDATE()`
        // (already filtered by paren), a CAST-stripped expression, or
        // a `SELECT TOP` result — we want only the simple alias case
        // emitted by `OPENJSON … WITH (…)`.
        if alias != leading {
            // Allow only if the trailing chars are punctuation (e.g.
            // `country,` already trimmed; `country` alone passes).
            // If anything else followed, this is an expression, skip.
            let trailing = &alias[leading.len()..];
            if !trailing.trim().is_empty() {
                continue;
            }
        }
        // Don't overwrite earlier bindings. First alias wins per var.
        out.entry(leading).or_insert(var);
    }
    out
}

/// Find every `<table>.<col> = @<var>` (or `<col> = @<var>` after
/// `lookups.<table>`) clause and produce var → hint.
///
/// The scan window is bounded by the **next** `lookups.` mention so
/// we don't pick up an alt-key column that belongs to a different
/// lookup table. Hits where the WHERE compares against a literal
/// (e.g. `WHERE prc_Code = 60105`) are skipped — only `@<var>`
/// right-hand-sides count.
fn extract_var_lookup_hints(body: &str, catalog: &SqlCatalog) -> BTreeMap<String, LookupHint> {
    let mut out: BTreeMap<String, LookupHint> = BTreeMap::new();
    let lower = body.to_ascii_lowercase();

    let mut starts: Vec<(usize, String)> = Vec::new();
    let mut idx = 0usize;
    while let Some(rel) = lower[idx..].find("lookups.") {
        let abs = idx + rel;
        let after = &body[abs + "lookups.".len()..];
        let mut table = String::new();
        for c in after.chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                table.push(c);
            } else {
                break;
            }
        }
        if !table.is_empty() {
            starts.push((abs, table.clone()));
        }
        idx = abs + "lookups.".len() + table.len().max(1);
    }

    for (i, (start, table)) in starts.iter().enumerate() {
        let end = starts
            .get(i + 1)
            .map(|(s, _)| *s)
            .unwrap_or_else(|| body.len());
        let window = &body[*start..end];
        if let Some((var_name, alt_col)) = find_where_pattern(window) {
            // Verify: the matched column must actually be a column of
            // `lookups.<table>` in the catalog. This guards against
            // heuristic over-reach (e.g. `WHERE cpf_Last_Name = …`
            // landing inside a `lookups.prc_Program_Response_Const_Lookup`
            // window because the proc is large).
            if !column_exists_on(catalog, "lookups", table, &alt_col) {
                continue;
            }
            let format_hint = derive_format_hint(&alt_col, table);
            out.entry(var_name).or_insert(LookupHint {
                validated_by: format!("lookups.{}", table),
                matched_column: alt_col,
                format_hint,
            });
        }
    }
    out
}

/// Decide whether a `WHERE <col> = @<var>` near `lookups.<table>` is a
/// genuine validation, or a heuristic false-positive (e.g. a WHERE
/// clause from an unrelated nearby block leaking into the lookup
/// window).
///
/// Rules:
/// 1. If the catalog has the table, verify the column actually exists
///    on it. Definitive answer.
/// 2. If the catalog doesn't have the table (common: lookups live in a
///    sibling schema repo), fall back to **Hungarian-prefix matching**:
///    real lookup tables in this codebase follow `<prefix>_<rest>_Lookup`
///    where the alt-key column also starts with `<prefix>_*` (e.g.
///    `col_Country_Lookup` → `col_Alpha_2_Code` shares prefix `col`).
///    Reject anything where the column's prefix doesn't match the
///    table's prefix.
/// 3. Built-in fallback: also accept generic `iso_*` and `<prefix>_Code`
///    column names since those follow the same convention.
fn column_exists_on(catalog: &SqlCatalog, schema: &str, table: &str, col: &str) -> bool {
    let target = catalog.tables.iter().find(|t| {
        t.schema
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case(schema))
            .unwrap_or(false)
            && t.name.eq_ignore_ascii_case(table)
    });
    if let Some(t) = target {
        return t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col));
    }
    // Hungarian-prefix fallback.
    let table_prefix = hungarian_prefix(table);
    let col_prefix = hungarian_prefix(col);
    let col_lc = col.to_ascii_lowercase();
    if !table_prefix.is_empty()
        && !col_prefix.is_empty()
        && table_prefix.eq_ignore_ascii_case(&col_prefix)
    {
        return true;
    }
    // Standard ISO column patterns are allowed regardless of prefix —
    // they're a known convention.
    if col_lc.starts_with("iso_") || col_lc.contains("alpha_2") || col_lc.contains("alpha_3") {
        return true;
    }
    false
}

fn hungarian_prefix(s: &str) -> String {
    // First underscore-terminated token if it's 2-4 lowercase letters.
    let mut parts = s.splitn(2, '_');
    let first = parts.next().unwrap_or("");
    if (2..=4).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_lowercase())
    {
        first.to_string()
    } else {
        String::new()
    }
}

/// Within a small window starting at a `lookups.<table>` mention,
/// find the FIRST `<x>.<col> = @<var>` or `<col> = @<var>` clause
/// after a `WHERE` keyword. Returns `(var_name_with_leading_at,
/// alt_col_name)`.
fn find_where_pattern(window: &str) -> Option<(String, String)> {
    let lower = window.to_ascii_lowercase();
    // Match `where` followed by any whitespace (space, tab, newline)
    // with a word boundary before. The `where ` literal didn't work
    // because real procs format SQL with `WHERE` on its own line.
    let lower_bytes = lower.as_bytes();
    let where_idx = (0..lower_bytes.len()).find(|&i| {
        if i + 5 > lower_bytes.len() {
            return false;
        }
        if &lower_bytes[i..i + 5] != b"where" {
            return false;
        }
        let prev_ok =
            i == 0 || (!lower_bytes[i - 1].is_ascii_alphanumeric() && lower_bytes[i - 1] != b'_');
        let next_ok = i + 5 == lower_bytes.len() || lower_bytes[i + 5].is_ascii_whitespace();
        prev_ok && next_ok
    })?;
    let after = &window[where_idx + "where".len()..];
    // Walk forward token-by-token until we hit the `<col> = @<var>`
    // shape. Tolerate brackets, schema-qualifiers, whitespace.
    // The column we want is the one being compared against the
    // variable, so we look for `<x> = @<var>` pattern.
    let bytes = after.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // Walk back to find the column identifier.
            let mut j = i;
            while j > 0 && (bytes[j - 1] == b' ' || bytes[j - 1] == b'\t') {
                j -= 1;
            }
            let col_end = j;
            while j > 0 {
                let b = bytes[j - 1];
                if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'[' || b == b']' {
                    j -= 1;
                } else {
                    break;
                }
            }
            let col_raw = &after[j..col_end];
            // Strip table-qualifier: `tbl.col` → `col`.
            let col_clean = col_raw
                .replace('[', "")
                .replace(']', "")
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_string();
            // Walk forward past `=` and whitespace to find `@var`.
            let mut k = i + 1;
            while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'@' {
                let var_start = k;
                let mut m = k + 1;
                while m < bytes.len()
                    && (bytes[m].is_ascii_alphanumeric() || bytes[m] == b'_')
                {
                    m += 1;
                }
                let var_name = after[var_start..m].to_string();
                if !col_clean.is_empty() && !var_name.is_empty() {
                    return Some((var_name, col_clean));
                }
            }
        }
        i += 1;
    }
    None
}

/// Map a lookup alt-key column name to a human-readable format hint.
/// The convention follows the dt SQL codebase:
///
/// - `*_Alpha_2_Code` → ISO 3166-1 alpha-2 country code
/// - `*_Alpha_3_Code` → ISO 3166-1 alpha-3 country code
/// - `*_ISO_639_1`    → ISO 639-1 language code
/// - `iso_3166_2_*`   → ISO 3166-2 region/state code
/// - `*_Code`         → opaque code, validated against the lookup table
/// - `*_Desc`         → human description, validated against the lookup table
fn derive_format_hint(alt_column: &str, lookup_table: &str) -> String {
    let lc = alt_column.to_ascii_lowercase();
    if lc.contains("alpha_2_code") || lc.ends_with("alpha_2") {
        return "ISO 3166-1 alpha-2 country code (e.g. ZA, US, GB)".into();
    }
    if lc.contains("alpha_3_code") || lc.ends_with("alpha_3") {
        return "ISO 3166-1 alpha-3 country code (e.g. ZAF, USA, GBR)".into();
    }
    if lc.contains("iso_639_1") {
        return "ISO 639-1 language code (e.g. en, af, fr)".into();
    }
    if lc.contains("iso_3166_2") {
        return "ISO 3166-2 region/state code (e.g. ZA-WC, US-NY)".into();
    }
    if lc.ends_with("_code") || lc == "code" {
        return format!(
            "Opaque code validated at runtime against `lookups.{}`. \
             See the lookup table for current allowed values.",
            lookup_table
        );
    }
    if lc.ends_with("_desc") || lc == "description" {
        return format!(
            "Human description validated at runtime against `lookups.{}`.",
            lookup_table
        );
    }
    format!(
        "Validated at runtime against `lookups.{}` (column `{}`).",
        lookup_table, alt_column
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_catalog::{SqlCatalog, SqlKind, SqlObject, TableOp, TableOpKind};

    fn proc(name: &str) -> SqlObject {
        SqlObject {
            kind: SqlKind::Procedure,
            schema: Some("cardholder".into()),
            name: name.into(),
            referenced_tables: Vec::new(),
            ops: vec![TableOp {
                table: "cardholder.cpf_Client_Profile".into(),
                kinds: vec![TableOpKind::Update],
            }],
            source_path: "fake.sql".into(),
        }
    }

    #[test]
    fn verb_map_post_create_cardholder() {
        let m = map_verb("p_txn_Create_Cardholder").unwrap();
        assert_eq!(m.method, "post");
        assert_eq!(m.entity_path, "cardholder");
    }

    #[test]
    fn verb_map_put_update_cardholder() {
        let m = map_verb("p_txn_Update_Cardholder").unwrap();
        assert_eq!(m.method, "put");
        assert_eq!(m.entity_path, "cardholder");
    }

    #[test]
    fn verb_map_get_details_picks_id_path() {
        let m = map_verb("p_txn_Get_Cardholder_Details").unwrap();
        assert_eq!(m.method, "get");
        assert!(m.has_id);
    }

    #[test]
    fn verb_map_get_all_details_picks_list_path() {
        let m = map_verb("p_txn_Get_All_Cardholder_Details").unwrap();
        assert_eq!(m.method, "get");
        assert!(!m.has_id);
    }

    #[test]
    fn unknown_proc_returns_none() {
        assert!(map_verb("p_dte_Format_Error_Message").is_none());
    }

    #[test]
    fn schema_for_uniqueidentifier_emits_string_uuid() {
        let lookups: BTreeMap<String, LookupHint> = BTreeMap::new();
        let enums: EnumOverrides = BTreeMap::new();
        let v = schema_for_sql_type("UNIQUEIDENTIFIER", "id", &lookups, &enums);
        let yaml = serde_yaml::to_string(&v).unwrap();
        assert!(yaml.contains("type: string"));
        assert!(yaml.contains("format: uuid"));
    }

    #[test]
    fn schema_for_nvarchar_with_length_emits_max_length() {
        let lookups: BTreeMap<String, LookupHint> = BTreeMap::new();
        let enums: EnumOverrides = BTreeMap::new();
        let v = schema_for_sql_type("NVARCHAR(1024)", "lastName", &lookups, &enums);
        let yaml = serde_yaml::to_string(&v).unwrap();
        assert!(yaml.contains("maxLength: 1024"));
    }

    #[test]
    fn lookup_hinted_field_emits_format_hint_and_validated_by() {
        let mut lookups: BTreeMap<String, LookupHint> = BTreeMap::new();
        lookups.insert(
            "country".into(),
            LookupHint {
                validated_by: "lookups.col_Country_Lookup".into(),
                matched_column: "col_Alpha_2_Code".into(),
                format_hint: "ISO 3166-1 alpha-2 country code (e.g. ZA, US, GB)".into(),
            },
        );
        let enums: EnumOverrides = BTreeMap::new();
        let v = schema_for_sql_type("VARCHAR(2)", "country", &lookups, &enums);
        let yaml = serde_yaml::to_string(&v).unwrap();
        assert!(yaml.contains("type: string"));
        assert!(yaml.contains("ISO 3166-1 alpha-2"));
        assert!(yaml.contains("x-validated-by: lookups.col_Country_Lookup"));
        assert!(yaml.contains("x-matched-column: col_Alpha_2_Code"));
        // No enum should be emitted — runtime lookup, not enum.
        assert!(!yaml.contains("enum:"));
    }

    #[test]
    fn analyse_lookup_hints_chains_alias_var_to_alt_key_column() {
        let body = r#"
            CREATE PROCEDURE [x].[y] AS BEGIN
                SELECT
                    @sCountry = [country]
                FROM
                    OPENJSON(@jRequest)
                        WITH (
                            country VARCHAR(2) '$.billingAddress.country'
                        );

                SELECT
                    @sCountry = col_Code
                FROM
                    lookups.col_Country_Lookup WITH (NOLOCK)
                WHERE
                    col_Alpha_2_Code = @sCountry;
            END
        "#;
        // No catalog → column verification falls back to "trust the
        // WHERE clause" (see column_exists_on rationale).
        let cat = SqlCatalog::default();
        let hints = analyse_lookup_hints("x.y", body, &cat);
        let h = hints.get("country").expect("country alias should map");
        assert_eq!(h.validated_by, "lookups.col_Country_Lookup");
        assert!(h.format_hint.contains("ISO 3166-1 alpha-2"));
    }

    #[test]
    fn lookup_hint_dropped_when_prefix_mismatch_and_table_not_in_catalog() {
        // Real-world: dtcard ground-truth has only `cardholder` schema
        // tables, no `lookups` schema. A heuristic-overreach WHERE
        // like `WHERE cpf_Last_Name = @sLastName` near a `lookups.prc_*`
        // mention should be DROPPED on prefix mismatch (`cpf` vs `prc`).
        let body = r#"
            CREATE PROCEDURE [x].[y] AS BEGIN
                SELECT @sLastName = [last_name]
                FROM OPENJSON(@jRequest)
                    WITH (last_name NVARCHAR(1024) '$.personalDetails.lastName');

                SELECT prc_Code, prc_Desc
                FROM lookups.prc_Program_Response_Const_Lookup
                WHERE cpf_Last_Name = @sLastName;
            END
        "#;
        let cat = SqlCatalog::default(); // table not in catalog
        let hints = analyse_lookup_hints("x.y", body, &cat);
        assert!(
            hints.get("last_name").is_none() && hints.get("lastName").is_none(),
            "spurious hint emitted (prefix mismatch should drop): {:?}",
            hints
        );
    }

    #[test]
    fn lookup_hint_dropped_when_matched_column_not_on_lookup_table() {
        let body = r#"
            CREATE PROCEDURE [x].[y] AS BEGIN
                SELECT @sLastName = [last_name]
                FROM OPENJSON(@jRequest)
                    WITH (last_name NVARCHAR(1024) '$.personalDetails.lastName');

                SELECT prc_Code, prc_Desc
                FROM lookups.prc_Program_Response_Const_Lookup
                WHERE cpf_Last_Name = @sLastName;
            END
        "#;
        // Catalog has the lookup table, but cpf_Last_Name is NOT one
        // of its columns — we expect the hint to be suppressed.
        use crate::schema::parse_create_table;
        let prc = parse_create_table(
            r#"CREATE TABLE [lookups].[prc_Program_Response_Const_Lookup] (
                prc_Code INT NOT NULL,
                prc_Desc NVARCHAR(1000) NULL,
                CONSTRAINT [PK_prc] PRIMARY KEY (prc_Code)
            );"#,
        )
        .unwrap();
        let cat = SqlCatalog {
            tables: vec![prc],
            objects: Vec::new(),
        };
        let hints = analyse_lookup_hints("x.y", body, &cat);
        assert!(
            hints.get("last_name").is_none() && hints.get("lastName").is_none(),
            "spurious hint emitted: {:?}",
            hints
        );
    }

    #[test]
    fn generate_openapi_emits_minimum_doc_with_no_procs() {
        let cat = SqlCatalog::default();
        let api = generate_openapi(
            "Cardholder Management",
            "cardholder",
            &cat,
            std::path::Path::new("."),
            None,
        );
        assert!(api.yaml.contains("openapi: 3.1.0"));
        assert!(api.yaml.contains("Cardholder Management"));
        assert_eq!(api.op_count, 0);
    }

    #[test]
    fn generate_openapi_emits_path_for_known_proc() {
        let cat = SqlCatalog {
            tables: Vec::new(),
            objects: vec![proc("p_txn_Update_Cardholder")],
        };
        let api = generate_openapi(
            "Cardholder Management",
            "cardholder",
            &cat,
            std::path::Path::new("."),
            None,
        );
        assert!(api.yaml.contains("/cardholder"));
        assert!(api.yaml.contains("put:"));
        assert_eq!(api.op_count, 1);
    }
}
