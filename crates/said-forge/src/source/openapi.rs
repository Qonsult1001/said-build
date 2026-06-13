//! OpenAPI 3.x adapter.
//!
//! Per spec §7.1. One story per operation. `method + path` becomes the slug;
//! `operation_id` (when present) goes into `fields`.

use crate::source::{DirectiveSource, SourceCapabilities};
use crate::{sanitize_slug, DirectiveDoc, DirectiveMeta, ForgeError, ForgeResult, Story, StoryKind};
use async_trait::async_trait;

pub struct OpenApiSource;

#[async_trait]
impl DirectiveSource for OpenApiSource {
    fn name(&self) -> &'static str { "openapi" }
    fn supported_extensions(&self) -> &'static [&'static str] { &["yaml", "yml", "json"] }
    fn supported_url_schemes(&self) -> &'static [&'static str] { &["http", "https"] }
    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            supports_incremental_reload: true,
            supports_filter_dsl: true,
            provides_schema_fields: true,
            requires_network: false,
        }
    }

    fn detect(&self, path_or_url: &str) -> bool {
        // URL — require http/https + extension or "openapi"/"swagger" hint.
        if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
            let lower = path_or_url.to_lowercase();
            return lower.ends_with(".yaml")
                || lower.ends_with(".yml")
                || lower.ends_with(".json")
                || lower.contains("openapi")
                || lower.contains("swagger");
        }
        // Reject any other URL scheme (ftp://, file://, etc.).
        if let Some(colon_pos) = path_or_url.find(':') {
            let before = &path_or_url[..colon_pos];
            let is_scheme = before.chars().all(|c| c.is_ascii_alphabetic())
                && path_or_url[colon_pos..].starts_with("://");
            if is_scheme {
                return false;
            }
        }
        // File — check extension then (cheaply) peek at first bytes if possible.
        let lower_path = path_or_url.to_lowercase();
        let ext_match = lower_path.ends_with(".yaml")
            || lower_path.ends_with(".yml")
            || lower_path.ends_with(".json");
        if !ext_match {
            return false;
        }
        // Content peek. If the file doesn't exist, fall back to extension.
        let Ok(bytes) = std::fs::read(path_or_url) else {
            return true;
        };
        let head_len = bytes.len().min(512);
        let head = String::from_utf8_lossy(&bytes[..head_len]);
        head.contains("openapi:") || head.contains("\"openapi\"") || head.contains("swagger:")
    }

    async fn load(&self, path_or_url: &str, operator: &str) -> ForgeResult<DirectiveDoc> {
        let is_url = path_or_url.starts_with("http://") || path_or_url.starts_with("https://");
        let (raw, content_type) = if is_url {
            let resp = reqwest::get(path_or_url).await.map_err(|e| ForgeError::Http {
                url: path_or_url.to_string(),
                message: e.to_string(),
            })?;
            if !resp.status().is_success() {
                return Err(ForgeError::Http {
                    url: path_or_url.to_string(),
                    message: format!("HTTP {}", resp.status()),
                });
            }
            let ct = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let bytes = resp.bytes().await.map_err(|e| ForgeError::Http {
                url: path_or_url.to_string(),
                message: e.to_string(),
            })?;
            (bytes.to_vec(), ct)
        } else {
            let bytes = std::fs::read(path_or_url).map_err(|cause| ForgeError::Io {
                path: path_or_url.to_string(),
                cause,
            })?;
            let ct = if path_or_url.ends_with(".json") {
                Some("application/json".into())
            } else {
                Some("application/yaml".into())
            };
            (bytes, ct)
        };
        Ok(DirectiveDoc {
            source: path_or_url.to_string(),
            adapter: "openapi".into(),
            raw,
            meta: DirectiveMeta {
                loaded_at_utc: now_utc(),
                operator: operator.to_string(),
                content_type,
            },
        })
    }

    fn extract_stories(&self, doc: &DirectiveDoc) -> ForgeResult<Vec<Story>> {
        let text = std::str::from_utf8(&doc.raw).map_err(|e| ForgeError::Parse {
            path: doc.source.clone(),
            message: format!("utf-8: {}", e),
        })?;
        let directive_hash = crate::forge_hash(&doc.source);

        // Parse as generic JSON/YAML rather than going through oas3, which has
        // stricter requirements + API-version churn. We only need the
        // paths.<path>.<method> skeleton.
        let json_value: serde_json::Value = if text.trim_start().starts_with('{') {
            serde_json::from_str(text).map_err(|e| ForgeError::Parse {
                path: doc.source.clone(),
                message: format!("json: {}", e),
            })?
        } else {
            serde_yaml_to_json(text, &doc.source)?
        };

        let paths = json_value
            .get("paths")
            .and_then(|v| v.as_object())
            .ok_or_else(|| ForgeError::Parse {
                path: doc.source.clone(),
                message: "document has no `paths` object".into(),
            })?;

        let methods = ["get", "post", "put", "delete", "patch", "head", "options", "trace"];
        let mut stories = Vec::new();
        for (path, path_item) in paths {
            let Some(path_obj) = path_item.as_object() else { continue };
            for method in methods {
                if let Some(op) = path_obj.get(method) {
                    stories.push(build_story(path, method, op, &directive_hash));
                }
            }
        }
        Ok(stories)
    }
}

