//! Tests for the DualEncoder — fast (Model2Vec) + precision (external) paths.
//!
//! Tests without the `static-embed` feature verify:
//! - DualEncoder works in precision-only mode
//! - wrap_precision produces correct entries
//! - has_static() returns false without the feature
//! - Stats tracking works
//!
//! Tests WITH `static-embed` feature (run with --features static-embed) verify:
//! - Model2Vec loads and encodes
//! - Fast path produces embeddings in the same latent space
//! - Dual encoder routes correctly between fast and precision
//! - Speed comparison: static vs simulated precision

use sca_core::latent_cluster::{DualEncoder, EncoderSource, LatentClusterIndex};

// =============================================================================
// HELPERS
// =============================================================================

fn sinusoidal_embedding(id: u32, dim: usize) -> Vec<f32> {
    let mut emb = vec![0.0f32; dim];
    let pos = id as f64;
    for i in 0..(dim / 2).min(32) {
        let freq = 1.0 / (10000.0_f64).powf(2.0 * i as f64 / 64.0);
        emb[2 * i] = (pos * freq).sin() as f32;
        emb[2 * i + 1] = (pos * freq).cos() as f32;
    }
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for v in emb.iter_mut() {
            *v /= norm;
        }
    }
    emb
}

// =============================================================================
// TESTS: PRECISION-ONLY MODE (no static-embed feature needed)
// =============================================================================

#[test]
fn test_dual_encoder_precision_only() {
    let mut enc = DualEncoder::precision_only(64);
    assert!(!enc.has_static());
    assert_eq!(enc.stats(), (0, 0));

    let entry = enc.wrap_precision(sinusoidal_embedding(0, 384));
    assert_eq!(entry.source, EncoderSource::Precision);
    assert_eq!(entry.embedding.len(), 384);
    assert_eq!(enc.stats(), (0, 1));
}

#[test]
fn test_dual_encoder_fast_returns_none_without_feature() {
    let mut enc = DualEncoder::precision_only(64);
    let result = enc.encode_fast("hello world");
    // Without static-embed feature, fast path always returns None
    if !enc.has_static() {
        assert!(result.is_none());
    }
}

#[test]
fn test_dual_encoder_add_fast_fallback() {
    let mut enc = DualEncoder::precision_only(64);
    let mut idx = LatentClusterIndex::new(64, 384, 4);

    // Fast path unavailable — returns None, caller must provide embedding
    let result = enc.add_fast(&mut idx, 0, "hello world");
    if !enc.has_static() {
        assert!(result.is_none());
        assert_eq!(idx.entry_count(), 0);
    }

    // Precision path: caller provides embedding
    let emb = sinusoidal_embedding(0, 384);
    let entry = enc.wrap_precision(emb.clone());
    assert_eq!(entry.source, EncoderSource::Precision);
    idx.add(0, &entry.embedding);
    assert_eq!(idx.entry_count(), 1);
}

#[test]
fn test_dual_encoder_mixed_indexing() {
    // Simulates the real use case: some docs via fast path, some via precision
    let mut enc = DualEncoder::precision_only(64);
    let mut idx = LatentClusterIndex::new(64, 384, 4);

    // Index 20 docs via precision (simulating LAM-encoded documents)
    for i in 0..20 {
        let emb = sinusoidal_embedding(i, 384);
        let entry = enc.wrap_precision(emb);
        idx.add(i as usize, &entry.embedding);
    }

    idx.build();
    assert_eq!(idx.entry_count(), 20);
    assert_eq!(enc.stats(), (0, 20));

    // Search should work
    let query = sinusoidal_embedding(5, 384);
    let results = idx.search_exact(&query, 1);
    assert!(!results.is_empty());
    assert_eq!(results[0].0, 5);
}

#[test]
fn test_dual_encoder_stats_tracking() {
    let mut enc = DualEncoder::precision_only(64);

    for i in 0..10 {
        enc.wrap_precision(sinusoidal_embedding(i, 384));
    }

    let (static_count, precision_count) = enc.stats();
    assert_eq!(static_count, 0);
    assert_eq!(precision_count, 10);
}

// =============================================================================
// TESTS: WITH STATIC-EMBED FEATURE (run: cargo test --features static-embed)
// =============================================================================

#[cfg(feature = "static-embed")]
mod static_embed_tests {
    use super::*;
    use sca_core::latent_cluster::StaticEncoder;
    use std::time::Instant;

    const MODEL_NAME: &str = "minishlab/potion-base-8M";

    #[test]
    fn test_static_encoder_loads() {
        let enc = StaticEncoder::from_pretrained(MODEL_NAME);
        assert!(enc.is_ok(), "Should load Model2Vec: {:?}", enc.err());
    }

