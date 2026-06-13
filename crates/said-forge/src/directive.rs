//! Directive-op abstraction.
//!
//! A directive "op" is a single operation described by the wishlist: one
//! HTTP endpoint, one spec-file heading, one Excel row, one JSON endpoint
//! block, whatever. The brain stores each op as a frame tagged
//! `forge-op:<slug>`, with the operation's field schema embedded as JSON.
//! `forge gaps` + `forge run` iterate these frames regardless of source —
//! the adapter is responsible for parsing source → ops; everyone downstream
//! works in ops.
//!
//! Current extractors:
//! - OpenAPI: one op per `paths.<path>.<method>`; fields from
//!   `requestBody.content.application/json.schema.properties`, and from
//!   path + query + header parameters.
//! - Markdown (Dev Planning): one op per heading; fields from the
//!   `### Parameters` section using the bulleted-name / required /
//!   location / type shape we've seen in dt's spec files.
//!
//! XLSX row-as-op extractor reuses column headers as fields — that's
//! handled inline in `source::xlsx::extract_stories` for now; an xlsx
//! adapter for OpSpec is a follow-up.

use serde::{Deserialize, Serialize};

/// A single operation from the wishlist (OpenAPI op, MD spec heading, …).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpSpec {
    /// Slug for tagging. Stable across re-ingests of the same op.
    pub slug: String,
    /// Human label — `"POST /cardholders"` or `"Create Cardholder"`.
    pub label: String,
    /// HTTP method + path (when derivable; None for non-HTTP ops).
    pub method: Option<String>,
    pub path: Option<String>,
    /// Free-text summary if the source had one.
    pub summary: Option<String>,
    /// Fields describing the request shape.
    pub fields: Vec<OpField>,
    /// Source adapter name (openapi, markdown, xlsx).
    pub adapter: String,
    /// Where in the source this came from — file path + anchor.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpField {
    pub name: String,
    /// Logical type token — `string`, `integer`, `number`, `boolean`,
    /// `array`, `object`, `uuid`, `date-time`, `date`. Adapter-normalised.
    pub logical_type: String,
    /// Where the field appears: `body`, `path`, `query`, `header`.
    pub location: Location,
    pub required: bool,
    /// Optional max length hint (OpenAPI `maxLength`, etc). Used for
    /// varchar-length comparison.
    pub max_length: Option<u32>,
    /// Optional format (OpenAPI `format` — `date-time`, `uuid`, `email`, …).
    pub format: Option<String>,
    /// Free-text description from the source.
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Location {
    Body,
    Path,
    Query,
    Header,
    Other,
}

// ─────────────────────────── OpenAPI extractor ───────────────────────────

/// Extract ops from an OpenAPI doc loaded as serde_json::Value. Assumes the
/// doc is already YAML→JSON-converted (OpenApiSource does this).
pub fn extract_ops_from_openapi(doc: &serde_json::Value, source: &str) -> Vec<OpSpec> {
    let mut ops = Vec::new();
    let Some(paths) = doc.get("paths").and_then(|p| p.as_object()) else {
        return ops;
    };
    let components_schemas = doc.pointer("/components/schemas").cloned();

    for (path, path_item) in paths {
        let Some(path_obj) = path_item.as_object() else { continue };
        for (method_key, op) in path_obj {
            let method_upper = method_key.to_ascii_uppercase();
            if !matches!(
                method_upper.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD"
            ) {
                continue;
            }
            let slug = slugify_op(&method_upper, path);
            let label = format!("{} {}", method_upper, path);
            let summary = op
                .get("summary")
                .and_then(|s| s.as_str())
                .map(String::from);

            let mut fields: Vec<OpField> = Vec::new();

            // parameters: path, query, header.
            if let Some(params) = op.get("parameters").and_then(|p| p.as_array()) {
                for p in params {
                    if let Some(f) = openapi_param_to_field(p) {
                        fields.push(f);
                    }
                }
            }

            // requestBody.content["application/json"].schema.properties.
            if let Some(body_schema) = op.pointer("/requestBody/content/application~1json/schema")
            {
                let resolved = resolve_ref(body_schema, components_schemas.as_ref());
                flatten_object_fields(&resolved, Location::Body, "", &mut fields, components_schemas.as_ref());
            }

            ops.push(OpSpec {
                slug,
                label,
                method: Some(method_upper),
                path: Some(path.clone()),
                summary,
                fields,
                adapter: "openapi".into(),
                source: format!("{}#{}.{}", source, path, method_key),
            });
        }
    }
    ops
}

