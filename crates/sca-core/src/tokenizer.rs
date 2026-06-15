//! Lightweight, deterministic tokenizers.
//!
//! Restored from the test contract (`tests/test_tokenizer.rs`) — the module
//! existed in the larger codebase but was dropped from the minimal build while
//! its test survived. The contract: case-insensitive, punctuation-stripping,
//! whitespace splitting, with a stable hash form.

/// A tokenizer turns text into a list of tokens. `tokenize` yields opaque token
/// ids (stable across calls); `words` yields the normalized word strings.
pub trait Tokenizer {
    /// Tokenize into stable token ids (u64). Same input → same output.
    fn tokenize(&self, text: &str) -> Vec<u64>;

    /// Extract normalized words (lowercased, punctuation stripped) in order.
    fn words(&self, text: &str) -> Vec<String> {
        normalize_words(text)
    }
}

/// Split on whitespace, lowercase, strip leading/trailing punctuation. Empty and
/// whitespace-only input yield no words.
fn normalize_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Stable FNV-1a hash of a string → u64. Deterministic, no allocation.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x00000100000001B3);
    }
    h
}

/// Whitespace tokenizer: case-insensitive, punctuation-stripped words, each
/// mapped to a stable id via FNV-1a so `tokenize("Hello") == tokenize("hello")`.
pub struct WhitespaceTokenizer;

impl Tokenizer for WhitespaceTokenizer {
    fn tokenize(&self, text: &str) -> Vec<u64> {
        normalize_words(text).iter().map(|w| fnv1a(w)).collect()
    }
}

/// Hash tokenizer: same normalization, ids via FNV-1a. Distinct type so callers
/// can select a tokenization strategy; deterministic by construction.
pub struct HashTokenizer;

impl Tokenizer for HashTokenizer {
    fn tokenize(&self, text: &str) -> Vec<u64> {
        normalize_words(text).iter().map(|w| fnv1a(w)).collect()
    }
}