    #[test]
    fn test_static_encoder_produces_embeddings() {
        let enc = StaticEncoder::from_pretrained(MODEL_NAME).unwrap();
        let emb = enc.encode_one("Hello world, this is a test");
        assert!(!emb.is_empty(), "Embedding should not be empty");
        // Model2Vec typically produces 256-dim embeddings
        assert!(emb.len() >= 64, "Embedding dim {} should be >= 64", emb.len());

        // Should be roughly L2-normalized
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(norm > 0.1, "Embedding should have non-trivial magnitude");
    }

    #[test]
    fn test_static_encoder_batch() {
        let enc = StaticEncoder::from_pretrained(MODEL_NAME).unwrap();
        let texts: Vec<String> = (0..10)
            .map(|i| format!("Document number {} about various topics", i))
            .collect();
        let embeddings = enc.encode_batch(&texts);
        assert_eq!(embeddings.len(), 10);
    }

    #[test]
    fn test_dual_encoder_with_static() {
        let mut enc = DualEncoder::with_static(MODEL_NAME, 64).unwrap();
        assert!(enc.has_static());

        let entry = enc.encode_fast("Hello world").unwrap();
        assert_eq!(entry.source, EncoderSource::Static);
        assert!(!entry.embedding.is_empty());

        let (static_count, precision_count) = enc.stats();
        assert_eq!(static_count, 1);
        assert_eq!(precision_count, 0);
    }

    #[test]
    fn test_dual_encoder_add_fast_to_index() {
        let mut enc = DualEncoder::with_static(MODEL_NAME, 64).unwrap();
        let mut idx = LatentClusterIndex::new(64, 256, 4); // 256 source dim for Model2Vec

        for i in 0..20 {
            let text = format!("Document {} about topic {}", i, i % 5);
            enc.add_fast(&mut idx, i, &text);
        }

        idx.build();
        assert_eq!(idx.entry_count(), 20);
        assert_eq!(enc.stats().0, 20); // all via static
    }

    #[test]
    fn test_dual_encoder_mixed_fast_and_precision() {
        let mut enc = DualEncoder::with_static(MODEL_NAME, 64).unwrap();
        let mut idx = LatentClusterIndex::new(64, 384, 4);

        // First 10 docs via fast path (static)
        for i in 0..10 {
            let text = format!("Fast-indexed document {}", i);
            enc.add_fast(&mut idx, i, &text);
        }

        // Next 10 docs via precision path (simulated LAM)
        for i in 10..20 {
            let emb = sinusoidal_embedding(i, 384);
            let entry = enc.wrap_precision(emb);
            idx.add(i as usize, &entry.embedding);
        }

        idx.build();
        assert_eq!(idx.entry_count(), 20);

        let (sc, pc) = enc.stats();
        assert_eq!(sc, 10);
        assert_eq!(pc, 10);
    }

    #[test]
    fn test_dual_encoder_dedup_works_with_fast() {
        let mut enc = DualEncoder::with_static(MODEL_NAME, 64).unwrap();
        let mut idx = LatentClusterIndex::new(64, 256, 4);

        // Index a document
        enc.add_fast_dedup(&mut idx, 0, "unique document content");
        assert_eq!(idx.entry_count(), 1);

        // Try to add same content again — should be rejected by BLAKE3 dedup
        let result = enc.add_fast_dedup(&mut idx, 1, "unique document content");
        assert!(result.is_none());
        assert_eq!(idx.entry_count(), 1);
    }

    #[test]
    fn test_speed_static_vs_simulated_precision() {
        let enc = StaticEncoder::from_pretrained(MODEL_NAME).unwrap();

        let texts: Vec<String> = (0..1000)
            .map(|i| format!("Document number {} about machine learning and artificial intelligence topic {}", i, i % 10))
            .collect();

        // Benchmark: Model2Vec static encode
        let start = Instant::now();
        let embeddings = enc.encode_batch(&texts);
        let static_ms = start.elapsed().as_secs_f64() * 1000.0;

        eprintln!("\n=== DUAL ENCODER SPEED BENCHMARK ===");
        eprintln!("Documents:       {}", texts.len());
        eprintln!("Static dim:      {}", embeddings[0].len());
        eprintln!("Static encode:   {:.1}ms ({:.1} µs/doc)", static_ms, static_ms * 1000.0 / texts.len() as f64);
        eprintln!("Throughput:      {:.0} docs/sec", texts.len() as f64 / (static_ms / 1000.0));

        // For reference: LAM encoding at ~25ms/doc would take 25,000ms for 1000 docs
        let estimated_lam_ms = 25.0 * texts.len() as f64;
        eprintln!("Est. LAM encode: {:.0}ms (at ~25ms/doc)", estimated_lam_ms);
        eprintln!("Speedup:         {:.0}x", estimated_lam_ms / static_ms);

        assert!(static_ms < 5000.0, "1000 docs should encode in <5s, took {:.1}ms", static_ms);
    }
}
