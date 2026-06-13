//! OpenAPI 3.0.3 slice renderer — emits one `openapi.yaml` per bundle.
//!
//! Inputs:
//!   - `bundle.toml::[[endpoints]]` — gives path, http_method, shape,
//!     description, response_dto, request_dto, route_params
//!   - Rendered C# DTO files (Models/Response/Bundle/*.cs and, for command
//!     shapes, Models/Request/Bundle/*.cs) — gives the property list
//!     used to materialise `components.schemas.<DtoName>`
//!   - Template fragments under `profiles/<P>/openapi/_templates/`
//!     (bundle-header, common-headers, query-list-params, api-error-schema)
//!
//! Output: one `openapi.yaml` file per bundle.
//!
//! Concatenation into the project-wide spec happens in a separate
//! `bundle-docs` command which merges every bundle's slice.
//!
//! C# → OpenAPI type mapping:
//!   - `Guid` / `Guid?`           → string + format uuid
//!   - `string`                   → string
//!   - `int` / `int?` / `long`    → integer
//!   - `decimal` / `decimal?`     → number
//!   - `bool` / `bool?`           → boolean
//!   - `DateTime` / `DateTime?`   → string + format date-time
//!   - `List<T>` / `IEnumerable<T>` → array of items: <T>
//!   - `dynamic` / `object`       → object (description "Schema varies")
//!   - anything PascalCase else   → $ref to a nested component (TODO: not implemented yet — emitted as `object`)

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::cs_dto_audit::{parse_dto_file, DtoFile, DtoProperty};
use super::manifest::{Bundle, EndpointRow, Profile};

