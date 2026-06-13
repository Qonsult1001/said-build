//! CLI filter expressions for `forge list` and `forge run`.
//!
//! Per spec §4.2. Supported:
//! - `method:<verb>` — OpenAPI method match (case-insensitive)
//! - `path:<glob>` — path glob with `*` wildcards
//! - `kind:<StoryKind>` — snake_case kind match
//! - `tag:<openapi-tag>` — OpenAPI tag match
//! - `text:<substring>` — slug or title substring (case-insensitive)

use crate::{ForgeError, ForgeResult, Story, StoryKind};

#[derive(Debug, Clone)]
pub enum Filter {
    Method(String),
    Path(String),
    Kind(StoryKind),
    Tag(String),
    Text(String),
}

pub fn parse(expr: &str) -> ForgeResult<Filter> {
    let (key, value) = expr
        .split_once(':')
        .ok_or_else(|| ForgeError::Config(format!("filter '{}' missing `:`", expr)))?;
    match key {
        "method" => Ok(Filter::Method(value.to_uppercase())),
        "path" => Ok(Filter::Path(value.to_string())),
        "kind" => {
            let k: StoryKind = serde_json::from_value(serde_json::Value::String(value.into()))
                .map_err(|e| ForgeError::Config(format!("invalid kind '{}': {}", value, e)))?;
            Ok(Filter::Kind(k))
        }
        "tag" => Ok(Filter::Tag(value.to_string())),
        "text" => Ok(Filter::Text(value.to_lowercase())),
        other => Err(ForgeError::Config(format!("unknown filter key '{}'", other))),
    }
}

pub fn matches(story: &Story, f: &Filter) -> bool {
    match f {
        Filter::Method(m) => story
            .fields
            .get("method")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case(m))
            .unwrap_or(false),
        Filter::Path(g) => {
            let path = story
                .fields
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            glob_match(g, path)
        }
        Filter::Kind(k) => story.kind == *k,
        Filter::Tag(t) => story
            .fields
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(|x| x.as_str() == Some(t)))
            .unwrap_or(false),
        Filter::Text(q) => {
            story.title.to_lowercase().contains(q) || story.slug.to_lowercase().contains(q)
        }
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return pattern == text;
    }
    let mut cursor = 0;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !text[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
        } else if i == parts.len() - 1 {
            return text[cursor..].ends_with(part);
        } else if let Some(pos) = text[cursor..].find(part) {
            cursor += pos + part.len();
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn api_story(method: &str, path: &str) -> Story {
        let mut f = BTreeMap::new();
        f.insert("method".into(), serde_json::json!(method));
        f.insert("path".into(), serde_json::json!(path));
        f.insert("tags".into(), serde_json::json!(["pet", "store"]));
        Story {
            slug: format!("{}-{}", method.to_lowercase(), crate::sanitize_slug(path)),
            title: format!("{} {}", method, path),
            raw_text: format!("{} {}", method, path),
            kind: StoryKind::ApiEndpoint,
            fields: f,
            directive_hash: "a".into(),
            source_adapter: "openapi".into(),
            source_anchor: "x".into(),
        }
    }

    #[test]
    fn method_filter() {
        let f = parse("method:GET").unwrap();
        assert!(matches(&api_story("GET", "/pet"), &f));
        assert!(!matches(&api_story("POST", "/pet"), &f));
    }

    #[test]
    fn path_glob_matches() {
        let f = parse("path:/pet/*").unwrap();
        assert!(matches(&api_story("GET", "/pet/42"), &f));
        assert!(matches(&api_story("GET", "/pet/42/uploadImage"), &f));
        assert!(!matches(&api_story("GET", "/store/inventory"), &f));
    }

    #[test]
    fn kind_filter() {
        let f = parse("kind:api_endpoint").unwrap();
        assert!(matches(&api_story("GET", "/pet"), &f));
    }

    #[test]
    fn tag_filter() {
        let f = parse("tag:pet").unwrap();
        assert!(matches(&api_story("GET", "/pet"), &f));
        let not = parse("tag:user").unwrap();
        assert!(!matches(&api_story("GET", "/pet"), &not));
    }

    #[test]
    fn text_filter_matches_title_or_slug() {
        let f = parse("text:pet").unwrap();
        assert!(matches(&api_story("GET", "/pet"), &f));
        let np = parse("text:xyz").unwrap();
        assert!(!matches(&api_story("GET", "/pet"), &np));
    }

    #[test]
    fn unknown_filter_errors() {
        assert!(parse("banana:yes").is_err());
        assert!(parse("no-colon").is_err());
    }

    #[test]
    fn glob_helper() {
        assert!(glob_match("/pet/*", "/pet/42"));
        assert!(glob_match("*pet*", "/api/v2/pet/42"));
        assert!(!glob_match("/store/*", "/pet/42"));
    }
}
