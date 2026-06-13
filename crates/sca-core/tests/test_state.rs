use sca_core::ScaEngine;
use sca_core::state;

#[test]
fn test_serialize_deserialize_roundtrip() {
    let mut engine = ScaEngine::new();
    engine.stream_index("doc1", "Hello world this is Rust programming", 100_000);
    engine.stream_index("doc2", "Python machine learning deep neural", 100_000);
    let bytes = state::serialize_state(&engine).expect("serialize failed");
    assert!(!bytes.is_empty());
    let mut restored = state::deserialize_state(&bytes).expect("deserialize failed");
    assert_eq!(restored.doc_count(), 2);
    let results = restored.search("rust programming", 5, None);
    assert!(!results.is_empty());
    assert_eq!(results[0].doc_id, "doc1");
}

#[test]
fn test_empty_engine_roundtrip() {
    let engine = ScaEngine::new();
    let bytes = state::serialize_state(&engine).expect("serialize failed");
    let restored = state::deserialize_state(&bytes).expect("deserialize failed");
    assert_eq!(restored.doc_count(), 0);
}

#[test]
fn test_compression_saves_space() {
    let mut engine = ScaEngine::new();
    for i in 0..100 {
        engine.stream_index(
            &format!("doc{}", i),
            &format!("document number {} with some text content about programming and machine learning", i),
            100_000,
        );
    }
    let bytes = state::serialize_state(&engine).expect("serialize failed");
    // Compare against uncompressed bincode 2.x
    let uncompressed = bincode::serde::encode_to_vec(&engine, bincode::config::standard())
        .expect("bincode encode failed");
    assert!(bytes.len() < uncompressed.len(),
        "compressed {} should be < uncompressed {}", bytes.len(), uncompressed.len());
}
