use std::collections::{HashMap, HashSet};
use sca_core::idf::IDFTable;
use sca_core::fuzzy::soundex;

#[test]
fn test_idf_basic_formula() {
    let mut table = IDFTable::new();
    let token_postings = vec![(1u32, 2usize), (2u32, 10usize)];
    let word_doc_counts = HashMap::new();
    table.rebuild(10, &token_postings, &word_doc_counts, &|_| String::new());
    let idf_rare = table.get_token_idf(1);
    let idf_common = table.get_token_idf(2);
    assert!(idf_rare > idf_common, "rare={} should be > common={}", idf_rare, idf_common);
    assert!((idf_common - 1.0).abs() < 0.01, "common should be ~1.0, got {}", idf_common);
}

#[test]
fn test_idf_unknown_token() {
    let table = IDFTable::new();
    assert_eq!(table.get_token_idf(999), 0.5);
    assert_eq!(table.get_word_idf("unknown"), 1.0);
}

#[test]
fn test_word_level_idf() {
    let mut table = IDFTable::new();
    let mut word_doc_counts: HashMap<String, HashSet<String>> = HashMap::new();
    word_doc_counts.insert("python".to_string(), ["doc1", "doc2"].iter().map(|s| s.to_string()).collect());
    word_doc_counts.insert("rust".to_string(), ["doc1"].iter().map(|s| s.to_string()).collect());
    word_doc_counts.insert("go".to_string(), ["doc1", "doc2", "doc3"].iter().map(|s| s.to_string()).collect());
    table.rebuild(3, &[], &word_doc_counts, &soundex);
    assert!(table.word_idf.contains_key("python"));
    assert!(table.word_idf.contains_key("rust"));
    assert!(!table.word_idf.contains_key("go"));
    let idf_rust = table.get_word_idf("rust");
    let idf_python = table.get_word_idf("python");
    assert!(idf_rust > idf_python, "rust (rarer) should have higher IDF");
}

#[test]
fn test_phonetic_index() {
    let mut table = IDFTable::new();
    let mut word_doc_counts: HashMap<String, HashSet<String>> = HashMap::new();
    word_doc_counts.insert("robert".to_string(), ["doc1"].iter().map(|s| s.to_string()).collect());
    word_doc_counts.insert("rupert".to_string(), ["doc2"].iter().map(|s| s.to_string()).collect());
    table.rebuild(2, &[], &word_doc_counts, &soundex);
    let sx = soundex("robert");
    let matches = table.phonetic_lookup(&sx);
    assert!(matches.contains("robert"));
    assert!(matches.contains("rupert"));
}

#[test]
fn test_query_idf() {
    let mut table = IDFTable::new();
    let mut word_doc_counts: HashMap<String, HashSet<String>> = HashMap::new();
    word_doc_counts.insert("machine".to_string(), ["doc1"].iter().map(|s| s.to_string()).collect());
    word_doc_counts.insert("learning".to_string(), ["doc1", "doc2", "doc3"].iter().map(|s| s.to_string()).collect());
    table.rebuild(3, &[], &word_doc_counts, &soundex);
    let query_words = vec!["machine".to_string(), "learning".to_string()];
    let total_idf = table.get_query_idf(&query_words);
    assert!(total_idf > 0.0);
    assert!(total_idf > table.get_word_idf("learning"));
}

#[test]
fn test_alpha_values() {
    assert_eq!(IDFTable::get_alpha("high_lexical"), 0.85);
    assert_eq!(IDFTable::get_alpha("balanced"), 0.70);
    assert_eq!(IDFTable::get_alpha("pure_semantic"), 0.0);
    assert_eq!(IDFTable::get_alpha("unknown"), 0.70);
}

#[test]
fn test_dirty_flag() {
    let mut table = IDFTable::new();
    assert!(table.dirty);
    table.rebuild(1, &[(1, 1)], &HashMap::new(), &|_| String::new());
    assert!(!table.dirty);
    table.mark_dirty();
    assert!(table.dirty);
}

#[test]
fn test_clear() {
    let mut table = IDFTable::new();
    table.rebuild(1, &[(1, 1)], &HashMap::new(), &|_| String::new());
    assert_eq!(table.token_count(), 1);
    table.clear();
    assert_eq!(table.token_count(), 0);
    assert_eq!(table.word_count(), 0);
    assert!(table.dirty);
}
