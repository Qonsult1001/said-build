//! Shared retrieval pipeline — thin wrapper around the ONE canonical
//! `sca_core::recall::search_full` function.
//!
//! Both example binaries (test_folder_recall + mteb_rust) import this module.
//! The actual pipeline logic lives in `sca_core::recall::search_full` inside
//! sca-core. If you need to change retrieval, change it THERE — every caller
//! (said ask CLI, MTEB harness, smoke validator) picks it up automatically.

use std::collections::HashMap;

// Re-export the canonical functions so callers see them as `pipeline::*`.
pub use sca_core::recall::{search_full, PassageEngine};

use sca_core::engine::ScaEngine;

/// Build 512-word passages with 256-word stride from a doc corpus.
/// Returns `(p_ids, p_texts, passage_id → parent_doc_id)`.
pub fn build_passages(
    doc_ids: &[String],
    doc_texts: &[String],
) -> (Vec<String>, Vec<String>, HashMap<String, String>) {
    let mut p_ids = Vec::new();
    let mut p_texts = Vec::new();
    let mut p2d: HashMap<String, String> = HashMap::new();

    for (did, text) in doc_ids.iter().zip(doc_texts.iter()) {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() <= 512 {
            let pid = format!("{}_p0", did);
            p2d.insert(pid.clone(), did.clone());
            p_ids.push(pid);
            p_texts.push(text.clone());
            continue;
        }
        let mut i = 0usize;
        let mut pi = 0usize;
        loop {
            let end = (i + 512).min(words.len());
            let pid = format!("{}_p{}", did, pi);
            p2d.insert(pid.clone(), did.clone());
            p_ids.push(pid);
            p_texts.push(words[i..end].join(" "));
            if end >= words.len() {
                break;
            }
            i += 256;
            pi += 1;
        }
    }
    (p_ids, p_texts, p2d)
}

/// NDCG@10 — ported from pytrec_eval's relevance_evaluator.
pub fn ndcg_at_k(
    ranked: &[(String, f32)],
    qrels: &HashMap<String, i32>,
    k: usize,
) -> f64 {
    let mut dcg = 0.0f64;
    for (rank, (doc_id, _)) in ranked.iter().take(k).enumerate() {
        let rel = *qrels.get(doc_id).unwrap_or(&0) as f64;
        if rel > 0.0 {
            let gain = (2f64).powf(rel) - 1.0;
            let discount = ((rank + 2) as f64).log2();
            dcg += gain / discount;
        }
    }
    let mut ideal_rels: Vec<i32> = qrels.values().copied().filter(|r| *r > 0).collect();
    ideal_rels.sort_unstable_by(|a, b| b.cmp(a));
    let mut idcg = 0.0f64;
    for (rank, rel) in ideal_rels.iter().take(k).enumerate() {
        let gain = (2f64).powf(*rel as f64) - 1.0;
        let discount = ((rank + 2) as f64).log2();
        idcg += gain / discount;
    }
    if idcg > 0.0 { dcg / idcg } else { 0.0 }
}

/// Helper: build a PassageEngine from doc corpus + load encoder.
pub fn build_passage_engine(
    doc_ids: &[String],
    doc_texts: &[String],
    encoder_path: &str,
) -> Result<(sca_core::recall::PassageEngine, HashMap<String, String>), String> {
    let mut pe = sca_core::recall::PassageEngine::new();
    pe.engine.load_static_encoder(encoder_path)?;
    pe.engine.core.set_holographic_16view(false, None);

    let (p_ids, p_texts, p2d_external) = build_passages(doc_ids, doc_texts);
    // Store p2d inside the PassageEngine itself
    pe.passage_to_doc = p2d_external.clone();
    pe.passage_ids = p_ids.clone();

    pe.engine.index_batch(&p_ids, &p_texts)
        .map_err(|e| format!("passage index_batch: {}", e))?;

    Ok((pe, p2d_external))
}
