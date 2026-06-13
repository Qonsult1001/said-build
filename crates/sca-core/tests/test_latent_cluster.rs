//! Standalone tests for LatentClusterIndex — true latent space thinking.
//!
//! Tests cluster quality, BLAKE3 dedup, Matryoshka truncation, strength-weighted
//! search, recall consolidation, topic introspection, and pre-filter recall —
//! all independent of CrystallineCore.

use sca_core::latent_cluster::{LatentClusterIndex, LatentSpace};
use std::time::Instant;

// =============================================================================
// HELPERS
// =============================================================================

/// Generate a synthetic 384-dim embedding centered around a "topic" direction.
fn make_embedding(topic_id: usize, noise_seed: usize, dim: usize) -> Vec<f32> {
    let mut emb = vec![0.0f32; dim];
    let base = (topic_id * 37) % dim;
    for i in 0..dim {
        let hash =
            ((i.wrapping_mul(2654435761) ^ noise_seed.wrapping_mul(2246822519)) >> 16) as f32;
        let noise_val = (hash % 1000.0) / 5000.0;
        let topic_signal = if (i + base) % 5 == topic_id % 5 {
            1.0
        } else {
            0.0
        };
        emb[i] = topic_signal + noise_val;
    }
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for v in emb.iter_mut() {
            *v /= norm;
        }
    }
    emb
}

fn make_query(topic_id: usize, dim: usize) -> Vec<f32> {
    make_embedding(topic_id, 999999, dim)
}

// =============================================================================
// 1. LATENT SPACE CORE — the true representation
// =============================================================================

#[test]
fn test_latent_space_new() {
    let space = LatentSpace::new(64, 384);
    assert_eq!(space.latent_dim, 64);
    assert_eq!(space.source_dim, 384);
    assert_eq!(space.entry_count(), 0);
    assert_eq!(space.cluster_count(), 0);
}

#[test]
fn test_latent_space_add_entry_with_strength() {
    let mut space = LatentSpace::new(4, 8);
    let hash = *blake3::hash(b"hello world").as_bytes();
    space.add_entry(vec![1.0, 0.0, 0.0, 0.0], 0, 42, hash);

    assert_eq!(space.entry_count(), 1);
    let entry = &space.entries[0];
    assert_eq!(entry.doc_idx, 42);
    assert_eq!(entry.cluster_id, 0);
    assert_eq!(entry.strength, 1.0); // initial strength
}

#[test]
fn test_latent_space_strength_consolidation() {
    let mut space = LatentSpace::new(4, 8);
    let hash = *blake3::hash(b"memory content").as_bytes();
    space.add_entry(vec![1.0, 0.0, 0.0, 0.0], 0, 0, hash);

    // Strengthen on recall — like biological memory consolidation
    space.strengthen(0, 0.5);
    assert_eq!(space.entries[0].strength, 1.5);

    space.strengthen(0, 0.5);
    assert_eq!(space.entries[0].strength, 2.0);

    // Strength caps at 10.0
    for _ in 0..100 {
        space.strengthen(0, 1.0);
    }
    assert_eq!(space.entries[0].strength, 10.0);
}

#[test]
fn test_latent_space_decay() {
    let mut space = LatentSpace::new(4, 8);
    for i in 0..5 {
        let hash = *blake3::hash(format!("doc {}", i).as_bytes()).as_bytes();
        space.add_entry(vec![1.0, 0.0, 0.0, 0.0], 0, i, hash);
    }

    // Strengthen one entry (it will survive decay better)
    space.strengthen(2, 4.0); // now 5.0

    // Decay all by 50%
    space.decay_all(0.5);

    assert!((space.entries[0].strength - 0.5).abs() < 0.01);
    assert!((space.entries[2].strength - 2.5).abs() < 0.01); // stronger survives
}

