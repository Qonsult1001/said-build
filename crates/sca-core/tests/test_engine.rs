use sca_core::ScaEngine;
use sca_core::quantize::QuantizeConfig;
use sca_core::state;

#[test]
fn test_stream_index_basic() {
    let mut engine = ScaEngine::new();
    let stats = engine.stream_index("doc1", "Hello world this is a test document about Rust programming", 100_000);
    assert!(stats["chunks"] >= 1);
    assert!(stats["tokens"] > 0);
    assert_eq!(engine.doc_count(), 1);
}

#[test]
fn test_stream_index_multiple_docs() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "Python is dynamically typed", 100_000);
    engine.stream_index("doc2", "Rust is statically typed and memory safe", 100_000);
    engine.stream_index("doc3", "JavaScript runs in the browser", 100_000);
    assert_eq!(engine.doc_count(), 3);
    assert!(engine.vocab_size() > 5);
}

#[test]
fn test_search_lexical_finds_matching_doc() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "Python is dynamically typed programming language", 100_000);
    engine.stream_index("doc2", "Rust is statically typed and memory safe", 100_000);
    let results = engine.search("dynamically typed", 10, None);
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "doc1");
}

#[test]
fn test_search_returns_scored_results() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "machine learning with Python and TensorFlow deep neural networks", 100_000);
    engine.stream_index("doc2", "web development with JavaScript and React frontend", 100_000);
    let results = engine.search("machine learning neural networks", 10, None);
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "doc1");
    assert!(results[0].score > 0.0);
}

#[test]
fn test_search_with_embeddings() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "artificial intelligence", 100_000);
    engine.stream_index("doc2", "cooking recipes", 100_000);
    engine.add_embedding("doc1", vec![1.0, 0.0, 0.0, 0.0]);
    engine.add_embedding("doc2", vec![0.0, 1.0, 0.0, 0.0]);
    let query_emb = vec![0.9, 0.1, 0.0, 0.0];
    let results = engine.search("intelligence", 10, Some(&query_emb));
    assert!(!results.is_empty());
}

#[test]
fn test_search_exact() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "The quick brown fox jumps over the lazy dog", 100_000);
    engine.stream_index("doc2", "Hello world", 100_000);
    let results = engine.search_exact("brown fox");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, "doc1");
}

#[test]
fn test_recall_context() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "The answer to everything is forty two according to Douglas Adams", 100_000);
    let result = engine.recall_context("forty two", 10);
    assert!(result.is_some());
    let (doc_id, context) = result.unwrap();
    assert_eq!(doc_id, "doc1");
    assert!(context.contains("forty two"));
}

#[test]
fn test_index_many() {
    let mut engine = ScaEngine::new();
    engine.index_many(&[("doc1", "first document"), ("doc2", "second document"), ("doc3", "third document")]);
    assert_eq!(engine.doc_count(), 3);
}

#[test]
fn test_recall_alias() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "Rust programming language", 100_000);
    let search_results = engine.search("rust programming", 5, None);
    let recall_results = engine.recall("rust programming", 5, None);
    assert_eq!(search_results.len(), recall_results.len());
}

#[test]
fn test_clear() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "some text here", 100_000);
    assert_eq!(engine.doc_count(), 1);
    engine.clear();
    assert_eq!(engine.doc_count(), 0);
    assert_eq!(engine.token_count(), 0);
    assert_eq!(engine.vocab_size(), 0);
}

#[test]
fn test_reindex_same_doc() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "original text content", 100_000);
    engine.stream_index("doc1", "updated text content new", 100_000);
    assert_eq!(engine.doc_count(), 1);
    let results = engine.search_exact("updated");
    assert_eq!(results.len(), 1);
}

#[test]
fn test_chunked_indexing() {
    let mut engine = ScaEngine::new();
    let long_text = "word ".repeat(10_000);
    let stats = engine.stream_index("doc1", &long_text, 10_000);
    assert!(stats["chunks"] >= 5);
}

#[test]
fn test_code_query_routes_lexical() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "the passkey is abc123xyz", 100_000);
    engine.stream_index("doc2", "hello world document", 100_000);
    let results = engine.search("passkey abc123xyz", 10, None);
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "doc1");
}