fn now_utc() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Thin YAML → JSON adapter so extract_stories can walk with serde_json.
fn serde_yaml_to_json(text: &str, source: &str) -> ForgeResult<serde_json::Value> {
    // We don't have serde_yaml in the dep tree; use a minimal JSON-escape pre-pass?
    // Instead, use oas3's underlying yaml dependency. Rather than another dep,
    // we pull `serde_yaml` via oas3 transitively — use it directly.
    let parsed: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| ForgeError::Parse {
        path: source.into(),
        message: format!("yaml: {}", e),
    })?;
    // Convert yaml::Value → json::Value via a round-trip string.
    let json_str = serde_json::to_string(&parsed).map_err(|e| ForgeError::Parse {
        path: source.into(),
        message: format!("yaml->json: {}", e),
    })?;
    serde_json::from_str::<serde_json::Value>(&json_str).map_err(|e| ForgeError::Parse {
        path: source.into(),
        message: format!("json from yaml: {}", e),
    })
}

fn build_story(
    path: &str,
    method: &str,
    op: &serde_json::Value,
    directive_hash: &str,
) -> Story {
    let method_upper = method.to_uppercase();
    let operation_id = op
        .get("operationId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let summary = op
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let description = op
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let title = if !summary.is_empty() {
        summary.clone()
    } else {
        format!("{} {}", method_upper, path)
    };
    let slug = format!("{}-{}", method, sanitize_slug(path));

    let mut fields = std::collections::BTreeMap::new();
    fields.insert("method".into(), serde_json::Value::String(method_upper.clone()));
    fields.insert("path".into(), serde_json::Value::String(path.into()));
    if !operation_id.is_empty() {
        fields.insert("operation_id".into(), serde_json::Value::String(operation_id.clone()));
    }
    if !summary.is_empty() {
        fields.insert("summary".into(), serde_json::Value::String(summary.clone()));
    }
    if !description.is_empty() {
        fields.insert("description".into(), serde_json::Value::String(description));
    }
    if let Some(tags) = op.get("tags").and_then(|v| v.as_array()) {
        let tag_vec: Vec<serde_json::Value> = tags.iter().cloned().collect();
        if !tag_vec.is_empty() {
            fields.insert("tags".into(), serde_json::Value::Array(tag_vec));
        }
    }
    if let Some(params) = op.get("parameters").and_then(|v| v.as_array()) {
        let param_summaries: Vec<serde_json::Value> = params
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.get("name").cloned().unwrap_or(serde_json::Value::Null),
                    "in":   p.get("in").cloned().unwrap_or(serde_json::Value::Null),
                    "required": p.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
                })
            })
            .collect();
        if !param_summaries.is_empty() {
            fields.insert("parameters".into(), serde_json::Value::Array(param_summaries));
        }
    }
    if let Some(responses) = op.get("responses").and_then(|v| v.as_object()) {
        let mut resp_map = serde_json::Map::new();
        for (code, _body) in responses {
            resp_map.insert(code.clone(), serde_json::json!({ "code": code }));
        }
        if !resp_map.is_empty() {
            fields.insert("responses".into(), serde_json::Value::Object(resp_map));
        }
    }

    let raw_text = format!("{} {} — {}", method_upper, path, title);
    Story {
        slug,
        title,
        raw_text,
        kind: StoryKind::ApiEndpoint,
        fields,
        directive_hash: directive_hash.to_string(),
        source_adapter: "openapi".into(),
        source_anchor: format!("paths.{}.{}", path, method),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("petstore.yaml")
    }

    #[test]
    fn detects_yaml_file_by_extension() {
        let s = OpenApiSource;
        assert!(s.detect("fixtures/petstore.yaml"));
        assert!(s.detect("anywhere/api.yml"));
        assert!(s.detect("schema.json"));
        assert!(!s.detect("README.md"));
        assert!(!s.detect("data.csv"));
    }

    #[test]
    fn detects_http_urls() {
        let s = OpenApiSource;
        assert!(s.detect("https://petstore.swagger.io/v2/swagger.json"));
        assert!(s.detect("http://example.com/openapi.yaml"));
        assert!(!s.detect("ftp://example.com/spec.yaml"));
    }

    #[tokio::test]
    async fn loads_local_fixture() {
        let s = OpenApiSource;
        let doc = s.load(fixture_path().to_str().unwrap(), "test-user").await.unwrap();
        assert_eq!(doc.adapter, "openapi");
        assert!(doc.raw.starts_with(b"openapi:"));
        assert_eq!(doc.meta.operator, "test-user");
    }

    #[tokio::test]
    async fn extracts_twenty_stories_from_petstore() {
        let s = OpenApiSource;
        let doc = s.load(fixture_path().to_str().unwrap(), "test").await.unwrap();
        let stories = s.extract_stories(&doc).unwrap();
        assert_eq!(stories.len(), 20, "petstore fixture has 20 operations");
        let add_pet = stories.iter().find(|s| s.slug == "post-pet").unwrap();
        assert_eq!(add_pet.kind, StoryKind::ApiEndpoint);
        assert_eq!(add_pet.fields.get("method").and_then(|v| v.as_str()), Some("POST"));
        assert_eq!(add_pet.fields.get("path").and_then(|v| v.as_str()), Some("/pet"));
        assert_eq!(
            add_pet.fields.get("operation_id").and_then(|v| v.as_str()),
            Some("addPet")
        );
    }

    #[tokio::test]
    async fn slugs_disambiguate_same_path_different_methods() {
        let s = OpenApiSource;
        let doc = s.load(fixture_path().to_str().unwrap(), "test").await.unwrap();
        let stories = s.extract_stories(&doc).unwrap();
        let pet_slugs: Vec<_> = stories
            .iter()
            .filter(|s| s.fields.get("path").and_then(|v| v.as_str()) == Some("/pet/{petId}"))
            .map(|s| s.slug.clone())
            .collect();
        // Petstore has GET, POST, DELETE on /pet/{petId}
        assert_eq!(pet_slugs.len(), 3);
        let mut sorted = pet_slugs.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            vec!["delete-pet-petid", "get-pet-petid", "post-pet-petid"]
        );
    }

    #[tokio::test]
    async fn tags_are_preserved_in_fields() {
        let s = OpenApiSource;
        let doc = s.load(fixture_path().to_str().unwrap(), "test").await.unwrap();
        let stories = s.extract_stories(&doc).unwrap();
        let add_pet = stories.iter().find(|s| s.slug == "post-pet").unwrap();
        let tags = add_pet
            .fields
            .get("tags")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].as_str(), Some("pet"));
    }
}
