use sca_core::hdc::{HyperVector, HDCEngine};

#[test]
fn test_hypervector_random() {
    let v = HyperVector::random();
    let ones = v.hamming_distance(&HyperVector::zeros());
    assert!(ones > 3000 && ones < 7000, "Expected ~50% ones, got {}", ones);
}

#[test]
fn test_hypervector_from_seed_deterministic() {
    let v1 = HyperVector::from_seed("test_seed_123");
    let v2 = HyperVector::from_seed("test_seed_123");
    assert_eq!(v1.similarity(&v2), 1.0);
    let v3 = HyperVector::from_seed("different_seed");
    let sim = v1.similarity(&v3);
    assert!(sim < 0.55 && sim > 0.45, "Different seeds should be ~0.5, got {}", sim);
}

#[test]
fn test_bind_is_xor() {
    let a = HyperVector::random();
    let b = HyperVector::random();
    let bound = a.bind(&b);
    let recovered = bound.unbind(&b);
    assert_eq!(recovered.similarity(&a), 1.0);
}

#[test]
fn test_bundle_majority_vote() {
    let a = HyperVector::from_seed("concept_a");
    let b = HyperVector::from_seed("concept_b");
    let c = HyperVector::from_seed("concept_c");
    let bundled = HyperVector::bundle(&[a.clone(), a.clone(), b.clone()]);
    let sim_a = bundled.similarity(&a);
    let sim_b = bundled.similarity(&b);
    assert!(sim_a > sim_b, "sim_a={} should be > sim_b={}", sim_a, sim_b);
    let sim_c = bundled.similarity(&c);
    assert!(sim_a > sim_c, "sim_a={} should be > sim_c={}", sim_a, sim_c);
}

#[test]
fn test_similarity_range() {
    let a = HyperVector::random();
    let b = HyperVector::random();
    let sim = a.similarity(&b);
    assert!(sim >= 0.0 && sim <= 1.0);
    assert_eq!(a.similarity(&a), 1.0);
}

#[test]
fn test_hamming_distance() {
    let a = HyperVector::from_seed("same");
    let b = HyperVector::from_seed("same");
    assert_eq!(a.hamming_distance(&b), 0);
    let c = HyperVector::from_seed("different");
    let dist = a.hamming_distance(&c);
    assert!(dist > 4000 && dist < 6000, "Expected ~5000, got {}", dist);
}

#[test]
fn test_zeros() {
    let z = HyperVector::zeros();
    assert_eq!(z.hamming_distance(&HyperVector::zeros()), 0);
}

#[test]
fn test_hdc_engine_concept_memory() {
    let mut engine = HDCEngine::new();
    let v1 = engine.get_vector("hello");
    let v2 = engine.get_vector("hello");
    assert_eq!(v1.similarity(&v2), 1.0);
    assert_eq!(engine.concept_count(), 1);
}

#[test]
fn test_hdc_engine_encode_and_search() {
    let mut engine = HDCEngine::new();
    engine.encode_document("doc1", &[("color", "red"), ("size", "large")]);
    engine.encode_document("doc2", &[("color", "blue"), ("size", "small")]);
    let results = engine.search_fuzzy("color", "red");
    assert!(!results.is_empty());
    let doc1_score = results.iter().find(|(id, _)| id == "doc1").map(|(_, s)| *s).unwrap_or(0.0);
    let doc2_score = results.iter().find(|(id, _)| id == "doc2").map(|(_, s)| *s).unwrap_or(0.0);
    assert!(doc1_score > doc2_score, "doc1={} should beat doc2={}", doc1_score, doc2_score);
}

#[test]
fn test_hdc_engine_extract_attribute() {
    let mut engine = HDCEngine::new();
    engine.encode_document("doc1", &[("name", "alice"), ("role", "engineer")]);
    let result = engine.extract_attribute("doc1", "name");
    assert!(result.is_some());
    let (_best_match, score) = result.unwrap();
    assert!(score > 0.45, "Score should be above noise floor, got {}", score);
}

#[test]
fn test_ssp_init_and_encode() {
    let mut engine = HDCEngine::new();
    assert!(!engine.is_ssp_enabled());
    engine.init_ssp();
    assert!(engine.is_ssp_enabled());
    let embedding: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
    let entity = engine.encode_entity(&embedding);
    assert!(entity.is_some());
}

#[test]
fn test_ssp_entity_similarity() {
    let mut engine = HDCEngine::new();
    engine.init_ssp();
    let emb1: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
    let emb2: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0 + 0.001).collect();
    let emb3: Vec<f32> = (0..384).map(|i| -((i as f32) / 384.0)).collect();
    engine.ssp_register_entity("doc1", &emb1);
    assert_eq!(engine.ssp_entity_count(), 1);
    let sim_similar = engine.ssp_entity_similarity(&emb2, "doc1");
    let sim_opposite = engine.ssp_entity_similarity(&emb3, "doc1");
    assert!(sim_similar > sim_opposite, "similar={} should be > opposite={}", sim_similar, sim_opposite);
}

#[test]
fn test_hdc_engine_clear() {
    let mut engine = HDCEngine::new();
    engine.get_vector("test");
    engine.encode_document("doc1", &[("a", "b")]);
    engine.init_ssp();
    let emb: Vec<f32> = vec![0.0; 384];
    engine.ssp_register_entity("doc1", &emb);
    engine.clear();
    assert_eq!(engine.concept_count(), 0);
    assert_eq!(engine.doc_count(), 0);
    assert_eq!(engine.ssp_entity_count(), 0);
}