#[test]
fn test_index_1000_docs_recall_at_10() {
    let mut engine = ScaEngine::new();
    let topics = [
        "machine learning neural network deep",
        "web development javascript react frontend",
        "database sql postgresql query optimization",
        "rust programming memory safety ownership",
        "python data science pandas numpy",
        "cloud computing kubernetes docker container",
        "security encryption authentication authorization",
        "mobile development ios android flutter",
        "devops cicd pipeline deployment automation",
        "blockchain cryptocurrency ethereum smart contract",
    ];
    for i in 0..1000 {
        let topic = topics[i % topics.len()];
        let text = format!("document {} about {} with unique content id_{}", i, topic, i);
        engine.stream_index(&format!("doc{}", i), &text, 100_000);
    }
    assert_eq!(engine.doc_count(), 1000);
    let results = engine.search("rust programming memory safety", 10, None);
    assert_eq!(results.len(), 10);
    let rust_hits: usize = results.iter()
        .filter(|h| {
            let idx: usize = h.doc_id.strip_prefix("doc").unwrap().parse().unwrap();
            idx % 10 == 3
        })
        .count();
    assert!(rust_hits >= 8, "At least 8 of top-10 should be rust docs, got {}", rust_hits);
}

#[test]
fn test_quantized_search_with_embeddings() {
    let mut engine = ScaEngine::new();
    let dim = 16;
    engine.set_quantize_config(QuantizeConfig::standard(dim));
    for i in 0..5 {
        engine.stream_index(&format!("doc{}", i), &format!("document about topic {}", i), 100_000);
    }
    let ids: Vec<String> = (0..5).map(|i| format!("doc{}", i)).collect();
    let mut embeddings_flat = Vec::new();
    for i in 0..5 {
        let mut emb = vec![0.0f32; dim];
        emb[i % dim] = 1.0;
        embeddings_flat.extend(emb);
    }
    let passage_counts = vec![1usize; 5];
    engine.add_docs_quantized(&ids, &embeddings_flat, &passage_counts);
    assert!(engine.is_quantized_mode());
    let mut query_emb = vec![0.0f32; dim];
    query_emb[0] = 1.0;
    let results = engine.search_unified_quantized(&query_emb, "document topic", 5);
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "doc0");
}

#[test]
fn test_quantized_holographic_search() {
    let mut engine = ScaEngine::new();
    let dim = 16;
    engine.set_quantize_config(QuantizeConfig::holographic(dim));
    engine.stream_index("doc_a", "alpha document about science", 100_000);
    engine.stream_index("doc_b", "beta document about cooking", 100_000);
    let ids = vec!["doc_a".to_string(), "doc_b".to_string()];
    let mut emb_flat = Vec::new();
    let mut emb_a = vec![1.0f32; dim / 2];
    emb_a.extend(vec![-1.0f32; dim / 2]);
    emb_flat.extend(&emb_a);
    let mut emb_b = vec![-1.0f32; dim / 2];
    emb_b.extend(vec![1.0f32; dim / 2]);
    emb_flat.extend(&emb_b);
    engine.add_docs_quantized(&ids, &emb_flat, &[1, 1]);
    let mut q = vec![0.9f32; dim / 2];
    q.extend(vec![-0.9f32; dim / 2]);
    let results = engine.search_unified_quantized(&q, "", 5);
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "doc_a");
}

#[test]
fn test_roundtrip_preserves_search_results() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "Rust is a systems programming language", 100_000);
    engine.stream_index("doc2", "Python is great for data science", 100_000);
    engine.stream_index("doc3", "JavaScript powers the modern web", 100_000);
    let results_before = engine.search("rust systems programming", 3, None);
    let bytes = state::serialize_state(&engine).unwrap();
    let mut restored = state::deserialize_state(&bytes).unwrap();
    let results_after = restored.search("rust systems programming", 3, None);
    assert_eq!(results_before.len(), results_after.len());
    for (a, b) in results_before.iter().zip(results_after.iter()) {
        assert_eq!(a.doc_id, b.doc_id);
        assert!((a.score - b.score).abs() < 0.001);
    }
}
