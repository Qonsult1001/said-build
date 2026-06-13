//! Per-`StoryKind` query plan builders. One function per kind.
//!
//! Per spec §8.1.

use crate::grounding::RetrievalQuery;
use crate::story::Pillar;
use crate::{Story, StoryKind};

/// Shorthand constructor — SCA engine, no authority filter.
fn sca_query(query: impl Into<String>, origin: impl Into<String>, pillars: Vec<Pillar>) -> RetrievalQuery {
    RetrievalQuery {
        query: query.into(),
        origin: origin.into(),
        pillar_scope: pillars,
        engine: "sca".into(),
        authority_filter: None,
    }
}

/// Shorthand constructor — sym engine, no authority filter.
fn sym_query(query: impl Into<String>, origin: impl Into<String>, pillars: Vec<Pillar>) -> RetrievalQuery {
    RetrievalQuery {
        query: query.into(),
        origin: origin.into(),
        pillar_scope: pillars,
        engine: "sym".into(),
        authority_filter: None,
    }
}

/// Dispatch to the appropriate builder for a story's kind.
pub fn queries_for(story: &Story) -> Vec<RetrievalQuery> {
    match story.kind {
        StoryKind::ApiEndpoint => api_endpoint_queries(story),
        StoryKind::Requirement => requirement_queries(story),
        StoryKind::Ticket | StoryKind::TableRow => row_queries(story),
        StoryKind::Generic => generic_queries(story),
    }
}

pub fn api_endpoint_queries(story: &Story) -> Vec<RetrievalQuery> {
    let mut out = Vec::new();
    let method = story
        .fields
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let path = story
        .fields
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let operation_id = story
        .fields
        .get("operation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !method.is_empty() && !path.is_empty() {
        out.push(sca_query(
            format!("{} {}", method, path),
            "method+path",
            vec![Pillar::Code, Pillar::External],
        ));
        // Noun-focused last path segments
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.starts_with('{'))
            .collect();
        let tail_query = segments.iter().rev().take(2).rev().copied().collect::<Vec<_>>().join(" ");
        if !tail_query.is_empty() {
            out.push(sca_query(
                tail_query,
                "path_tail",
                vec![Pillar::Code, Pillar::External, Pillar::Semantic],
            ));
        }
    }
    if !operation_id.is_empty() {
        out.push(sym_query(operation_id, "operation_id", vec![Pillar::Code]));
        let snake = camel_to_snake(operation_id);
        if snake != operation_id {
            out.push(sym_query(snake, "operation_id_snake", vec![Pillar::Code]));
        }
    }
    // Tags
    if let Some(tags) = story.fields.get("tags").and_then(|v| v.as_array()) {
        for tag in tags {
            if let Some(t) = tag.as_str() {
                out.push(sca_query(
                    t.to_string(),
                    "tag",
                    vec![Pillar::External, Pillar::Semantic],
                ));
            }
        }
    }
    // Business-logic query: searches External-pillar docs (requirements PDFs,
    // Dev Planning MDs) for any frame matching the story's title + path-noun
    // tokens, scoped to authority levels that carry real client/scope signal.
    // Without this, API-endpoint grounding never reaches the 40+ business-rule
    // PDFs in 3-requirements/ because the OpenAPI-derived queries all key off
    // method+path and operation_id.
    let path_nouns: String = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .collect::<Vec<_>>()
        .join(" ");
    let biz_query = format!("{} {}", story.title.trim(), path_nouns)
        .trim()
        .to_string();
    if !biz_query.is_empty() {
        out.push(RetrievalQuery {
            query: biz_query,
            origin: "business_logic".into(),
            pillar_scope: vec![Pillar::External],
            engine: "sca".into(),
            authority_filter: Some(vec![
                "law".into(),
                "requested".into(),
                "agreed".into(),
            ]),
        });
    }
    // Episodic lookback
    if !operation_id.is_empty() {
        out.push(sca_query(
            operation_id,
            "episodic_lookback",
            vec![Pillar::Episodic],
        ));
    }
    out
}

pub fn requirement_queries(story: &Story) -> Vec<RetrievalQuery> {
    let mut out = Vec::new();
    out.push(sca_query(
        story.title.clone(),
        "title",
        Pillar::ALL.to_vec(),
    ));
    if !story.raw_text.is_empty() {
        out.push(sca_query(
            story.raw_text.clone(),
            "raw_text",
            vec![Pillar::External, Pillar::Semantic, Pillar::Code],
        ));
    }
    for np in noun_phrases(&story.title) {
        out.push(sym_query(np.clone(), "noun_phrase", vec![Pillar::Code]));
        out.push(sca_query(np, "procedural_lookback", vec![Pillar::Procedural]));
    }
    out
}

