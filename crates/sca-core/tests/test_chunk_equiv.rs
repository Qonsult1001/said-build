//! Prove the streaming chunk_text (wrapper over chunk_text_fold) produces BYTE-IDENTICAL
//! passages to the original char-based chunker — so the streaming/batched encode cannot
//! have changed recall. Reimplements the original algorithm here as the oracle.

use sca_core::engine::ScaEngine;

/// The ORIGINAL chunk_text (8eec488) — char-based, verbatim, as the oracle.
fn original_chunk_text(text: &str, chunk_size: usize, stride: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let char_count = chars.len();
    let mut passages = Vec::new();
    let mut start = 0;
    while start < char_count {
        let end = (start + chunk_size).min(char_count);
        let chunk: String = chars[start..end].iter().collect();
        if chunk.trim().len() >= 50 {
            passages.push(chunk);
        }
        start += stride;
    }
    if passages.is_empty() && !text.is_empty() {
        let end = chunk_size.min(char_count);
        let chunk: String = chars[0..end].iter().collect();
        passages.push(chunk);
    }
    passages
}

#[test]
fn streaming_chunker_matches_original_byte_for_byte() {
    let cases = vec![
        String::new(),
        "short".to_string(),
        "a".repeat(49),
        "a".repeat(50),
        "a".repeat(51),
        "x".repeat(512),
        "y".repeat(513),
        "z".repeat(1024),
        "word ".repeat(500),
        "café münster Ñoño 日本語 ".repeat(80), // multibyte UTF-8
        "  leading and trailing whitespace  ".repeat(40),
        "line\nwith\nnewlines\n".repeat(60),
        ("public class Foo { void bar() { return; } } ").repeat(200), // code-like
    ];
    for (i, text) in cases.iter().enumerate() {
        let want = original_chunk_text(text, 512, 256);
        let got = ScaEngine::chunk_text_public(text, 512, 256);
        assert_eq!(got, want, "case {i} ({}chars) passages differ", text.chars().count());
    }
}