fn resolve_ref<'a>(
    schema: &'a serde_json::Value,
    components: Option<&'a serde_json::Value>,
) -> serde_json::Value {
    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str()) {
        // Strip `#/components/schemas/` prefix to get the key.
        if let Some(key) = r.strip_prefix("#/components/schemas/") {
            if let Some(components) = components {
                if let Some(v) = components.get(key) {
                    return v.clone();
                }
            }
        }
    }
    schema.clone()
}

fn openapi_param_to_field(p: &serde_json::Value) -> Option<OpField> {
    let name = p.get("name").and_then(|v| v.as_str())?.to_string();
    let loc = match p.get("in").and_then(|v| v.as_str()) {
        Some("path") => Location::Path,
        Some("query") => Location::Query,
        Some("header") => Location::Header,
        Some("cookie") => Location::Other,
        _ => Location::Other,
    };
    let required = p
        .get("required")
        .and_then(|v| v.as_bool())
        .unwrap_or(matches!(loc, Location::Path)); // path params are always required
    let description = p.get("description").and_then(|v| v.as_str()).map(String::from);
    let schema = p.get("schema").unwrap_or(p);
    let logical_type = json_type_of(schema).unwrap_or_else(|| "string".into());
    let format = schema
        .get("format")
        .and_then(|v| v.as_str())
        .map(String::from);
    let max_length = schema
        .get("maxLength")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    Some(OpField {
        name,
        logical_type,
        location: loc,
        required,
        max_length,
        format,
        description,
    })
}

fn flatten_object_fields(
    schema: &serde_json::Value,
    location: Location,
    prefix: &str,
    out: &mut Vec<OpField>,
    components: Option<&serde_json::Value>,
) {
    let resolved = resolve_ref(schema, components);
    let Some(props) = resolved.get("properties").and_then(|p| p.as_object()) else {
        return;
    };
    let required_set: std::collections::HashSet<String> = resolved
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for (name, prop) in props {
        let prop_resolved = resolve_ref(prop, components);
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        let logical_type = json_type_of(&prop_resolved).unwrap_or_else(|| "string".into());
        let format = prop_resolved
            .get("format")
            .and_then(|v| v.as_str())
            .map(String::from);
        let max_length = prop_resolved
            .get("maxLength")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);
        let description = prop_resolved
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        out.push(OpField {
            name: full.clone(),
            logical_type: logical_type.clone(),
            location,
            required: required_set.contains(name),
            max_length,
            format,
            description,
        });
        // Recurse into nested objects at most one level (keeps field lists
        // tractable; deeper nesting is rare in REST bodies and noisy).
        if logical_type == "object" && !prefix.contains('.') {
            flatten_object_fields(&prop_resolved, location, &full, out, components);
        }
    }
}

fn json_type_of(v: &serde_json::Value) -> Option<String> {
    v.get("type").and_then(|t| t.as_str()).map(String::from)
}

fn slugify_op(method: &str, path: &str) -> String {
    let path_part: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let combined = format!("{}-{}", method.to_lowercase(), path_part);
    let mut out = String::with_capacity(combined.len());
    let mut prev_dash = false;
    for c in combined.chars() {
        if c == '-' {
            if !prev_dash && !out.is_empty() {
                out.push(c);
                prev_dash = true;
            }
        } else {
            out.push(c);
            prev_dash = false;
        }
    }
    out.trim_end_matches('-').to_string()
}

// ─────────────────────────── Markdown (Dev Planning) extractor ───────────────────────────

/// Extract ops from a Markdown spec bundle — heading per op, bulleted
/// Parameters section beneath. Matches the dt/Spec/Dev Planning/spec/*.md
/// shape: `## POST /cardholders`, `### Parameters`, bullet list of
/// `#### field_name` + `- **Required**: True` + `- **Location**: body`
/// + `- **Type**: string`.
pub fn extract_ops_from_markdown(text: &str, source: &str) -> Vec<OpSpec> {
    let mut ops = Vec::new();
    // Split on `^## ` headings — each chunk below is one operation.
    let lines: Vec<&str> = text.lines().collect();
    let mut heading_indices: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("## ") && !line.starts_with("### ") {
            heading_indices.push(i);
        }
    }

    for (idx_in_list, &start) in heading_indices.iter().enumerate() {
        let end = heading_indices
            .get(idx_in_list + 1)
            .copied()
            .unwrap_or(lines.len());
        let heading = lines[start].trim_start_matches("##").trim().to_string();
        let chunk = &lines[start..end];

        let (method, path) = extract_method_path(&heading);
        let summary = extract_summary(chunk);
        let fields = extract_md_parameters(chunk);
        let slug = match (&method, &path) {
            (Some(m), Some(p)) => slugify_op(m, p),
            _ => slugify_heading(&heading),
        };
        ops.push(OpSpec {
            slug,
            label: heading.clone(),
            method,
            path,
            summary,
            fields,
            adapter: "markdown".into(),
            source: format!("{}#line:{}", source, start + 1),
        });
    }
    ops
}