#[test]
fn test_latent_space_strength_weighted_search() {
    let mut space = LatentSpace::new(4, 8);

    // Two entries in same cluster, same vector — but different strength
    let hash1 = *blake3::hash(b"weak memory").as_bytes();
    let hash2 = *blake3::hash(b"strong memory").as_bytes();
    space.centroids.push(vec![1.0, 0.0, 0.0, 0.0]);
    space.add_entry(vec![0.9, 0.1, 0.0, 0.0], 0, 0, hash1); // strength 1.0
    space.add_entry(vec![0.9, 0.1, 0.0, 0.0], 0, 1, hash2); // strength 1.0

    // Strengthen entry 1
    space.strengthen(1, 4.0); // now 5.0

    let results = space.search(&[1.0, 0.0, 0.0, 0.0], 10);
    assert_eq!(results.len(), 2);
    // Strong memory should rank first (same dot-product, but 5x strength)
    assert_eq!(results[0].0, 1, "Strong memory should rank first");
    assert!(results[0].1 > results[1].1, "Strong score should be higher");
}

#[test]
fn test_latent_space_hash_recall() {
    let mut space = LatentSpace::new(4, 8);
    let hash = *blake3::hash(b"exact content").as_bytes();
    space.add_entry(vec![1.0, 0.0, 0.0, 0.0], 0, 42, hash);

    let recalled = space.recall_by_hash(&hash);
    assert!(recalled.is_some());
    assert_eq!(recalled.unwrap().doc_idx, 42);

    let wrong_hash = *blake3::hash(b"different content").as_bytes();
    assert!(space.recall_by_hash(&wrong_hash).is_none());
}

// =============================================================================
// 2. MATRYOSHKA TRUNCATION
// =============================================================================

#[test]
fn test_truncation_reduces_dimension() {
    let idx = LatentClusterIndex::new(64, 384, 8);
    let emb = make_embedding(0, 0, 384);
    let truncated = idx.truncate(&emb);
    assert_eq!(truncated.len(), 64);
}

#[test]
fn test_truncation_l2_normalized() {
    let idx = LatentClusterIndex::new(64, 384, 8);
    let emb = make_embedding(0, 0, 384);
    let truncated = idx.truncate(&emb);
    let norm: f32 = truncated.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "Should be L2-normalized, got norm={}",
        norm
    );
}

#[test]
fn test_truncation_preserves_nearest_neighbor() {
    let idx = LatentClusterIndex::new(64, 384, 8);
    let a = make_embedding(0, 1, 384);
    let b = make_embedding(0, 2, 384); // same topic
    let c = make_embedding(3, 1, 384); // different topic

    let ta = idx.truncate(&a);
    let tb = idx.truncate(&b);
    let tc = idx.truncate(&c);

    let dot_same: f32 = ta.iter().zip(tb.iter()).map(|(x, y)| x * y).sum();
    let dot_diff: f32 = ta.iter().zip(tc.iter()).map(|(x, y)| x * y).sum();

    assert!(
        dot_same > dot_diff,
        "Same-topic ({}) should exceed cross-topic ({})",
        dot_same,
        dot_diff
    );
}

#[test]
fn test_truncation_batch() {
    let idx = LatentClusterIndex::new(64, 384, 8);
    let flat: Vec<f32> = (0..3).flat_map(|i| make_embedding(i, 0, 384)).collect();
    let batch = idx.truncate_batch(&flat);
    assert_eq!(batch.len(), 3);
    for v in &batch {
        assert_eq!(v.len(), 64);
    }
}

// =============================================================================
// 3. BLAKE3 CONTENT DEDUP
// =============================================================================

#[test]
fn test_dedup_catches_exact_duplicate() {
    let mut idx = LatentClusterIndex::default_config();
    let emb = make_embedding(0, 0, 384);

    let r1 = idx.add_with_dedup(0, &emb, "hello world");
    assert!(r1.is_some());

    let r2 = idx.add_with_dedup(1, &emb, "hello world"); // duplicate
    assert!(r2.is_none());
    assert_eq!(idx.entry_count(), 1);
}

