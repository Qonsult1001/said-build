use sca_core::quantize::*;

#[test]
fn test_quantize_standard_basic() {
    let config = QuantizeConfig::standard(8);
    let embedding = vec![1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
    let result = quantize_standard(&embedding, &config.corpus_mean, config.quantized_dim);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], 0b01010101);
}

#[test]
fn test_quantize_standard_with_mean() {
    let mean = vec![0.5; 4];
    let embedding = vec![1.0, 0.0, 1.0, 0.0];
    let result = quantize_standard(&embedding, &mean, 1);
    assert_eq!(result[0] & 0x0F, 0b0101);
}

#[test]
fn test_quantize_holographic_16_views() {
    let config = QuantizeConfig::holographic(8);
    let embedding = vec![1.0; 8];
    let result = quantize_holographic(&embedding, &config.corpus_mean, config.quantized_dim, 0.15);
    assert_eq!(result.len(), config.quantized_dim * 16);
}

#[test]
fn test_quantize_batch() {
    let config = QuantizeConfig::standard(4);
    let embeddings = vec![1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0];
    let result = quantize_batch(&embeddings, &config);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_hamming_distance_bytes() {
    assert_eq!(hamming_distance_bytes(&[0xFF], &[0xFF]), 0);
    assert_eq!(hamming_distance_bytes(&[0xFF], &[0x00]), 8);
    assert_eq!(hamming_distance_bytes(&[0b10101010], &[0b01010101]), 8);
    assert_eq!(hamming_distance_bytes(&[0b11110000], &[0b11100000]), 1);
}

#[test]
fn test_hamming_similarity() {
    assert_eq!(hamming_similarity(&[0xFF], &[0xFF]), 1.0);
    assert_eq!(hamming_similarity(&[0xFF], &[0x00]), 0.0);
    let sim = hamming_similarity(&[0b11110000], &[0b11100000]);
    assert!((sim - 0.875).abs() < 0.01);
}

#[test]
fn test_holographic_similarity() {
    let dim = 1;
    let a = vec![0xFF; 16];
    let b = vec![0xFF; 16];
    assert_eq!(holographic_similarity(&a, &b, dim), 1.0);
    let c = vec![0x00; 16];
    assert_eq!(holographic_similarity(&a, &c, dim), 0.0);
}

#[test]
fn test_quantize_config_bytes_per_passage() {
    let std_config = QuantizeConfig::standard(384);
    assert_eq!(std_config.bytes_per_passage(), 48);
    let holo_config = QuantizeConfig::holographic(384);
    assert_eq!(holo_config.bytes_per_passage(), 48 * 16);
}

#[test]
fn test_similar_embeddings_have_low_hamming_distance() {
    let config = QuantizeConfig::standard(384);
    let emb1: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
    let emb2: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0 + 0.001).collect();
    let emb3: Vec<f32> = (0..384).map(|i| -((i as f32) / 384.0)).collect();
    let q1 = quantize_standard(&emb1, &config.corpus_mean, config.quantized_dim);
    let q2 = quantize_standard(&emb2, &config.corpus_mean, config.quantized_dim);
    let q3 = quantize_standard(&emb3, &config.corpus_mean, config.quantized_dim);
    let sim_close = hamming_similarity(&q1, &q2);
    let sim_far = hamming_similarity(&q1, &q3);
    assert!(sim_close > sim_far, "close={} far={}", sim_close, sim_far);
}

#[test]
fn test_dynamic_scale() {
    let mut config = QuantizeConfig::holographic(4);
    let embeddings = vec![1.0, 2.0, 3.0, 4.0, 0.1, 0.2, 0.3, 0.4, 5.0, 6.0, 7.0, 8.0];
    config.compute_dynamic_scale(&embeddings);
    assert!(config.holographic_scale > 0.0);
}
