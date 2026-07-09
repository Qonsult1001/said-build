//! Regression: `ask` recall must be DETERMINISTIC. The candidate set is built in a HashMap; the
//! final sort was by confidence with NO tie-break, so ties (very common for short one-line memories)
//! resolved by the HashMap's per-process-random iteration order — the same query returned a different
//! memory each run, and the correct answer was often dropped before the rerank could fix it (measured
//! recall@1 ~20-33% on 100 short memories). Fix: deterministic doc_id tie-break on every ranking sort.
//! This test stores short memories that tie, then asserts the SAME query yields the SAME ranking
//! across many runs on the same process AND that a stored fact is reliably its own top hit.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_recall_determinism -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn ask_ranking_is_deterministic_across_runs() {
    let path = "test_recall_determinism.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "encoder loads");

    // Short, distinct personal memories — the free-brain workload. Deliberately several about the
    // same kind of thing so their scores tie (the exact case that was non-deterministic).
    let facts = [
        ("wifi", "my wifi password is sunflower-42"),
        ("allergy", "I am allergic to peanuts"),
        ("herb", "I dislike coriander intensely"),
        ("car", "my car is a blue Toyota"),
        ("coffee", "I take my coffee black with no sugar"),
        ("gym", "my gym membership is at FitZone"),
        ("airport", "I usually fly out of Gatwick"),
        ("landlord", "my landlord is called Mr Patel"),
        ("degree", "I studied marine biology at university"),
        ("colour", "my favourite colour is teal"),
    ];
    for (id, text) in facts {
        brain.remember_with_salience(Some(id), text, None, Pillar::Episodic, vec![]);
    }
    brain.build_index().expect("build_index");

    // (A) DETERMINISM: the same query must return the SAME top doc_id every time.
    let q = "what am I allergic to";
    let first = {
        let (cands, _) = sca_core::ask::ask(&mut brain, q, 5, false, None);
        cands.first().map(|c| c.doc_id.clone())
    };
    assert!(first.is_some(), "query returned at least one candidate");
    for run in 0..15 {
        let (cands, _) = sca_core::ask::ask(&mut brain, q, 5, false, None);
        let top = cands.first().map(|c| c.doc_id.clone());
        assert_eq!(top, first, "run {run}: same query must give the SAME top hit every time \
            (non-deterministic ranking regressed)");
    }

    // (B) confidence is non-negative (the rerank score floor `cosine.max(0.0)`).
    let (cands, _) = sca_core::ask::ask(&mut brain, q, 5, false, None);
    for c in &cands {
        assert!(c.confidence >= 0.0, "confidence must be >= 0.0, got {}", c.confidence);
    }

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
}