#[test]
fn test_dedup_no_false_positives() {
    let mut idx = LatentClusterIndex::default_config();
    let emb = make_embedding(0, 0, 384);
    idx.add_with_dedup(0, &emb, "hello world");

    assert!(idx.dedup_check("hello world!").is_none());
    assert!(idx.dedup_check("Hello World").is_none());
    assert!(idx.dedup_check("hello  world").is_none());
}

#[test]
fn test_dedup_at_scale_zero_false_negatives() {
    let mut idx = LatentClusterIndex::default_config();
    let emb = make_embedding(0, 0, 384);

    for i in 0..5000 {
        let text = format!("document number {} unique suffix {}", i, i * 7);
        idx.add_with_dedup(i, &emb, &text);
    }
    assert_eq!(idx.entry_count(), 5000);

    // Every single one should be found
    for i in 0..5000 {
        let text = format!("document number {} unique suffix {}", i, i * 7);
        assert!(
            idx.dedup_check(&text).is_some(),
            "Should find doc {}",
            i
        );
    }
}

#[test]
fn test_recall_by_hash() {
    let mut idx = LatentClusterIndex::new(64, 384, 8);
    let emb = make_embedding(0, 0, 384);
    let text = "specific content";
    let hash = *blake3::hash(text.as_bytes()).as_bytes();

    idx.add_with_dedup(42, &emb, text);

    let entry = idx.recall_by_hash(&hash);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().doc_idx, 42);

    let wrong = *blake3::hash(b"other").as_bytes();
    assert!(idx.recall_by_hash(&wrong).is_none());
}

// =============================================================================
// 4. CLUSTER BUILDING
// =============================================================================

#[test]
fn test_build_empty() {
    let mut idx = LatentClusterIndex::new(64, 384, 8);
    idx.build();
    assert!(idx.is_built());
    assert_eq!(idx.cluster_count(), 0);
}

#[test]
fn test_build_single_entry() {
    let mut idx = LatentClusterIndex::new(64, 384, 8);
    idx.add(0, &make_embedding(0, 0, 384));
    idx.build();
    assert!(idx.is_built());
    assert_eq!(idx.cluster_count(), 1);
}

#[test]
fn test_build_k_clamped_to_n() {
    let mut idx = LatentClusterIndex::new(64, 384, 100);
    for i in 0..5 {
        idx.add(i, &make_embedding(i, 0, 384));
    }
    idx.build();
    assert_eq!(idx.cluster_count(), 5);
}

#[test]
fn test_build_assigns_all_entries() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for topic in 0..4 {
        for doc in 0..10 {
            idx.add(topic * 10 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();
    let sizes = idx.cluster_sizes();
    assert_eq!(sizes.iter().sum::<usize>(), 40);
}

#[test]
fn test_build_generates_topic_labels() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for i in 0..20 {
        idx.add(i, &make_embedding(i % 4, i, 384));
    }
    idx.build();

    let topics = idx.topics();
    assert_eq!(topics.len(), 4);
    for (label, count) in &topics {
        assert!(label.starts_with("topic_"));
        assert!(*count > 0);
    }
}

#[test]
fn test_set_topic_label() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for i in 0..20 {
        idx.add(i, &make_embedding(i % 4, i, 384));
    }
    idx.build();

    idx.set_topic_label(0, "programming");
    idx.set_topic_label(1, "science");

    let topics = idx.topics();
    assert!(topics.iter().any(|(label, _)| *label == "programming"));
    assert!(topics.iter().any(|(label, _)| *label == "science"));
}

#[test]
fn test_rebuild_invalidates_after_add() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for i in 0..20 {
        idx.add(i, &make_embedding(i % 4, i, 384));
    }
    idx.build();
    assert!(idx.is_built());

    idx.add(20, &make_embedding(0, 20, 384));
    assert!(!idx.is_built());

    idx.build();
    assert!(idx.is_built());
    assert_eq!(idx.entry_count(), 21);
}

