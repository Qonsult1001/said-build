use std::collections::{HashMap, HashSet};
use sca_core::routing::{self, QueryRoute, looks_like_code};

fn empty_corpus() -> (HashSet<String>, HashMap<String, f32>) {
    (HashSet::new(), HashMap::new())
}

fn sample_corpus() -> (HashSet<String>, HashMap<String, f32>) {
    let mut known = HashSet::new();
    let mut idf = HashMap::new();
    for word in &["python", "rust", "machine", "learning", "model", "architecture"] {
        known.insert(word.to_string());
        idf.insert(word.to_string(), 2.0);
    }
    (known, idf)
}

#[test]
fn test_empty_query_routes_semantic() {
    let (known, idf) = empty_corpus();
    let result = routing::analyze_query("", &known, &idf, None);
    assert_eq!(result.route, QueryRoute::PureSemantic);
}

#[test]
fn test_short_words_filtered() {
    let (known, idf) = empty_corpus();
    let result = routing::analyze_query("I am ok", &known, &idf, None);
    assert_eq!(result.route, QueryRoute::PureSemantic);
}

#[test]
fn test_code_intent_routes_lexical() {
    let (known, idf) = sample_corpus();
    let result = routing::analyze_query("find the passkey for this system", &known, &idf, None);
    assert_eq!(result.route, QueryRoute::PureLexical);
}

#[test]
fn test_code_like_token_routes_lexical() {
    let (known, idf) = sample_corpus();
    let result = routing::analyze_query("what is abc123xyz", &known, &idf, None);
    assert_eq!(result.route, QueryRoute::PureLexical);
    assert!(result.has_code);
}

#[test]
fn test_natural_language_routes_hybrid() {
    let (known, idf) = sample_corpus();
    let result = routing::analyze_query("what architecture should I use for machine learning", &known, &idf, None);
    assert_eq!(result.route, QueryRoute::FullHybrid);
}

#[test]
fn test_compound_splitting() {
    let (known, idf) = empty_corpus();
    let result = routing::analyze_query("enter the passkey", &known, &idf, None);
    assert!(result.expanded.contains("pass"));
    assert!(result.expanded.contains("key"));
    assert!(result.expanded.contains("passkey"));
}

#[test]
fn test_looks_like_code() {
    assert!(looks_like_code("abc123"));
    assert!(looks_like_code("ABC_DEF"));
    assert!(looks_like_code("user@domain.com"));
    assert!(!looks_like_code("hello"));
    assert!(!looks_like_code("a"));
}

#[test]
fn test_query_analysis_fields() {
    let (known, idf) = sample_corpus();
    let result = routing::analyze_query("python machine learning model", &known, &idf, None);
    assert_eq!(result.words.len(), 4);
    assert!(result.idf_avg > 0.0);
    assert!(!result.has_code);
    assert!(!result.has_typo);
}