fn extract_method_path(heading: &str) -> (Option<String>, Option<String>) {
    let h = heading.trim();
    let mut parts = h.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();
    let first_upper = first.to_uppercase();
    if matches!(
        first_upper.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD"
    ) && rest.starts_with('/')
    {
        return (Some(first_upper), Some(rest.to_string()));
    }
    (None, None)
}

fn extract_summary(chunk: &[&str]) -> Option<String> {
    for line in chunk.iter().skip(1) {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("**Summary**:") {
            return Some(rest.trim().to_string());
        }
        if t.starts_with("### ") || t.starts_with("## ") {
            break;
        }
    }
    None
}

fn extract_md_parameters(chunk: &[&str]) -> Vec<OpField> {
    let mut out = Vec::new();
    let mut in_params = false;
    let mut current: Option<OpField> = None;

    for line in chunk {
        let t = line.trim();

        // Section gate: enter on `### Parameters`, exit on next H3 / H2.
        if t.starts_with("### ") {
            if t.eq_ignore_ascii_case("### Parameters") {
                in_params = true;
                continue;
            } else if in_params {
                if let Some(f) = current.take() {
                    out.push(f);
                }
                in_params = false;
                continue;
            }
        }
        if !in_params {
            continue;
        }

        if let Some(name) = t.strip_prefix("#### ") {
            if let Some(prev) = current.take() {
                out.push(prev);
            }
            current = Some(OpField {
                name: name.trim().to_string(),
                logical_type: "string".into(),
                location: Location::Body,
                required: false,
                max_length: None,
                format: None,
                description: None,
            });
            continue;
        }

        let Some(field) = current.as_mut() else { continue };
        // Values in dt's Dev Planning MDs are often wrapped in backticks:
        // `string`, `header`, `True`. Strip them before comparing.
        let strip = |s: &str| -> String {
            s.trim().trim_matches('`').trim().to_string()
        };
        if let Some(rest) = bullet_kv(t, "Required") {
            let v = strip(rest);
            field.required = v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes");
        } else if let Some(rest) = bullet_kv(t, "Location") {
            let v = strip(rest).to_ascii_lowercase();
            field.location = match v.as_str() {
                "path" => Location::Path,
                "query" => Location::Query,
                "header" => Location::Header,
                "body" => Location::Body,
                _ => Location::Other,
            };
        } else if let Some(rest) = bullet_kv(t, "Type") {
            field.logical_type = strip(rest).to_ascii_lowercase();
        } else if let Some(rest) = bullet_kv(t, "Format") {
            field.format = Some(strip(rest));
        } else if let Some(rest) = bullet_kv(t, "MaxLength") {
            field.max_length = strip(rest).parse().ok();
        } else if let Some(rest) = bullet_kv(t, "Description") {
            field.description = Some(strip(rest));
        }
    }
    if let Some(f) = current {
        out.push(f);
    }
    out
}

/// Parse bulleted key/value lines of the form `- **Key**: value` (optionally
/// with surrounding whitespace). Returns `Some(value)` when the key matches.
fn bullet_kv<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let t = line.trim();
    let stripped = t.strip_prefix('-')?.trim_start();
    let tag = format!("**{}**:", key);
    let rest = stripped.strip_prefix(&tag)?;
    Some(rest.trim())
}