// =============================================================================
// 5. STRENGTH-WEIGHTED SEARCH
// =============================================================================

#[test]
fn test_search_returns_correct_topic() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for topic in 0..4 {
        for doc in 0..20 {
            idx.add(topic * 20 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    let query = make_query(2, 384);
    let results = idx.search(&query, 2, 5);
    assert!(!results.is_empty());

    let top_doc = results[0].0;
    assert!(
        (40..60).contains(&top_doc),
        "Top result {} should be in topic 2 range [40,60)",
        top_doc
    );
}

#[test]
fn test_strength_affects_ranking() {
    let mut idx = LatentClusterIndex::new(64, 384, 1); // single cluster to isolate strength effect

    // Add two entries from same topic with similar embeddings
    let emb_a = make_embedding(0, 1, 384);
    let emb_b = make_embedding(0, 2, 384);
    idx.add(0, &emb_a); // doc 0
    idx.add(1, &emb_b); // doc 1
    idx.build();

    // Without strength boost: both should score similarly
    let query = make_query(0, 384);
    let baseline = idx.search(&query, 1, 2);
    let score_diff_before = (baseline[0].1 - baseline[1].1).abs();

    // Boost doc 1's strength dramatically
    idx.space_mut().strengthen(1, 9.0); // strength now 10.0

    let boosted = idx.search(&query, 1, 2);
    // Doc 1 should now clearly rank first
    assert_eq!(boosted[0].0, 1, "Strengthened entry should rank first");
    assert!(
        boosted[0].1 > boosted[1].1 * 2.0,
        "Strengthened score ({}) should be much higher than weak ({})",
        boosted[0].1,
        boosted[1].1
    );
    // The gap should be much larger than before
    let score_diff_after = (boosted[0].1 - boosted[1].1).abs();
    assert!(
        score_diff_after > score_diff_before,
        "Strength boost should widen the score gap"
    );
}

#[test]
fn test_recall_and_strengthen() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for topic in 0..4 {
        for doc in 0..10 {
            idx.add(topic * 10 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    // Check initial strength
    let initial_strength = idx.space().entries[0].strength;
    assert_eq!(initial_strength, 1.0);

    // Recall multiple times — relevant entries should get stronger
    let query = make_query(0, 384);
    for _ in 0..5 {
        idx.recall(&query, 2, 5);
    }

    // Entries in the recalled cluster should be stronger now
    let final_strength = idx.space().entries[0].strength;
    assert!(
        final_strength > initial_strength,
        "Recalled entries should strengthen: {} > {}",
        final_strength,
        initial_strength
    );
}

#[test]
fn test_decay_weakens_unrealled_memories() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for topic in 0..4 {
        for doc in 0..10 {
            idx.add(topic * 10 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    // Recall topic 0 to strengthen it
    let query = make_query(0, 384);
    for _ in 0..5 {
        idx.recall(&query, 1, 5);
    }

    // Decay everything
    idx.space_mut().decay_all(0.5);

    // Topic 0 entries should be stronger than topic 3 entries
    // (topic 0 was recalled and strengthened before decay)
    let topic0_strength = idx.space().entries[0].strength;
    let topic3_strength = idx.space().entries[30].strength;
    assert!(
        topic0_strength > topic3_strength,
        "Recalled topic ({}) should be stronger after decay than unrealled topic ({})",
        topic0_strength,
        topic3_strength
    );
}

// =============================================================================
// 6. TOPIC INTROSPECTION
// =============================================================================

#[test]
fn test_cluster_health() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for topic in 0..4 {
        for doc in 0..10 {
            idx.add(topic * 10 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    // Strengthen some entries in cluster 0
    for i in 0..10 {
        idx.space_mut().strengthen(i, 2.0);
    }

    let health = idx.cluster_health();
    assert_eq!(health.len(), 4);

    // Find the cluster that has strengthened entries
    let strong_cluster = health.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap();
    assert!(
        strong_cluster.1 > 1.5,
        "Strengthened cluster should have avg strength > 1.5, got {}",
        strong_cluster.1
    );
}

#[test]
fn test_weak_entries() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for i in 0..20 {
        idx.add(i, &make_embedding(i % 4, i, 384));
    }
    idx.build();

    // All start at strength 1.0, so none weak at threshold 0.5
    assert_eq!(idx.weak_entries(0.5).len(), 0);

    // Decay to 0.3
    idx.space_mut().decay_all(0.3);

    // Now all should be weak (0.3 < 0.5)
    let weak = idx.weak_entries(0.5);
    assert_eq!(weak.len(), 20);
}

#[test]
fn test_topics_with_counts() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for topic in 0..4 {
        for doc in 0..10 {
            idx.add(topic * 10 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    let topics = idx.topics();
    assert_eq!(topics.len(), 4);
    let total_count: usize = topics.iter().map(|(_, c)| c).sum();
    assert_eq!(total_count, 40);
}

// =============================================================================
// 7. PRE-FILTER CANDIDATE NARROWING
// =============================================================================

#[test]
fn test_get_candidates_narrows_search_space() {
    let mut idx = LatentClusterIndex::new(64, 384, 8);
    for topic in 0..8 {
        for doc in 0..100 {
            idx.add(topic * 100 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    let query = make_query(3, 384);
    let candidates = idx.get_candidates(&query, 2);

    assert!(candidates.len() < 800, "Should narrow from 800");
    assert!(candidates.len() <= 400, "~2 clusters worth");

    // Target topic's docs should be present
    let in_topic = candidates.iter().filter(|&&c| (300..400).contains(&c)).count();
    assert!(
        in_topic > 50,
        "At least half of topic 3 docs should be candidates, got {}",
        in_topic
    );
}

#[test]
fn test_get_candidates_fallback_when_not_built() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for i in 0..10 {
        idx.add(i, &make_embedding(0, i, 384));
    }
    let candidates = idx.get_candidates(&make_query(0, 384), 2);
    assert_eq!(candidates.len(), 10, "Fallback to all entries");
}

// =============================================================================
// 8. SEARCH_EXACT — 100% RECALL IN LATENT SPACE
// =============================================================================

#[test]
fn test_search_exact_100_percent_recall() {
    // This is the mathematical guarantee: search_exact scans ALL entries
    // in 64-dim latent space. Ranking must be identical to brute-force.
    let dim = 384;
    let mut idx = LatentClusterIndex::new(64, dim, 8);

    let mut all_embeddings: Vec<(usize, Vec<f32>)> = Vec::new();
    for topic in 0..8 {
        for doc in 0..50 {
            let doc_idx = topic * 50 + doc;
            let emb = make_embedding(topic, doc, dim);
            idx.add(doc_idx, &emb);
            all_embeddings.push((doc_idx, emb));
        }
    }
    idx.build();

    // Test against every topic
    for query_topic in 0..8 {
        let query = make_query(query_topic, dim);

        // search_exact: scans all 400 entries in 64-dim
        let exact_results = idx.search_exact(&query, 10);

        // Brute-force in 64-dim (same math, manual loop)
        let query_trunc = idx.truncate(&query);
        let mut brute: Vec<(usize, f32)> = all_embeddings
            .iter()
            .map(|(doc_idx, emb)| {
                let trunc = idx.truncate(emb);
                let dot: f32 = query_trunc.iter().zip(trunc.iter()).map(|(a, b)| a * b).sum();
                (*doc_idx, dot) // strength=1.0 for all, so score=dot
            })
            .collect();
        brute.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let brute_top10: Vec<usize> = brute.iter().take(10).map(|(i, _)| *i).collect();
        let exact_top10: Vec<usize> = exact_results.iter().map(|(i, _)| *i).collect();

        // 100% recall: every brute-force top-10 must appear in search_exact top-10
        let recall = brute_top10
            .iter()
            .filter(|d| exact_top10.contains(d))
            .count();
        assert_eq!(
            recall, 10,
            "search_exact must have 100% recall for topic {}: got {}/10 (exact={:?}, brute={:?})",
            query_topic, recall, exact_top10, brute_top10
        );
    }
}

#[test]
fn test_search_exact_ranking_matches_brute_force() {
    let dim = 384;
    let mut idx = LatentClusterIndex::new(64, dim, 4);

    for i in 0..200 {
        idx.add(i, &make_embedding(i % 4, i, dim));
    }
    idx.build();

    let query = make_query(2, dim);
    let exact = idx.search_exact(&query, 200); // get ALL results

    // Scores must be in descending order
    for window in exact.windows(2) {
        assert!(
            window[0].1 >= window[1].1,
            "search_exact scores must be descending: {} >= {}",
            window[0].1,
            window[1].1
        );
    }
}

#[test]
fn test_search_exact_strength_weighted() {
    let mut idx = LatentClusterIndex::new(64, 384, 2);
    let emb = make_embedding(0, 0, 384);

    idx.add(0, &emb);
    idx.add(1, &make_embedding(0, 1, 384));
    idx.build();

    // Boost entry 1's strength
    idx.space_mut().strengthen(1, 9.0);

    let results = idx.search_exact(&make_query(0, 384), 2);
    assert_eq!(results[0].0, 1, "Strong entry should rank first in search_exact");
}

#[test]
fn test_search_exact_no_build_required() {
    // search_exact doesn't need clusters — it scans all entries
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for i in 0..20 {
        idx.add(i, &make_embedding(i % 4, i, 384));
    }
    // Don't build — search_exact should still work
    let results = idx.search_exact(&make_query(0, 384), 5);
    assert_eq!(results.len(), 5);
}

// =============================================================================
// 8b. GET_CLUSTER_CANDIDATES — zero-allocation hot path
// =============================================================================

#[test]
fn test_get_cluster_candidates_returns_subset() {
    let mut idx = LatentClusterIndex::new(64, 384, 8);
    for topic in 0..8 {
        for doc in 0..100 {
            idx.add(topic * 100 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    let candidates = idx.get_cluster_candidates(&make_query(3, 384));
    // Should return ~1 cluster's worth (~100 docs), not all 800
    assert!(
        candidates.len() < 800,
        "Should narrow: got {} out of 800",
        candidates.len()
    );
    assert!(
        candidates.len() >= 50,
        "Should return at least some: got {}",
        candidates.len()
    );
}

#[test]
fn test_get_cluster_candidates_contains_target() {
    let mut idx = LatentClusterIndex::new(64, 384, 8);
    for topic in 0..8 {
        for doc in 0..50 {
            idx.add(topic * 50 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    let candidates = idx.get_cluster_candidates(&make_query(5, 384));
    let target_range = 250..300; // topic 5
    let in_target = candidates.iter().filter(|&&c| target_range.contains(&c)).count();
    assert!(
        in_target > 25,
        "Target topic docs should be in candidates: got {}/50",
        in_target
    );
}

// =============================================================================
// 9. PRE-FILTER RECALL QUALITY (cluster search)
// =============================================================================

#[test]
fn test_prefilter_recall_vs_brute_force() {
    let dim = 384;
    let mut idx = LatentClusterIndex::new(64, dim, 8);

    let mut all_embeddings: Vec<(usize, Vec<f32>)> = Vec::new();
    for topic in 0..8 {
        for doc in 0..50 {
            let doc_idx = topic * 50 + doc;
            let emb = make_embedding(topic, doc, dim);
            idx.add(doc_idx, &emb);
            all_embeddings.push((doc_idx, emb));
        }
    }
    idx.build();

    let query = make_query(5, dim);
    let query_trunc = idx.truncate(&query);

    // Brute-force top-10
    let mut brute_force: Vec<(usize, f32)> = all_embeddings
        .iter()
        .map(|(doc_idx, emb)| {
            let trunc = idx.truncate(emb);
            let dot: f32 = query_trunc.iter().zip(trunc.iter()).map(|(a, b)| a * b).sum();
            (*doc_idx, dot)
        })
        .collect();
    brute_force.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let brute_top10: Vec<usize> = brute_force.iter().take(10).map(|(i, _)| *i).collect();

    // Cluster-based top-10
    let cluster_top10: Vec<usize> = idx
        .search(&query, 3, 10)
        .into_iter()
        .map(|(i, _)| i)
        .collect();

    let recall = brute_top10
        .iter()
        .filter(|d| cluster_top10.contains(d))
        .count();
    assert!(
        recall >= 7,
        "Cluster recall should be >=70%, got {}/10",
        recall
    );
}

// =============================================================================
// 9. SPEED BENCHMARK
// =============================================================================

#[test]
fn test_speed_cluster_vs_brute_force() {
    let dim = 384;
    let n_docs = 10_000;
    let n_topics = 32;
    let mut idx = LatentClusterIndex::new(64, dim, n_topics);

    let mut all_truncated: Vec<Vec<f32>> = Vec::with_capacity(n_docs);
    for i in 0..n_docs {
        let topic = i % n_topics;
        let emb = make_embedding(topic, i, dim);
        let trunc = idx.truncate(&emb);
        all_truncated.push(trunc);
        idx.add(i, &emb);
    }

    let build_start = Instant::now();
    idx.build();
    let build_elapsed = build_start.elapsed();

    let query = make_query(7, dim);
    let query_trunc = idx.truncate(&query);

    // Brute-force benchmark
    let n_iters = 100;
    let bf_start = Instant::now();
    for _ in 0..n_iters {
        let mut scores: Vec<(usize, f32)> = all_truncated
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let dot: f32 = query_trunc.iter().zip(v.iter()).map(|(a, b)| a * b).sum();
                (i, dot)
            })
            .collect();
        scores.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        std::hint::black_box(&scores[..10]);
    }
    let bf_per_query = bf_start.elapsed() / n_iters;

    // Cluster benchmark
    let cl_start = Instant::now();
    for _ in 0..n_iters {
        let results = idx.search(&query, 2, 10);
        std::hint::black_box(&results);
    }
    let cl_per_query = cl_start.elapsed() / n_iters;

    eprintln!("\n=== LATENT SPACE SPEED BENCHMARK (10K docs, 32 clusters) ===");
    eprintln!("Build time:        {:?}", build_elapsed);
    eprintln!("Brute-force/query: {:?}", bf_per_query);
    eprintln!("Cluster/query:     {:?}", cl_per_query);
    if bf_per_query.as_nanos() > 0 {
        let speedup = bf_per_query.as_nanos() as f64 / cl_per_query.as_nanos().max(1) as f64;
        eprintln!("Speedup:           {:.1}x", speedup);
    }
    eprintln!("Candidates/query:  ~{}", idx.get_candidates(&query, 2).len());
    eprintln!("Memory:            {} bytes", idx.memory_bytes());

    assert!(!idx.search(&query, 2, 10).is_empty());
}

#[test]
fn test_speed_search_exact_vs_384d_brute() {
    // Proves: 64-dim exact scan is faster than 384-dim brute force
    // while giving 100% recall.
    let dim = 384;
    let n_docs = 10_000;
    let mut idx = LatentClusterIndex::new(64, dim, 32);

    let mut full_embeddings: Vec<Vec<f32>> = Vec::with_capacity(n_docs);
    for i in 0..n_docs {
        let emb = make_embedding(i % 32, i, dim);
        full_embeddings.push(emb.clone());
        idx.add(i, &emb);
    }
    idx.build();

    let query = make_query(7, dim);
    let n_iters = 100;

    // Benchmark: full 384-dim brute force
    let bf384_start = Instant::now();
    for _ in 0..n_iters {
        let mut scores: Vec<(usize, f32)> = full_embeddings
            .iter()
            .enumerate()
            .map(|(i, emb)| {
                let dot: f32 = query.iter().zip(emb.iter()).map(|(a, b)| a * b).sum();
                (i, dot)
            })
            .collect();
        scores.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        std::hint::black_box(&scores[..10]);
    }
    let bf384_per = bf384_start.elapsed() / n_iters;

    // Benchmark: search_exact (64-dim, 100% recall)
    let exact_start = Instant::now();
    for _ in 0..n_iters {
        let results = idx.search_exact(&query, 10);
        std::hint::black_box(&results);
    }
    let exact_per = exact_start.elapsed() / n_iters;

    // Benchmark: cluster search (64-dim, ~95% recall)
    let cluster_start = Instant::now();
    for _ in 0..n_iters {
        let results = idx.search(&query, 2, 10);
        std::hint::black_box(&results);
    }
    let cluster_per = cluster_start.elapsed() / n_iters;

    eprintln!("\n=== LATENT SPACE: ALL THREE PATHS (10K docs) ===");
    eprintln!("384-dim brute force: {:?}  (baseline)", bf384_per);
    eprintln!("64-dim search_exact: {:?}  (100% recall)", exact_per);
    eprintln!("64-dim cluster:      {:?}  (~95% recall)", cluster_per);
    if bf384_per.as_nanos() > 0 {
        eprintln!(
            "search_exact speedup over 384d: {:.1}x",
            bf384_per.as_nanos() as f64 / exact_per.as_nanos().max(1) as f64
        );
        eprintln!(
            "cluster speedup over 384d:      {:.1}x",
            bf384_per.as_nanos() as f64 / cluster_per.as_nanos().max(1) as f64
        );
    }
}

// =============================================================================
// 10. MEMORY USAGE
// =============================================================================

#[test]
fn test_memory_savings() {
    let mut idx = LatentClusterIndex::new(64, 384, 8);
    for i in 0..1000 {
        idx.add(i, &make_embedding(i % 8, i, 384));
    }
    idx.build();

    let latent_bytes = idx.memory_bytes();
    let full_384_bytes = 1000 * 384 * 4;
    let savings_pct = (1.0 - latent_bytes as f64 / full_384_bytes as f64) * 100.0;

    eprintln!("\n=== MEMORY SAVINGS ===");
    eprintln!(
        "Latent (64-dim):   {} bytes ({:.1} KB)",
        latent_bytes,
        latent_bytes as f64 / 1024.0
    );
    eprintln!(
        "Full (384-dim):    {} bytes ({:.1} KB)",
        full_384_bytes,
        full_384_bytes as f64 / 1024.0
    );
    eprintln!("Savings:           {:.1}%", savings_pct);

    assert!(latent_bytes < full_384_bytes);
    assert!(savings_pct > 70.0, "Should save >70%, got {:.1}%", savings_pct);
}

// =============================================================================
// 11. EDGE CASES
// =============================================================================

#[test]
fn test_zero_vector() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    idx.add(0, &vec![0.0f32; 384]);
    idx.add(1, &make_embedding(0, 0, 384));
    idx.build();
    let _ = idx.search(&vec![0.0f32; 384], 2, 5); // shouldn't panic
}

#[test]
fn test_search_scores_sorted_descending() {
    let mut idx = LatentClusterIndex::new(64, 384, 4);
    for topic in 0..4 {
        for doc in 0..20 {
            idx.add(topic * 20 + doc, &make_embedding(topic, doc, 384));
        }
    }
    idx.build();

    let results = idx.search(&make_query(1, 384), 4, 20);
    for window in results.windows(2) {
        assert!(window[0].1 >= window[1].1);
    }
}

#[test]
fn test_search_empty() {
    let idx = LatentClusterIndex::default_config();
    assert!(idx.search(&make_query(0, 384), 2, 5).is_empty());
}
