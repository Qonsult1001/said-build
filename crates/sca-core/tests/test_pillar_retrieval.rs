//! Decision 2 acceptance test — pillar-filtered retrieval.
//!
//! Verifies that:
//! 1. `search_full_scoped_pillars` with `pillar_scope=None` is identical to
//!    `search_full` (zero regression — proves we can ship Decision 2 without
//!    perturbing MTEB / LoCoMo baseline).
//! 2. `search_full_scoped_pillars` with `pillar_scope=Some(...)` drops
//!    frames whose pillar is not in the wanted set, even when they'd
//!    otherwise score high.
//! 3. `rerank_by_pillar` applies exponential recency decay ONLY to Episodic
//!    frames, leaves everything else unchanged (no silent regression on
//!    Code / Semantic / Procedural).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use sca_core::engine::ScaEngine;
use sca_core::frames::Pillar;
use sca_core::recall::{rerank_by_pillar, search_full, search_full_scoped_pillars};

fn find_encoder() -> Option<&'static str> {
    let paths: [&'static str; 3] = [
        "said-lam-static",
        "SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
    ];
    for p in paths {
        if Path::new(p).exists() {
            return Some(p);
        }
    }
    None
}

fn build_engine_with_docs(doc_ids: &[&str], doc_texts: &[&str]) -> Option<ScaEngine> {
    // Tests need the static encoder — without it, index_batch refuses.
    // If the encoder checkout isn't present (e.g. fresh CI clone), skip
    // the test gracefully instead of failing.
    let enc = find_encoder()?;
    let mut eng = ScaEngine::new();
    eng.load_static_encoder(enc).ok()?;
    eng.core.set_holographic_16view(false, None);
    let ids: Vec<String> = doc_ids.iter().map(|s| s.to_string()).collect();
    let texts: Vec<String> = doc_texts.iter().map(|s| s.to_string()).collect();
    eng.index_batch(&ids, &texts).ok()?;
    Some(eng)
}

#[test]
fn pillar_scope_none_is_identical_to_plain_search_full() {
    let ids = vec!["doc-a", "doc-b", "doc-c"];
    let texts = vec![
        "rust memory safety via ownership",
        "python has duck typing",
        "go uses goroutines for concurrency",
    ];
    let Some(mut eng) = build_engine_with_docs(&ids, &texts) else {
        eprintln!("skipping — static encoder not available");
        return;
    };

    let corpus_ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    let corpus_texts: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
    let corpus_lower: Vec<String> = texts.iter().map(|s| s.to_lowercase()).collect();

    let baseline = search_full(
        &mut eng,
        None,
        "rust ownership",
        None,
        10,
        &corpus_ids,
        &corpus_texts,
        &corpus_lower,
    );

    // Same call with pillar_scope=None, corpus_pillars=None.
    let with_nones = search_full_scoped_pillars(
        &mut eng,
        None,
        "rust ownership",
        None,
        10,
        &corpus_ids,
        &corpus_texts,
        &corpus_lower,
        None,
        None,
        None,
    );

    assert_eq!(
        baseline.len(),
        with_nones.len(),
        "result length must match"
    );
    for (a, b) in baseline.iter().zip(with_nones.iter()) {
        assert_eq!(a.0, b.0, "doc_id order must match");
    }
}

