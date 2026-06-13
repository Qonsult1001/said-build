use sca_core::tokenizer::{Tokenizer, WhitespaceTokenizer, HashTokenizer};

#[test]
fn test_whitespace_tokenizer_basic() {
    let tok = WhitespaceTokenizer;
    let tokens = tok.tokenize("Hello World");
    assert_eq!(tokens.len(), 2);
    assert_ne!(tokens[0], tokens[1]);
}

#[test]
fn test_whitespace_tokenizer_deterministic() {
    let tok = WhitespaceTokenizer;
    let a = tok.tokenize("hello world");
    let b = tok.tokenize("hello world");
    assert_eq!(a, b);
}

#[test]
fn test_whitespace_tokenizer_case_insensitive() {
    let tok = WhitespaceTokenizer;
    let a = tok.tokenize("Hello");
    let b = tok.tokenize("hello");
    assert_eq!(a, b);
}

#[test]
fn test_whitespace_tokenizer_strips_punctuation() {
    let tok = WhitespaceTokenizer;
    let a = tok.tokenize("hello,");
    let b = tok.tokenize("hello");
    assert_eq!(a, b);
}

#[test]
fn test_whitespace_tokenizer_empty() {
    let tok = WhitespaceTokenizer;
    assert!(tok.tokenize("").is_empty());
    assert!(tok.tokenize("   ").is_empty());
}

#[test]
fn test_words_extraction() {
    let tok = WhitespaceTokenizer;
    let words = tok.words("Hello, World! This is a TEST.");
    assert_eq!(words, vec!["hello", "world", "this", "is", "a", "test"]);
}

#[test]
fn test_hash_tokenizer() {
    let tok = HashTokenizer;
    let tokens = tok.tokenize("hello world");
    assert_eq!(tokens.len(), 2);
    let tokens2 = tok.tokenize("hello world");
    assert_eq!(tokens, tokens2);
}
