//! Byte-identity test for the hand-written WordPiece tokenizer + pooling (#4).
//!
//! GATE: the self-contained `OwnStaticEncoder` MUST produce embeddings that are
//! bit-for-bit (within f32 rounding of the SAME ops) identical to the model2vec
//! `StaticEncoder` (which goes through HF `tokenizers`). Retrieval quality depends
//! on text → token-ID → static-embedding producing EXACTLY the current vectors.
//!
//! This drives + locks the identity. If it ever goes RED, the own path has
//! diverged from model2vec and must NOT ship.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model" \
//!        --test test_own_tokenizer -- --nocapture

#![cfg(feature = "static-embed")]

use sca_core::latent_cluster::{OwnStaticEncoder, StaticEncoder};
use std::path::PathBuf;

/// Resolve a model dir that exists, or return None so the test skips gracefully.
fn model_dir() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    for cand in [
        "SAID-LAM-private/said-lam-static-4M",
        "SAID-LAM-private/said-lam-static-2M",
        "SAID-LAM-private/said-lam-static",
        "model-eval/potion-base-4M",
    ] {
        let p = root.join(cand);
        if p.join("model.safetensors").exists()
            && p.join("tokenizer.json").exists()
            && p.join("config.json").exists()
        {
            return Some(p);
        }
    }
    None
}

/// The battery of texts: english, SQL, C#, identifiers, unicode/accents,
/// punctuation-heavy, empty, very long (>512 tokens), CJK, mixed.
fn battery() -> Vec<String> {
    let long = "select ".repeat(600); // forces >512 tokens + char pre-truncation
    let very_long_word = "a".repeat(250); // exceeds max_input_chars_per_word=100 → [UNK]
    vec![
        "".to_string(),
        " ".to_string(),
        "hello world".to_string(),
        "The quick brown fox jumps over the lazy dog.".to_string(),
        "CREATE PROCEDURE GetUserById @id INT AS SELECT * FROM Users WHERE Id = @id;".to_string(),
        "public class Foo { private int x; public void Bar() { return; } }".to_string(),
        "getUserById fetchUserData parseJSONResponse HTTPSConnection".to_string(),
        "café résumé naïve Zürich Größe".to_string(),
        "!@#$%^&*()_+-=[]{}|;':\",./<>?`~".to_string(),
        "a.b.c.d::e->f(g)[h]{i}".to_string(),
        "Mixed CASE with Numbers 12345 and symbols #hashtag @mention".to_string(),
        "野口里佳 Noguchi Rika 中文测试 日本語".to_string(),
        "tab\there\nnewline\rcarriage".to_string(),
        long,
        very_long_word,
        "SELECT a, b, c\nFROM table_name\nWHERE x > 10 AND y < 20\nGROUP BY a;".to_string(),
        "snake_case CamelCase kebab-case SCREAMING_SNAKE".to_string(),
        "https://example.com/path?query=value&other=123#fragment".to_string(),
        "emoji test 🚀 🔥 ✅ and zero-width\u{200b}joiner".to_string(),
        "x".to_string(),
    ]
}

#[test]
fn own_tokenizer_is_byte_identical_to_model2vec() {
    let Some(dir) = model_dir() else {
        eprintln!("SKIP: no model dir found (need model.safetensors+tokenizer.json+config.json)");
        return;
    };
    let dir_s = dir.to_str().unwrap();
    eprintln!("model dir: {dir_s}");

    let hf = StaticEncoder::from_model2vec(dir_s).expect("load model2vec (HF) encoder");
    let own = OwnStaticEncoder::from_pretrained(dir_s).expect("load own encoder");

    let texts = battery();
    let hf_emb = hf.encode_batch_model2vec(&texts);
    let own_emb = own.encode_batch(&texts);

    assert_eq!(hf_emb.len(), own_emb.len(), "batch length mismatch");

    let mut max_diff = 0.0f32;
    let mut worst_idx = 0usize;
    for (i, (a, b)) in hf_emb.iter().zip(own_emb.iter()).enumerate() {
        assert_eq!(
            a.len(),
            b.len(),
            "dim mismatch on text {i}: hf={} own={} (text={:?})",
            a.len(),
            b.len(),
            texts[i].chars().take(40).collect::<String>()
        );
        for (j, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
            let d = (x - y).abs();
            if d > max_diff {
                max_diff = d;
                worst_idx = i;
            }
            assert!(
                d <= 1e-5,
                "text {i} dim {j}: hf={x} own={y} diff={d} (text={:?})",
                texts[i].chars().take(40).collect::<String>()
            );
        }
    }
    eprintln!(
        "IDENTITY OK: {} texts, max per-element diff = {:.3e} (worst text idx {})",
        texts.len(),
        max_diff,
        worst_idx
    );
}