fn slugify_heading(h: &str) -> String {
    h.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_op_stable() {
        assert_eq!(slugify_op("POST", "/cardholders"), "post-cardholders");
        assert_eq!(
            slugify_op("GET", "/cardholders/{cardholder_id}/transitions"),
            "get-cardholders-cardholder-id-transitions"
        );
    }

    #[test]
    fn openapi_extracts_op_with_body_and_query_fields() {
        let doc = serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/cardholders": {
                    "post": {
                        "summary": "Create a cardholder",
                        "parameters": [
                            {"name": "include", "in": "query", "schema": {"type": "string"}},
                        ],
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "required": ["firstName"],
                                        "properties": {
                                            "firstName": {"type": "string", "maxLength": 50},
                                            "dateOfBirth": {"type": "string", "format": "date"},
                                            "age": {"type": "integer"},
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        let ops = extract_ops_from_openapi(&doc, "api.yaml");
        assert_eq!(ops.len(), 1);
        let op = &ops[0];
        assert_eq!(op.method.as_deref(), Some("POST"));
        assert_eq!(op.path.as_deref(), Some("/cardholders"));
        assert_eq!(op.slug, "post-cardholders");
        // 1 query param + 3 body fields
        assert_eq!(op.fields.len(), 4);
        let first = op.fields.iter().find(|f| f.name == "firstName").unwrap();
        assert_eq!(first.logical_type, "string");
        assert_eq!(first.max_length, Some(50));
        assert!(first.required);
        let dob = op.fields.iter().find(|f| f.name == "dateOfBirth").unwrap();
        assert_eq!(dob.format.as_deref(), Some("date"));
        assert!(!dob.required);
        let query = op.fields.iter().find(|f| f.name == "include").unwrap();
        assert_eq!(query.location, Location::Query);
    }

    #[test]
    fn openapi_resolves_component_refs() {
        let doc = serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/x": {
                    "post": {
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {"$ref": "#/components/schemas/Thing"}
                                }
                            }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "Thing": {
                        "type": "object",
                        "required": ["id"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"}
                        }
                    }
                }
            }
        });
        let ops = extract_ops_from_openapi(&doc, "api.yaml");
        let op = &ops[0];
        assert_eq!(op.fields.len(), 1);
        assert_eq!(op.fields[0].name, "id");
        assert_eq!(op.fields[0].format.as_deref(), Some("uuid"));
        assert!(op.fields[0].required);
    }

    #[test]
    fn markdown_extracts_op_with_parameters() {
        let md = r#"
## POST /cardholders

**Summary**: Creates a new cardholder with the provided information.

### Parameters

#### Content-Type

- **Location**: `header`
- **Required**: True
- **Type**: `string`

#### firstName

- **Location**: body
- **Required**: True
- **Type**: string
- **MaxLength**: 50

#### dateOfBirth

- **Location**: body
- **Required**: False
- **Type**: string
- **Format**: date
"#;
        let ops = extract_ops_from_markdown(md, "spec.md");
        assert_eq!(ops.len(), 1);
        let op = &ops[0];
        assert_eq!(op.method.as_deref(), Some("POST"));
        assert_eq!(op.path.as_deref(), Some("/cardholders"));
        assert_eq!(op.summary.as_deref(), Some("Creates a new cardholder with the provided information."));
        assert_eq!(op.fields.len(), 3);
        let first = op.fields.iter().find(|f| f.name == "firstName").unwrap();
        assert_eq!(first.logical_type, "string");
        assert_eq!(first.max_length, Some(50));
        assert!(first.required);
        assert_eq!(first.location, Location::Body);
        let dob = op.fields.iter().find(|f| f.name == "dateOfBirth").unwrap();
        assert_eq!(dob.format.as_deref(), Some("date"));
        assert!(!dob.required);
    }

    #[test]
    fn markdown_ignores_backticks_around_values() {
        // The dt specs wrap values in backticks: `string`, `header`, etc.
        // bullet_kv is naïve so we normalise just by matching substrings.
        let line = "- **Location**: `header`";
        let v = bullet_kv(line, "Location").unwrap();
        // Value is "`header`" — fine, Location parser lowercases and strips.
        assert_eq!(v, "`header`");
    }

    #[test]
    fn markdown_strips_backticks_in_location_and_type() {
        let md = r#"
## GET /x

### Parameters

#### q

- **Location**: `header`
- **Required**: True
- **Type**: `string`
"#;
        let ops = extract_ops_from_markdown(md, "s.md");
        assert_eq!(ops.len(), 1);
        let f = &ops[0].fields[0];
        assert_eq!(f.location, Location::Header);
        assert_eq!(f.logical_type, "string");
        assert!(f.required);
    }

    #[test]
    fn markdown_two_headings_two_ops() {
        let md = r#"
## POST /a

### Parameters

#### one

- **Location**: body
- **Required**: True
- **Type**: string

## GET /b

### Parameters

#### two

- **Location**: query
- **Required**: False
- **Type**: integer
"#;
        let ops = extract_ops_from_markdown(md, "s.md");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].slug, "post-a");
        assert_eq!(ops[1].slug, "get-b");
    }
}