#[test]
fn pillar_scope_drops_out_of_scope_frames() {
    // Three docs, each tagged with a different pillar. Query matches all
    // three. Filtering to just Episodic should return ONLY doc-a.
    let ids = vec!["doc-a", "doc-b", "doc-c"];
    let texts = vec![
        "meeting minutes yesterday afternoon",
        "meeting minutes yesterday afternoon",
        "meeting minutes yesterday afternoon",
    ];
    let pillars = vec![Pillar::Episodic, Pillar::Semantic, Pillar::Code];

    let Some(mut eng) = build_engine_with_docs(&ids, &texts) else {
        eprintln!("skipping — static encoder not available");
        return;
    };
    let corpus_ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    let corpus_texts: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
    let corpus_lower: Vec<String> = texts.iter().map(|s| s.to_lowercase()).collect();

    let mut wanted = HashSet::new();
    wanted.insert(Pillar::Episodic);

    let hits = search_full_scoped_pillars(
        &mut eng,
        None,
        "meeting minutes",
        None,
        10,
        &corpus_ids,
        &corpus_texts,
        &corpus_lower,
        None,
        Some(&wanted),
        Some(&pillars),
    );

    assert!(!hits.is_empty(), "expected at least one Episodic hit");
    for (did, _) in &hits {
        assert_eq!(
            did, "doc-a",
            "pillar filter should keep only Episodic frames"
        );
    }
}

#[test]
fn rerank_by_pillar_applies_recency_only_to_episodic() {
    // Three docs, identical SCA = 0.80.
    //   doc-new  : Episodic, 1h old      → ~0.80 × exp(-1/24)  ≈ 0.767
    //   doc-old  : Episodic, 240h old    → ~0.80 × exp(-10)    ≈ 0.000036
    //   doc-sem  : Semantic, 240h old    → 0.80 (unchanged)
    // Expected order after rerank:  doc-sem, doc-new, doc-old.
    // The test's job: Semantic must NOT have decayed; stale Episodic MUST
    // have decayed below recent Episodic.
    let hits: Vec<(String, f32)> = vec![
        ("doc-new".to_string(), 0.80),
        ("doc-old".to_string(), 0.80),
        ("doc-sem".to_string(), 0.80),
    ];

    let mut pillar_of: HashMap<String, Pillar> = HashMap::new();
    pillar_of.insert("doc-new".to_string(), Pillar::Episodic);
    pillar_of.insert("doc-old".to_string(), Pillar::Episodic);
    pillar_of.insert("doc-sem".to_string(), Pillar::Semantic);

    let mut age_hours_of: HashMap<String, f32> = HashMap::new();
    age_hours_of.insert("doc-new".to_string(), 1.0);
    age_hours_of.insert("doc-old".to_string(), 240.0);
    age_hours_of.insert("doc-sem".to_string(), 240.0);

    let ranked = rerank_by_pillar(&hits, &pillar_of, &age_hours_of, 24.0);

    // 1. Semantic score must be UNCHANGED (0.80 exactly).
    let sem = ranked.iter().find(|(d, _)| d == "doc-sem").unwrap();
    assert!(
        (sem.1 - 0.80).abs() < 1e-5,
        "Semantic score must be unchanged, got {}",
        sem.1
    );

    // 2. Recent Episodic score must be in (original × exp(-1/24)) ballpark.
    let new_ep = ranked.iter().find(|(d, _)| d == "doc-new").unwrap();
    let expected_new = 0.80f32 * (-1.0f32 / 24.0).exp();
    assert!(
        (new_ep.1 - expected_new).abs() < 1e-4,
        "recent Episodic: expected ≈ {}, got {}",
        expected_new,
        new_ep.1
    );

    // 3. Stale Episodic must have been penalized far below recent Episodic.
    let old_ep = ranked.iter().find(|(d, _)| d == "doc-old").unwrap();
    assert!(
        old_ep.1 < new_ep.1,
        "stale Episodic ({}) must rank below recent Episodic ({})",
        old_ep.1,
        new_ep.1
    );

    // 4. Semantic (unchanged 0.80) should top the list — Decision 2 is
    //    OPT-IN; if you ask for Episodic decay, you ARE asking to prefer
    //    non-Episodic hits when they tie in SCA. This is the expected
    //    tradeoff; callers who don't want it skip rerank_by_pillar.
    assert_eq!(
        ranked[0].0, "doc-sem",
        "with recency decay on Episodic only, Semantic 0.80 should top"
    );
}
