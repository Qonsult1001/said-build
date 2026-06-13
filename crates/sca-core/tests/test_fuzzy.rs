use sca_core::fuzzy::{soundex, levenshtein};

#[test]
fn test_soundex_basic() {
    assert_eq!(soundex("Robert"), "R163");
    assert_eq!(soundex("Rupert"), "R163");
    assert_eq!(soundex("Robert"), soundex("Rupert"));
}

#[test]
fn test_soundex_different() {
    assert_ne!(soundex("Robert"), soundex("Smith"));
    assert_ne!(soundex("Alice"), soundex("Bob"));
}

#[test]
fn test_soundex_empty() {
    assert_eq!(soundex(""), "0000");
}

#[test]
fn test_soundex_short() {
    let result = soundex("A");
    assert_eq!(result.len(), 4);
    assert!(result.starts_with('A'));
}

#[test]
fn test_levenshtein_identical() {
    assert_eq!(levenshtein("hello", "hello"), 0);
}

#[test]
fn test_levenshtein_one_edit() {
    assert_eq!(levenshtein("hello", "hallo"), 1);
    assert_eq!(levenshtein("hello", "hell"), 1);
    assert_eq!(levenshtein("hell", "hello"), 1);
}

#[test]
fn test_levenshtein_empty() {
    assert_eq!(levenshtein("", "hello"), 5);
    assert_eq!(levenshtein("hello", ""), 5);
    assert_eq!(levenshtein("", ""), 0);
}

#[test]
fn test_levenshtein_completely_different() {
    assert_eq!(levenshtein("abc", "xyz"), 3);
}

#[test]
fn test_levenshtein_unicode() {
    assert_eq!(levenshtein("café", "cafe"), 1);
}
