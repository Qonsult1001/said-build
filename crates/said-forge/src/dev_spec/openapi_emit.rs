//! OpenAPI emission from `DevSpecEndpoint`. Produces operations that
//! match global standards (tags, parameters, requestBody, responses)
//! and a `components/schemas/` block keyed by entity-derived names.
//!
//! See:
//!   - `docs/superpowers/specs/2026-04-29-openapi-compliance-and-erd-redesign.md`
//!   - Reference: `dtcard/4-expectations/api-specification_31 March 2026.yml`

use crate::dev_spec::types::{DevSpecEndpoint, DevSpecSchema};
use crate::OpenApiStandard;
use serde_yaml::{Mapping, Value};

/// Standard headers every operation carries — request tracing + auth.
/// Driven by `standard.parameters.standard_headers`. Names + required
/// status come from the `required` / `optional` arrays; per-header
/// type/format/description from the matching subtable.
pub fn standard_headers(standard: &OpenApiStandard) -> Vec<Mapping> {
    let rules = &standard.parameters.standard_headers;
    let mut out = Vec::new();
    let names: Vec<&str> = rules.required.iter()
        .chain(rules.optional.iter())
        .map(|s| s.as_str())
        .collect();
    for name in names {
        let def = rules.headers.get(name);
        let required = rules.required.iter().any(|r| r == name);
        let mut m = Mapping::new();
        m.insert(Value::String("name".into()), Value::String(name.to_string()));
        m.insert(Value::String("in".into()), Value::String("header".into()));
        if required {
            m.insert(Value::String("required".into()), Value::Bool(true));
        }
        if let Some(d) = def.and_then(|d| d.description.clone()) {
            m.insert(Value::String("description".into()), Value::String(d));
        }
        let mut schema = Mapping::new();
        schema.insert(Value::String("type".into()),
            Value::String(def.map(|d| d.r#type.clone()).unwrap_or_else(|| "string".into())));
        if let Some(f) = def.and_then(|d| d.format.clone()) {
            schema.insert(Value::String("format".into()), Value::String(f));
        }
        if let Some(e) = def.and_then(|d| d.example.clone()) {
            schema.insert(Value::String("example".into()), Value::String(e));
        }
        m.insert(Value::String("schema".into()), Value::Mapping(schema));
        out.push(m);
    }
    out
}

/// Tag extracted from `<Folder>/<file>.md` source path. Folder name
/// is the domain (Cardholders, Accounts, etc.); we singularise it.
pub fn tag_from_source_file(source: &str) -> String {
    let folder = source.split(['/', '\\']).next().unwrap_or("");
    if folder.is_empty() || folder.ends_with(".md") {
        return "Default".into();
    }
    pascal_singular(folder)
}

fn pascal_singular(word: &str) -> String {
    let mut chars = word.chars();
    let pascal: String = match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    };
    if let Some(stem) = pascal.strip_suffix("ies") {
        format!("{stem}y")
    } else if pascal.ends_with('s') && pascal.len() > 1 {
        pascal[..pascal.len() - 1].to_string()
    } else {
        pascal
    }
}

/// Operation-name derivation. Filename `POST-cardholders-createCardholder.md`
/// → trailing camelCase token after the last `-` is the explicit op name
/// (`CreateCardholder`). Fallback: `<Verb><PascalSingular(last-collection)>`,
/// with `List` instead of `Get` when the URL is a bare collection
/// (no `{id}` after the last segment) — distinguishes list endpoints
/// from by-id GETs so their schemas don't collide.
fn operation_name(ep: &DevSpecEndpoint) -> String {
    let stem = ep.source_file
        .rsplit(['/', '\\']).next().unwrap_or("")
        .strip_suffix(".md").unwrap_or("");
    let parts: Vec<&str> = stem.split('-').collect();
    if let Some(last) = parts.last() {
        if last.chars().any(|c| c.is_ascii_uppercase()) && !last.starts_with('_') {
            return capitalise_first(last);
        }
    }
    let last_collection = ep.path
        .split('/').rev().find(|s| !s.is_empty() && !s.starts_with('{'))
        .unwrap_or("Resource");
    // Detect bare-collection GET (path ends with non-template segment).
    let path_ends_with_id = ep.path.split('/').last()
        .map(|s| s.starts_with('{'))
        .unwrap_or(false);
    let verb_pascal = if ep.method.eq_ignore_ascii_case("GET") && !path_ends_with_id {
        "List".to_string()
    } else {
        capitalise_first(&ep.method.to_lowercase())
    };
    format!("{verb_pascal}{}", pascal_singular(last_collection))
}

fn capitalise_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

fn request_schema_name(ep: &DevSpecEndpoint) -> String {
    format!("{}Request", operation_name(ep))
}

fn response_schema_name(ep: &DevSpecEndpoint) -> String {
    format!("{}Response", operation_name(ep))
}

/// Build the YAML mapping for one operation, populated from a matching
/// `DevSpecEndpoint`. Records the proc binding in `description` for
/// traceability. Standard-header set, response codes, content-type
/// variants, and shared error schema all come from `standard`.
pub fn build_operation_yaml(
    ep: &DevSpecEndpoint,
    proc_full: &str,
    standard: &OpenApiStandard,
) -> Value {
    let mut op = Mapping::new();
    op.insert(Value::String("tags".into()),
        Value::Sequence(vec![Value::String(tag_from_source_file(&ep.source_file))]));
    op.insert(Value::String("summary".into()),
        Value::String(if ep.summary.is_empty() {
            format!("{} {}", ep.method, ep.path)
        } else {
            ep.summary.clone()
        }));
    op.insert(Value::String("description".into()),
        Value::String(format!(
            "Implemented by `{}`. Endpoint registered in the API rule registry and bound by the 6-gate matcher.",
            proc_full
        )));
    op.insert(Value::String("operationId".into()),
        Value::String(format!("{}_{}",
            ep.method.to_lowercase(),
            proc_full.replace('.', "_"))));
    op.insert(Value::String("x-source".into()),
        Value::String("registry-mapped".into()));

    // Parameters: standard headers + path/query/header params from the
    // Dev Spec. UUID format on path params keyed off `uuid_suffix`;
    // query/header params carry default/min/max from their schema.
    //
    // Header params declared in the Dev Spec are filtered against the
    // workspace standard_headers list to avoid emitting the same
    // tracing header twice (once from the standard, once per file).
    let mut params: Vec<Value> = standard_headers(standard).into_iter()
        .map(Value::Mapping).collect();
    let standard_header_names: std::collections::HashSet<String> = standard
        .parameters.standard_headers.required.iter()
        .chain(standard.parameters.standard_headers.optional.iter())
        .map(|s| s.to_lowercase())
        .collect();
    let uuid_suffix = &standard.parameters.path_params.uuid_suffix;
    for pp in &ep.path_params {
        // Determine the param's location. Empty `location` = legacy
        // path-param construction (extract_path_params). Anything else
        // came from `extract_parameters_section`.
        let location = if pp.location.is_empty() { "path" } else { pp.location.as_str() };
        // Skip header params already covered by the standard.
        if location == "header" && standard_header_names.contains(&pp.name.to_lowercase()) {
            continue;
        }

        let mut m = Mapping::new();
        m.insert(Value::String("name".into()), Value::String(pp.name.clone()));
        m.insert(Value::String("in".into()), Value::String(location.to_string()));
        // Path params are always required; others honour the parsed flag.
        let required = if location == "path" { true } else { pp.required };
        m.insert(Value::String("required".into()), Value::Bool(required));
        m.insert(Value::String("description".into()),
            Value::String(if pp.description.is_empty() {
                match location {
                    "path" => format!("Path parameter: {}", pp.name),
                    "query" => format!("Query parameter: {}", pp.name),
                    "header" => format!("Header parameter: {}", pp.name),
                    _ => format!("Parameter: {}", pp.name),
                }
            } else {
                pp.description.clone()
            }));

        let mut schema = Mapping::new();
        // Type:
        //   - explicit `pp.ty` (from query/header schema) takes priority
        //   - path params default to `string`
        let ty = if pp.ty.is_empty() { "string".to_string() } else { pp.ty.clone() };
        schema.insert(Value::String("type".into()), Value::String(ty.clone()));
        // UUID format on path-params matching the suffix.
        if location == "path"
            && (pp.ty == "uuid" || pp.name.ends_with(uuid_suffix.as_str()))
        {
            schema.insert(Value::String("format".into()), Value::String("uuid".into()));
        }
        if let Some(d) = &pp.default {
            // Try integer first, fall back to string.
            let v = d.parse::<i64>()
                .map(|i| Value::Number(i.into()))
                .unwrap_or_else(|_| Value::String(d.clone()));
            schema.insert(Value::String("default".into()), v);
        }
        if let Some(d) = &pp.minimum {
            let v = d.parse::<i64>()
                .map(|i| Value::Number(i.into()))
                .unwrap_or_else(|_| Value::String(d.clone()));
            schema.insert(Value::String("minimum".into()), v);
        }
        if let Some(d) = &pp.maximum {
            let v = d.parse::<i64>()
                .map(|i| Value::Number(i.into()))
                .unwrap_or_else(|_| Value::String(d.clone()));
            schema.insert(Value::String("maximum".into()), v);
        }
        if let Some(ex) = &pp.example {
            schema.insert(Value::String("example".into()), Value::String(ex.clone()));
        }
        m.insert(Value::String("schema".into()), Value::Mapping(schema));
        params.push(Value::Mapping(m));
    }
    op.insert(Value::String("parameters".into()), Value::Sequence(params));

    // Request body — when present. Content-type variants from config.
    if ep.request_body.is_some() {
        let mut rb = Mapping::new();
        rb.insert(Value::String("required".into()), Value::Bool(true));
        rb.insert(Value::String("description".into()),
            Value::String(format!("Payload for {} {}.", ep.method, ep.path)));
        let mut content = Mapping::new();
        let schema_ref = format!("#/components/schemas/{}", request_schema_name(ep));
        for ct in &standard.content_types.request {
            let mut media = Mapping::new();
            let mut schema = Mapping::new();
            schema.insert(Value::String("$ref".into()), Value::String(schema_ref.clone()));
            media.insert(Value::String("schema".into()), Value::Mapping(schema));
            content.insert(Value::String(ct.clone()), Value::Mapping(media));
        }
        rb.insert(Value::String("content".into()), Value::Mapping(content));
        op.insert(Value::String("requestBody".into()), Value::Mapping(rb));
    }

    // Responses: success + error codes from config; error $ref points
    // at the configured shared_error_schema.
    let mut responses = Mapping::new();

    let mut rsucc = Mapping::new();
    rsucc.insert(Value::String("description".into()),
        Value::String(standard.responses.success_description.clone()));
    let mut rsucc_content = Mapping::new();
    for ct in &standard.content_types.response {
        let mut media = Mapping::new();
        let mut schema = Mapping::new();
        schema.insert(Value::String("$ref".into()),
            Value::String(format!("#/components/schemas/{}", response_schema_name(ep))));
        media.insert(Value::String("schema".into()), Value::Mapping(schema));
        rsucc_content.insert(Value::String(ct.clone()), Value::Mapping(media));
    }
    rsucc.insert(Value::String("content".into()), Value::Mapping(rsucc_content));
    responses.insert(Value::String(standard.responses.success_code.clone()),
        Value::Mapping(rsucc));

    let mut rerr = Mapping::new();
    rerr.insert(Value::String("description".into()),
        Value::String(standard.responses.error_description.clone()));
    let mut rerr_content = Mapping::new();
    for ct in &standard.content_types.response {
        let mut media = Mapping::new();
        let mut schema = Mapping::new();
        schema.insert(Value::String("$ref".into()),
            Value::String(format!("#/components/schemas/{}",
                standard.responses.shared_error_schema)));
        media.insert(Value::String("schema".into()), Value::Mapping(schema));
        rerr_content.insert(Value::String(ct.clone()), Value::Mapping(media));
    }
    rerr.insert(Value::String("content".into()), Value::Mapping(rerr_content));
    responses.insert(Value::String(standard.responses.error_code.clone()),
        Value::Mapping(rerr));

    op.insert(Value::String("responses".into()), Value::Mapping(responses));

    Value::Mapping(op)
}

/// Build the `components/schemas/` mapping. Includes the configured
/// shared error schema (when `components.include_api_error` is true) +
/// per-endpoint `<OpName>Request` and `<OpName>Response` schemas.
pub fn emit_components_schemas(
    endpoints: &[DevSpecEndpoint],
    standard: &OpenApiStandard,
) -> Value {
    let mut schemas = Mapping::new();

    // Shared error component, name controlled by config.
    if standard.components.include_api_error {
        let mut api_error = Mapping::new();
        api_error.insert(Value::String("type".into()), Value::String("object".into()));
        let mut props = Mapping::new();
        let mut code = Mapping::new();
        code.insert(Value::String("type".into()), Value::String("string".into()));
        code.insert(Value::String("description".into()),
            Value::String("Error code generated by the application.".into()));
        code.insert(Value::String("nullable".into()), Value::Bool(true));
        props.insert(Value::String("code".into()), Value::Mapping(code));
        let mut desc = Mapping::new();
        desc.insert(Value::String("type".into()), Value::String("string".into()));
        desc.insert(Value::String("description".into()),
            Value::String("Short description of the error code.".into()));
        desc.insert(Value::String("nullable".into()), Value::Bool(true));
        props.insert(Value::String("description".into()), Value::Mapping(desc));
        api_error.insert(Value::String("properties".into()), Value::Mapping(props));
        api_error.insert(Value::String("description".into()),
            Value::String("Error model returned by the application.".into()));
        schemas.insert(
            Value::String(standard.responses.shared_error_schema.clone()),
            Value::Mapping(api_error),
        );
    }

    // Per-endpoint schemas.
    for ep in endpoints {
        if let Some(body) = &ep.request_body {
            schemas.insert(
                Value::String(request_schema_name(ep)),
                schema_to_yaml(body));
        }
        let resp_name = response_schema_name(ep);
        if !schemas.contains_key(&Value::String(resp_name.clone())) {
            schemas.insert(
                Value::String(resp_name),
                response_envelope_yaml(ep, standard));
        }
    }

    Value::Mapping(schemas)
}

/// Convert a `DevSpecSchema` (parsed from markdown) to OpenAPI YAML.
fn schema_to_yaml(s: &DevSpecSchema) -> Value {
    let mut m = Mapping::new();
    m.insert(Value::String("type".into()), Value::String(s.ty.clone()));
    if let Some(f) = &s.format {
        m.insert(Value::String("format".into()), Value::String(f.clone()));
    }
    if let Some(d) = &s.description {
        m.insert(Value::String("description".into()), Value::String(d.clone()));
    }
    if let Some(ex) = &s.example {
        m.insert(Value::String("example".into()), Value::String(ex.clone()));
    }
    if !s.properties.is_empty() {
        let mut props = Mapping::new();
        for (k, v) in &s.properties {
            props.insert(Value::String(k.clone()), schema_to_yaml(v));
        }
        m.insert(Value::String("properties".into()), Value::Mapping(props));
    }
    if let Some(items) = &s.items {
        m.insert(Value::String("items".into()), schema_to_yaml(items));
    }
    Value::Mapping(m)
}

/// Generic response envelope: tracking IDs + dateTime + result + error.
/// Mirrors the Dev Spec's standard response shape:
///   { requestId, correlationId, responseId, dateTime, result, error }
///
/// E2 — when the Dev Spec declared a full response schema (typically the
/// whole envelope), pull two things from it:
///   1. `result` — its `properties` are inlined under our envelope's
///      `result` so consumers see the typed payload.
///   2. Tracking-field examples — `requestId`/`correlationId`/`responseId`/`dateTime`
///      examples from the Dev Spec are copied onto the matching envelope
///      fields.
///
/// `error` always points at the central `ApiError` schema for consistency
/// across endpoints — Dev Spec's own inline error structure is ignored
/// here in favour of the shared component.
fn response_envelope_yaml(ep: &DevSpecEndpoint, standard: &OpenApiStandard) -> Value {
    // Pull tracking-field examples from the Dev Spec if it provided
    // a response schema with envelope properties at the top level.
    let dev_envelope = ep.response_body.as_ref();
    let example_for = |field: &str| -> Option<String> {
        dev_envelope
            .and_then(|s| s.properties.get(field))
            .and_then(|n| n.example.clone())
    };

    let mut m = Mapping::new();
    m.insert(Value::String("type".into()), Value::String("object".into()));
    m.insert(Value::String("description".into()),
        Value::String(format!("Response envelope for {} {}.", ep.method, ep.path)));
    let mut props = Mapping::new();
    for tracking in ["requestId", "correlationId", "responseId"] {
        let mut p = Mapping::new();
        p.insert(Value::String("type".into()), Value::String("string".into()));
        p.insert(Value::String("format".into()), Value::String("uuid".into()));
        if let Some(ex) = example_for(tracking) {
            p.insert(Value::String("example".into()), Value::String(ex));
        }
        props.insert(Value::String(tracking.into()), Value::Mapping(p));
    }
    let mut date_time = Mapping::new();
    date_time.insert(Value::String("type".into()), Value::String("string".into()));
    date_time.insert(Value::String("format".into()), Value::String("date-time".into()));
    if let Some(ex) = example_for("dateTime") {
        date_time.insert(Value::String("example".into()), Value::String(ex));
    }
    props.insert(Value::String("dateTime".into()), Value::Mapping(date_time));

    // `result`: the operation's payload.
    //   1. Dev Spec carries a top-level `result` field with `properties`
    //      → inline those properties (E2).
    //   2. Dev Spec response is a flat object (no envelope wrapper)
    //      → use the whole schema as `result`.
    //   3. No Dev Spec response → free-form nullable object placeholder.
    let result_node = match dev_envelope {
        Some(rb) => match rb.properties.get("result") {
            Some(inner) if !inner.properties.is_empty() => {
                schema_to_yaml(inner)
            }
            Some(inner) => schema_to_yaml(inner),
            None => schema_to_yaml(rb),
        },
        None => {
            let mut r = Mapping::new();
            r.insert(Value::String("type".into()), Value::String("object".into()));
            r.insert(Value::String("nullable".into()), Value::Bool(true));
            r.insert(Value::String("description".into()),
                Value::String("Operation payload (null on error).".into()));
            Value::Mapping(r)
        }
    };
    props.insert(Value::String("result".into()), result_node);

    let mut error = Mapping::new();
    error.insert(Value::String("$ref".into()),
        Value::String(format!("#/components/schemas/{}",
            standard.responses.shared_error_schema)));
    props.insert(Value::String("error".into()), Value::Mapping(error));
    m.insert(Value::String("properties".into()), Value::Mapping(props));
    Value::Mapping(m)
}
