//! Workspace-level OpenAPI generation standard.
//!
//! See `docs/superpowers/specs/2026-04-29-openapi-standard-config.md`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiStandard {
    #[serde(default)]
    pub paths: PathRules,
    #[serde(default)]
    pub brace_style: BraceStyleRule,
    #[serde(default)]
    pub parameters: ParameterRules,
    #[serde(default)]
    pub responses: ResponseRules,
    #[serde(default)]
    pub components: ComponentRules,
    #[serde(default)]
    pub content_types: ContentTypeRules,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRules {
    #[serde(default = "default_param_casing")]
    pub param_casing: ParamCasing,
    #[serde(default = "default_collection_casing")]
    pub collection_casing: CollectionCasing,
    #[serde(default = "default_collection_pluralisation")]
    pub collection_pluralisation: CollectionPluralisation,
    #[serde(default = "default_true")]
    pub log_normalisations: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParamCasing { CamelCaseId, Preserve }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CollectionCasing { Lowercase, Preserve }

/// Pluralisation rule for path collection segments.
///
/// `CollectionPluralSingularId` (default):
///   - `GET /<collection>`            → plural   (list all)
///   - `POST /<collection>`           → singular (create one)
///   - segment immediately followed by `{id}` → singular (resource by id)
///   - C2 nested rule: only the **root** segment flips singular when
///     followed by an id; nested `<sub>/{subId}` keeps the sub plural.
///
/// `Preserve` emits paths verbatim.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CollectionPluralisation {
    CollectionPluralSingularId,
    Preserve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraceStyleRule {
    #[serde(default = "default_brace_style")]
    pub output: BraceStyle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BraceStyle { Single, Double }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterRules {
    #[serde(default)]
    pub standard_headers: StandardHeaderRules,
    #[serde(default)]
    pub path_params: PathParamRules,
    /// Casing rule for **request and response body field names** —
    /// applied recursively into nested objects and to `path_params[].name`.
    /// `camel_case` rewrites `account_owner_id` to `accountOwnerId`;
    /// `preserve` emits verbatim.
    #[serde(default = "default_body_field_casing")]
    pub body_field_casing: BodyFieldCasing,
    /// Strip top-level keys from request bodies when they match a name
    /// declared as a standard header (`requestId`, `correlationId`, etc.)
    /// or are inherently response-only (`responseId`). Reason: tracing
    /// concerns belong in headers; duplicating them in the body is
    /// redundant and confuses generated clients. `id` is NOT stripped
    /// — it's a domain field for POST creates.
    /// Response bodies are untouched.
    #[serde(default = "default_true")]
    pub strip_header_fields_from_request_body: bool,

    /// Strip `id` from the request body for PUT/PATCH/DELETE operations.
    /// For these verbs, the resource id is in the URL path; including
    /// it in the body is redundant and creates an ambiguity (what
    /// happens if the body id and URL id disagree?). POST is exempt —
    /// POST creates a resource and the body's `id` IS the new id.
    #[serde(default = "default_true")]
    pub strip_id_for_modifying_verbs: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyFieldCasing { CamelCase, Preserve }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardHeaderRules {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
    #[serde(flatten, default)]
    pub headers: BTreeMap<String, HeaderDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderDef {
    #[serde(default = "default_string_type", rename = "type")]
    pub r#type: String,
    pub format: Option<String>,
    pub description: Option<String>,
    pub example: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathParamRules {
    #[serde(default = "default_uuid_suffix")]
    pub uuid_suffix: String,
    /// Canonicalise `{id}` and `{<entity>_id}` to `{<entity>Id}`. See
    /// `dtcard/.forge/openapi-standard.toml` for full description.
    #[serde(default = "default_true")]
    pub entity_id_canonicalisation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseRules {
    #[serde(default = "default_success_code")]
    pub success_code: String,
    #[serde(default = "default_success_description")]
    pub success_description: String,
    #[serde(default = "default_error_code")]
    pub error_code: String,
    #[serde(default = "default_error_description")]
    pub error_description: String,
    #[serde(default = "default_shared_error_schema")]
    pub shared_error_schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRules {
    #[serde(default = "default_true")]
    pub include_api_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentTypeRules {
    #[serde(default = "default_request_content_types")]
    pub request: Vec<String>,
    #[serde(default = "default_response_content_types")]
    pub response: Vec<String>,
}

fn default_param_casing() -> ParamCasing { ParamCasing::CamelCaseId }
fn default_collection_casing() -> CollectionCasing { CollectionCasing::Lowercase }
fn default_collection_pluralisation() -> CollectionPluralisation {
    CollectionPluralisation::CollectionPluralSingularId
}
fn default_brace_style() -> BraceStyle { BraceStyle::Single }
fn default_true() -> bool { true }
fn default_string_type() -> String { "string".into() }
fn default_uuid_suffix() -> String { "Id".into() }
fn default_body_field_casing() -> BodyFieldCasing { BodyFieldCasing::CamelCase }
fn default_success_code() -> String { "200".into() }
fn default_success_description() -> String { "Successful response".into() }
fn default_error_code() -> String { "400".into() }
fn default_error_description() -> String { "Error response".into() }
fn default_shared_error_schema() -> String { "ApiError".into() }
fn default_request_content_types() -> Vec<String> {
    vec!["application/json".into(), "text/json".into(), "application/*+json".into()]
}
fn default_response_content_types() -> Vec<String> { vec!["application/json".into()] }

impl Default for PathRules {
    fn default() -> Self {
        Self {
            param_casing: default_param_casing(),
            collection_casing: default_collection_casing(),
            collection_pluralisation: default_collection_pluralisation(),
            log_normalisations: true,
        }
    }
}
impl Default for BraceStyleRule {
    fn default() -> Self { Self { output: default_brace_style() } }
}
impl Default for ParameterRules {
    fn default() -> Self {
        Self {
            standard_headers: Default::default(),
            path_params: Default::default(),
            body_field_casing: default_body_field_casing(),
            strip_header_fields_from_request_body: true,
            strip_id_for_modifying_verbs: true,
        }
    }
}
impl Default for StandardHeaderRules {
    fn default() -> Self {
        let mut headers = BTreeMap::new();
        headers.insert("requestId".into(), HeaderDef {
            r#type: "string".into(),
            format: Some("uuid".into()),
            description: Some("Client-generated unique identifier for tracing this request.".into()),
            example: None,
        });
        headers.insert("correlationId".into(), HeaderDef {
            r#type: "string".into(),
            format: Some("uuid".into()),
            description: Some("End-to-end correlation identifier across services.".into()),
            example: None,
        });
        headers.insert("x-api-version".into(), HeaderDef {
            r#type: "string".into(),
            format: None,
            description: Some("API version identifier (e.g. 1.0.0).".into()),
            example: Some("1.0.0".into()),
        });
        headers.insert("Ocp-Apim-Subscription-Key".into(), HeaderDef {
            r#type: "string".into(),
            format: None,
            description: Some("Subscription key for authentication.".into()),
            example: None,
        });
        Self {
            required: vec!["requestId".into(), "x-api-version".into(), "Ocp-Apim-Subscription-Key".into()],
            optional: vec!["correlationId".into()],
            headers,
        }
    }
}
impl Default for PathParamRules {
    fn default() -> Self { Self {
        uuid_suffix: default_uuid_suffix(),
        entity_id_canonicalisation: true,
    } }
}
impl Default for ResponseRules {
    fn default() -> Self {
        Self {
            success_code: default_success_code(),
            success_description: default_success_description(),
            error_code: default_error_code(),
            error_description: default_error_description(),
            shared_error_schema: default_shared_error_schema(),
        }
    }
}
impl Default for ComponentRules {
    fn default() -> Self { Self { include_api_error: true } }
}
impl Default for ContentTypeRules {
    fn default() -> Self {
        Self {
            request: default_request_content_types(),
            response: default_response_content_types(),
        }
    }
}

impl OpenApiStandard {
    pub fn defaults() -> Self {
        Self {
            paths: PathRules::default(),
            brace_style: BraceStyleRule::default(),
            parameters: ParameterRules::default(),
            responses: ResponseRules::default(),
            components: ComponentRules::default(),
            content_types: ContentTypeRules::default(),
        }
    }

    pub fn load(workspace_root: &Path, client: Option<&str>) -> Result<Self, String> {
        let mut std = Self::defaults();
        let workspace_path = workspace_root.join(".forge").join("openapi-standard.toml");
        if workspace_path.exists() {
            let text = std::fs::read_to_string(&workspace_path)
                .map_err(|e| format!("read {}: {}", workspace_path.display(), e))?;
            std = toml::from_str(&text)
                .map_err(|e| format!("parse {}: {}", workspace_path.display(), e))?;
        }
        if let Some(c) = client {
            let override_path = workspace_root
                .join("1-ground-truth").join(c).join(".forge-overrides.toml");
            if override_path.exists() {
                let text = std::fs::read_to_string(&override_path)
                    .map_err(|e| format!("read {}: {}", override_path.display(), e))?;
                let raw: toml::Table = toml::from_str(&text)
                    .map_err(|e| format!("parse {}: {}", override_path.display(), e))?;
                merge_overrides(&mut std, &raw)?;
            }
        }
        Ok(std)
    }
}

fn merge_overrides(std: &mut OpenApiStandard, raw: &toml::Table) -> Result<(), String> {
    if let Some(paths) = raw.get("paths").and_then(|v| v.as_table()) {
        if let Some(v) = paths.get("param_casing").and_then(|v| v.as_str()) {
            std.paths.param_casing = match v {
                "camel_case_id" => ParamCasing::CamelCaseId,
                "preserve" => ParamCasing::Preserve,
                other => return Err(format!("unknown param_casing: {}", other)),
            };
        }
        if let Some(v) = paths.get("collection_casing").and_then(|v| v.as_str()) {
            std.paths.collection_casing = match v {
                "lowercase" => CollectionCasing::Lowercase,
                "preserve" => CollectionCasing::Preserve,
                other => return Err(format!("unknown collection_casing: {}", other)),
            };
        }
        if let Some(v) = paths.get("collection_pluralisation").and_then(|v| v.as_str()) {
            std.paths.collection_pluralisation = match v {
                "collection_plural_singular_id" => CollectionPluralisation::CollectionPluralSingularId,
                "preserve" => CollectionPluralisation::Preserve,
                other => return Err(format!("unknown collection_pluralisation: {}", other)),
            };
        }
        if let Some(v) = paths.get("log_normalisations").and_then(|v| v.as_bool()) {
            std.paths.log_normalisations = v;
        }
    }
    if let Some(bs) = raw.get("brace_style").and_then(|v| v.as_table()) {
        if let Some(v) = bs.get("output").and_then(|v| v.as_str()) {
            std.brace_style.output = match v {
                "single" => BraceStyle::Single,
                "double" => BraceStyle::Double,
                other => return Err(format!("unknown brace_style.output: {}", other)),
            };
        }
    }
    if let Some(params) = raw.get("parameters") {
        let s = toml::to_string(params).unwrap_or_default();
        if let Ok(rules) = toml::from_str::<ParameterRules>(&s) {
            std.parameters = rules;
        }
    }
    if let Some(resp) = raw.get("responses") {
        let s = toml::to_string(resp).unwrap_or_default();
        if let Ok(rules) = toml::from_str::<ResponseRules>(&s) {
            std.responses = rules;
        }
    }
    if let Some(comps) = raw.get("components") {
        let s = toml::to_string(comps).unwrap_or_default();
        if let Ok(rules) = toml::from_str::<ComponentRules>(&s) {
            std.components = rules;
        }
    }
    if let Some(ct) = raw.get("content_types") {
        let s = toml::to_string(ct).unwrap_or_default();
        if let Ok(rules) = toml::from_str::<ContentTypeRules>(&s) {
            std.content_types = rules;
        }
    }
    Ok(())
}