pub fn row_queries(story: &Story) -> Vec<RetrievalQuery> {
    let mut out = Vec::new();
    for (key, val) in &story.fields {
        let lk = key.to_lowercase();
        if lk == "id" || lk == "key" || lk == "code" || lk == "ref" || lk.ends_with("_id") {
            if let Some(s) = val.as_str() {
                out.push(sca_query(
                    s,
                    format!("field:{}", key),
                    vec![Pillar::Code, Pillar::External],
                ));
            }
        }
    }
    if !story.title.is_empty() {
        out.push(sca_query(
            story.title.clone(),
            "title",
            vec![Pillar::Code, Pillar::External],
        ));
    }
    out
}

pub fn generic_queries(story: &Story) -> Vec<RetrievalQuery> {
    vec![sca_query(
        story.raw_text.clone(),
        "raw_text",
        Pillar::ALL.to_vec(),
    )]
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

fn noun_phrases(title: &str) -> Vec<String> {
    title
        .split_whitespace()
        .filter(|t| t.len() >= 3 && t.chars().next().map_or(false, |c| c.is_uppercase()))
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Story, StoryKind};

    fn api_story() -> Story {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("method".into(), serde_json::json!("POST"));
        fields.insert("path".into(), serde_json::json!("/accounts/{id}/freeze"));
        fields.insert("operation_id".into(), serde_json::json!("freezeAccount"));
        fields.insert("tags".into(), serde_json::json!(["account", "fraud"]));
        Story {
            slug: "post-accounts-id-freeze".into(),
            title: "Freeze account".into(),
            raw_text: "POST /accounts/{id}/freeze".into(),
            kind: StoryKind::ApiEndpoint,
            fields,
            directive_hash: "a3f91".into(),
            source_adapter: "openapi".into(),
            source_anchor: "paths./accounts/{id}/freeze.post".into(),
        }
    }

    #[test]
    fn api_endpoint_builds_at_least_one_query_per_category() {
        let qs = api_endpoint_queries(&api_story());
        assert!(qs.iter().any(|q| q.origin == "method+path"), "must cover method+path query");
        assert!(qs.iter().any(|q| q.origin == "operation_id"), "must cover operation_id sym lookup");
        assert!(qs.iter().any(|q| q.origin == "tag"), "must cover OpenAPI tag scope");
        assert!(qs.iter().any(|q| q.origin == "business_logic"), "must cover business-logic query");
        assert!(qs.iter().any(|q| q.origin == "episodic_lookback"), "must cover episodic lookback");
    }

    #[test]
    fn business_logic_query_is_authority_filtered_to_external() {
        let qs = api_endpoint_queries(&api_story());
        let biz = qs.iter().find(|q| q.origin == "business_logic").unwrap();
        assert_eq!(biz.pillar_scope, vec![Pillar::External]);
        let filter = biz.authority_filter.as_ref().expect("business-logic must carry authority filter");
        assert!(filter.contains(&"law".to_string()));
        assert!(filter.contains(&"requested".to_string()));
        assert!(filter.contains(&"agreed".to_string()));
    }

    #[test]
    fn business_logic_query_includes_title_and_path_nouns() {
        let qs = api_endpoint_queries(&api_story());
        let biz = qs.iter().find(|q| q.origin == "business_logic").unwrap();
        assert!(biz.query.to_lowercase().contains("freeze"));
        assert!(biz.query.to_lowercase().contains("accounts"));
    }

    #[test]
    fn api_endpoint_method_path_query_scopes_code_and_external() {
        let qs = api_endpoint_queries(&api_story());
        let q = qs.iter().find(|q| q.origin == "method+path").unwrap();
        assert!(q.pillar_scope.contains(&Pillar::Code));
        assert!(q.pillar_scope.contains(&Pillar::External));
    }

    #[test]
    fn requirement_queries_span_three_pillars_for_raw_text() {
        let s = Story {
            slug: "close-account".into(),
            title: "Close account".into(),
            raw_text: "Close customer account and archive statements".into(),
            kind: StoryKind::Requirement,
            fields: Default::default(),
            directive_hash: "a3f91".into(),
            source_adapter: "markdown".into(),
            source_anchor: "line:5".into(),
        };
        let qs = requirement_queries(&s);
        assert!(qs.iter().any(|q|
            q.origin == "raw_text" &&
            q.pillar_scope.contains(&Pillar::External) &&
            q.pillar_scope.contains(&Pillar::Semantic) &&
            q.pillar_scope.contains(&Pillar::Code)
        ));
    }

    #[test]
    fn generic_query_scopes_all_pillars() {
        let s = Story {
            slug: "custom".into(),
            title: "custom".into(),
            raw_text: "anything goes".into(),
            kind: StoryKind::Generic,
            fields: Default::default(),
            directive_hash: "a3f91".into(),
            source_adapter: "markdown".into(),
            source_anchor: "mem".into(),
        };
        let qs = generic_queries(&s);
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].pillar_scope.len(), 6);
    }
}