pub fn render_bundle_with_templates(
    bundle: &Bundle,
    profile: &Profile,
    templates_root: &Path,
    workspace_root: &Path,
    out_path: &Path,
) -> Result<(), String> {
    let templates = TemplateSet::load(templates_root)?;
    let api_version = profile
        .bruno_defaults
        .api_version
        .clone()
        .unwrap_or_else(|| "1.0.0".to_string());

    // Resolve where rendered Response DTOs live for this profile/bundle.
    let response_dto_root_tpl = profile
        .generated_paths
        .cs_response_dto_root
        .as_deref()
        .ok_or_else(|| "profile.toml: generated_paths.cs_response_dto_root not set".to_string())?;
    let response_dto_root = workspace_root.join(response_dto_root_tpl).join(&bundle.bundle);

    // Optional: Request DTO root (used by command shape only).
    let request_dto_root: Option<PathBuf> = profile
        .generated_paths
        .cs_request_dto_root
        .as_deref()
        .map(|tpl| workspace_root.join(tpl).join(&bundle.bundle));

    let mut yaml = String::new();

    // ---- header ----------------------------------------------------------
    yaml.push_str(
        &templates
            .bundle_header
            .replace("{{bundle}}", &bundle.bundle)
            .replace("{{api_version}}", &api_version),
    );

    // ---- paths -----------------------------------------------------------
    // Group endpoints by path so OpenAPI's "one path key, multiple verbs"
    // shape is honoured. Without this, two endpoints sharing a path (e.g.
    // GET + PUT /account/{accountId}) emit as duplicate top-level keys,
    // which fails `bundle-docs` concat.
    yaml.push_str("paths:\n");
    let mut referenced_dtos: BTreeSet<String> = BTreeSet::new();
    let mut by_path: std::collections::BTreeMap<String, Vec<&EndpointRow>> =
        std::collections::BTreeMap::new();
    for ep in &bundle.endpoints {
        by_path.entry(ep.path.clone()).or_default().push(ep);
    }
    for (path, eps) in &by_path {
        yaml.push_str(&format!("  {}:\n", path));
        for ep in eps {
            let body = render_endpoint_verb(ep, &templates, &api_version, &bundle.bundle)?;
            yaml.push_str(&body);
            if let Some(dto) = ep.response_dto.as_ref() {
                if !dto.is_empty() {
                    referenced_dtos.insert(dto.clone());
                }
            }
            if let Some(dto) = ep.request_dto.as_ref() {
                if !dto.is_empty() {
                    referenced_dtos.insert(dto.clone());
                }
            }
        }
    }

    // ---- components.schemas ---------------------------------------------
    yaml.push_str("components:\n  schemas:\n");
    // ApiError first — every slice carries it; concatenator dedupes.
    for line in indent(&templates.api_error_schema, 4).lines() {
        yaml.push_str(line);
        yaml.push('\n');
    }

    // Then one schema per referenced DTO.
    for dto_name in &referenced_dtos {
        let dto = locate_and_parse_dto(dto_name, &response_dto_root, request_dto_root.as_deref())?;
        let schema_yaml = dto_to_schema(&dto, dto_name);
        for line in indent(&schema_yaml, 4).lines() {
            yaml.push_str(line);
            yaml.push('\n');
        }
    }

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(out_path, yaml.as_bytes())
        .map_err(|e| format!("write {}: {}", out_path.display(), e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Endpoint rendering
// ---------------------------------------------------------------------------

/// Render one verb block (the `    <verb>:` chunk and everything under
/// it). The caller is responsible for emitting the `  <path>:` line above
/// — multiple verbs may share one path, so the path key is emitted once
/// per path, not once per endpoint.
fn render_endpoint_verb(
    ep: &EndpointRow,
    templates: &TemplateSet,
    api_version: &str,
    bundle: &str,
) -> Result<String, String> {
    let verb = ep
        .method
        .as_deref()
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "get".to_string());
    let summary = endpoint_summary(ep);
    let description = ep
        .description
        .as_deref()
        .map(|d| d.trim().replace('\n', " "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Endpoint emitted by the proc framework from `bundles/{}/bundle.toml`.", bundle));

    let operation_id = format!(
        "{}_{}_{}",
        verb,
        ep.schema.clone().unwrap_or_else(|| "_".to_string()),
        ep.proc_name()
    );

    let mut out = String::new();
    out.push_str(&format!("    {}:\n", verb));
    out.push_str(&format!("      tags: [{}]\n", bundle));
    out.push_str(&format!("      summary: {}\n", yaml_string(&summary)));
    out.push_str(&format!("      description: {}\n", yaml_string(&description)));
    out.push_str(&format!("      operationId: {}\n", operation_id));
    out.push_str(&format!("      x-bundle: {}\n", bundle));
    out.push_str(&format!("      x-shape: {}\n", ep.shape));

    // Parameters: common headers + route params + shape-specific query params.
    out.push_str("      parameters:\n");
    for line in indent(&templates.common_headers, 6).lines() {
        out.push_str(line);
        out.push('\n');
    }
    // Substitute api-version literal into the common-headers block.
    out = out.replace("{{api_version}}", api_version);

    // Route params from bundle.toml route_params.
    for p in &ep.route_params {
        let name = strip_at_prefix(&p.name);
        let openapi_type = sql_type_to_openapi(&p.ty);
        out.push_str(&format!("      - name: {}\n", name));
        out.push_str("        in: path\n");
        out.push_str("        required: true\n");
        out.push_str(&format!("        description: 'Path parameter: {}'\n", name));
        out.push_str("        schema:\n");
        out.push_str(&format!("          type: {}\n", openapi_type.ty));
        if let Some(f) = openapi_type.format {
            out.push_str(&format!("          format: {}\n", f));
        }
    }

    // Shape-specific query params.
    if ep.shape == "query-list" {
        for line in indent(&templates.query_list_params, 6).lines() {
            out.push_str(line);
            out.push('\n');
        }
    }

    // Request body for command shape.
    if ep.shape == "command" {
        if let Some(req_dto) = ep.request_dto.as_ref().filter(|s| !s.is_empty()) {
            out.push_str("      requestBody:\n");
            out.push_str("        required: true\n");
            out.push_str(&format!(
                "        description: Payload for {} {}.\n",
                verb.to_uppercase(),
                ep.path
            ));
            out.push_str("        content:\n");
            out.push_str("          application/json:\n");
            out.push_str("            schema:\n");
            out.push_str(&format!(
                "              $ref: '#/components/schemas/{}'\n",
                req_dto
            ));
        }
    }

    // Responses.
    out.push_str("      responses:\n");
    let success_code = if ep.action_type == "CREATE" { "201" } else { "200" };
    out.push_str(&format!("        '{}':\n", success_code));
    out.push_str("          description: Successful response\n");
    out.push_str("          content:\n");
    out.push_str("            application/json:\n");
    out.push_str("              schema:\n");
    if let Some(resp_dto) = ep.response_dto.as_ref().filter(|s| !s.is_empty()) {
        out.push_str(&format!(
            "                $ref: '#/components/schemas/{}'\n",
            resp_dto
        ));
    } else {
        out.push_str("                type: object\n");
        out.push_str("                description: Response shape not declared in bundle.toml\n");
    }
    out.push_str("        '400':\n");
    out.push_str("          description: Error response\n");
    out.push_str("          content:\n");
    out.push_str("            application/json:\n");
    out.push_str("              schema:\n");
    out.push_str("                $ref: '#/components/schemas/ApiError'\n");

    Ok(out)
}

fn endpoint_summary(ep: &EndpointRow) -> String {
    let verb = match ep.action_type.as_str() {
        "CREATE" => "Create",
        "GET" => match ep.shape.as_str() {
            "query-list" => "List all",
            _ => "Get",
        },
        "UPDATE" => "Update",
        "DELETE" => "Delete",
        "READ" => match ep.shape.as_str() {
            "query-list" => "List all",
            _ => "Get",
        },
        _ => ep.action_type.as_str(),
    };
    format!("{} {}", verb, ep.entity)
}

// ---------------------------------------------------------------------------
// DTO discovery + schema emission
// ---------------------------------------------------------------------------

fn locate_and_parse_dto(
    dto_name: &str,
    response_root: &Path,
    request_root: Option<&Path>,
) -> Result<DtoFile, String> {
    let response_candidate = response_root.join(format!("{}.cs", dto_name));
    if response_candidate.exists() {
        return parse_dto_file(&response_candidate);
    }
    if let Some(req_root) = request_root {
        let request_candidate = req_root.join(format!("{}.cs", dto_name));
        if request_candidate.exists() {
            return parse_dto_file(&request_candidate);
        }
    }
    // Fall back to an empty DtoFile so the slice still emits — schema will be
    // a placeholder.
    Ok(DtoFile {
        class_name: dto_name.to_string(),
        ..Default::default()
    })
}

fn dto_to_schema(dto: &DtoFile, dto_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}:\n", dto_name));
    out.push_str("  type: object\n");
    out.push_str(&format!(
        "  description: Generated schema for {} (derived from the rendered C# DTO at render time).\n",
        dto_name
    ));
    if dto.properties.is_empty() {
        out.push_str("  properties: {}\n");
        return out;
    }
    out.push_str("  properties:\n");
    for p in &dto.properties {
        out.push_str(&format!("    {}:\n", camel_case(&p.name)));
        let mapped = cs_type_to_openapi(&p.ty);
        out.push_str(&format!("      type: {}\n", mapped.ty));
        if let Some(format) = mapped.format {
            out.push_str(&format!("      format: {}\n", format));
        }
        if let Some(items) = mapped.items {
            out.push_str("      items:\n");
            out.push_str(&format!("        type: {}\n", items.ty));
            if let Some(f) = items.format {
                out.push_str(&format!("        format: {}\n", f));
            }
        }
        if mapped.nullable {
            out.push_str("      nullable: true\n");
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct OpenApiType {
    ty: String,
    format: Option<String>,
    items: Option<Box<OpenApiType>>,
    nullable: bool,
}

fn cs_type_to_openapi(cs_type: &str) -> OpenApiType {
    let mut nullable = false;
    let base = cs_type.trim();
    let base = if let Some(stripped) = base.strip_suffix('?') {
        nullable = true;
        stripped.trim()
    } else {
        base
    };

    // List<T> / IEnumerable<T> / IList<T> / IReadOnlyList<T> / T[]
    if let Some(inner) = parse_collection(base) {
        let inner_mapped = cs_type_to_openapi(inner);
        return OpenApiType {
            ty: "array".into(),
            format: None,
            items: Some(Box::new(inner_mapped)),
            nullable,
        };
    }

    let (ty, format) = match base {
        "Guid" => ("string", Some("uuid")),
        "string" => ("string", None),
        "DateTime" | "DateTimeOffset" => ("string", Some("date-time")),
        "DateOnly" => ("string", Some("date")),
        "int" | "Int32" | "short" | "Int16" => ("integer", Some("int32")),
        "long" | "Int64" => ("integer", Some("int64")),
        "decimal" => ("number", None),
        "double" => ("number", Some("double")),
        "float" => ("number", Some("float")),
        "bool" | "Boolean" => ("boolean", None),
        "object" | "dynamic" => ("object", None),
        _ => ("object", None), // user-defined types: emit as opaque object (TODO: $ref)
    };

    OpenApiType {
        ty: ty.into(),
        format: format.map(String::from),
        items: None,
        nullable,
    }
}

fn parse_collection(ty: &str) -> Option<&str> {
    for prefix in ["List<", "IList<", "IReadOnlyList<", "IEnumerable<", "ICollection<"] {
        if let Some(rest) = ty.strip_prefix(prefix) {
            if let Some(inner) = rest.strip_suffix('>') {
                return Some(inner.trim());
            }
        }
    }
    if let Some(stripped) = ty.strip_suffix("[]") {
        return Some(stripped.trim());
    }
    None
}

#[derive(Debug, Clone, Default)]
struct OpenApiSimpleType {
    ty: String,
    format: Option<String>,
}

fn sql_type_to_openapi(sql_type: &str) -> OpenApiSimpleType {
    let upper = sql_type.to_uppercase();
    match upper.as_str() {
        "UNIQUEIDENTIFIER" => OpenApiSimpleType {
            ty: "string".into(),
            format: Some("uuid".into()),
        },
        "INT" | "SMALLINT" => OpenApiSimpleType {
            ty: "integer".into(),
            format: Some("int32".into()),
        },
        "BIGINT" => OpenApiSimpleType {
            ty: "integer".into(),
            format: Some("int64".into()),
        },
        "BIT" => OpenApiSimpleType {
            ty: "boolean".into(),
            format: None,
        },
        u if u.starts_with("DECIMAL") || u.starts_with("NUMERIC") || u.starts_with("MONEY") => {
            OpenApiSimpleType {
                ty: "number".into(),
                format: None,
            }
        }
        u if u.starts_with("DATE") || u.starts_with("TIME") => OpenApiSimpleType {
            ty: "string".into(),
            format: Some("date-time".into()),
        },
        _ => OpenApiSimpleType {
            ty: "string".into(),
            format: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct TemplateSet {
    bundle_header: String,
    common_headers: String,
    query_list_params: String,
    api_error_schema: String,
}

impl TemplateSet {
    fn load(root: &Path) -> Result<Self, String> {
        let read = |name: &str| -> Result<String, String> {
            let p = root.join(name);
            std::fs::read_to_string(&p)
                .map_err(|e| format!("read template {}: {}", p.display(), e))
        };
        Ok(Self {
            bundle_header: read("bundle-header.yaml")?,
            common_headers: read("common-headers.yaml")?,
            query_list_params: read("query-list-params.yaml")?,
            api_error_schema: read("api-error-schema.yaml")?,
        })
    }
}

fn indent(text: &str, n_spaces: usize) -> String {
    let pad: String = " ".repeat(n_spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                line.to_string()
            } else {
                format!("{}{}", pad, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn camel_case(name: &str) -> String {
    // PascalCase → camelCase ("FirstName" → "firstName"). Leaves the rest of
    // the identifier alone (assumes the caller already trimmed bracket/etc.).
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn strip_at_prefix(s: &str) -> String {
    s.trim_start_matches('@')
        .trim_start_matches('u') // common Hungarian: @uAccountId → AccountId
        .chars()
        .enumerate()
        .map(|(i, c)| if i == 0 { c.to_ascii_lowercase() } else { c })
        .collect()
}

fn yaml_string(s: &str) -> String {
    // Quote if contains characters that confuse YAML scalar parsing.
    if s.contains(':') || s.contains('#') || s.starts_with(' ') {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        s.to_string()
    }
}

#[allow(dead_code)]
fn _unused_dto_property_hint(_p: &DtoProperty) {
    // Marker so DtoProperty stays in scope for future TODOs (e.g. honouring
    // [Required] / [JsonRequired] attributes → required field list).
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cs_type_guid() {
        let m = cs_type_to_openapi("Guid");
        assert_eq!(m.ty, "string");
        assert_eq!(m.format.as_deref(), Some("uuid"));
        assert!(!m.nullable);
    }

    #[test]
    fn cs_type_nullable_int() {
        let m = cs_type_to_openapi("int?");
        assert_eq!(m.ty, "integer");
        assert!(m.nullable);
    }

    #[test]
    fn cs_type_list_string() {
        let m = cs_type_to_openapi("List<string>");
        assert_eq!(m.ty, "array");
        assert!(m.items.is_some());
        assert_eq!(m.items.as_ref().unwrap().ty, "string");
    }

    #[test]
    fn camel_case_pascal() {
        assert_eq!(camel_case("FirstName"), "firstName");
        assert_eq!(camel_case("ID"), "iD");
    }
}
