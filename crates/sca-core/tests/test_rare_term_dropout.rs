//! REGRESSION (diagnosing-bugs loop): a memory whose text contains a RARE exact term must stay in
//! the top-K when that term is queried WITH common filler words around it. Observed on the wiki-link
//! MCP benchmark: "cardiologist" alone → the gold at rank 1 (score 1.00), but "who is our lead
//! cardiologist" → the gold DROPS OUT of the top-10 entirely, while a filler doc ties at the
//! semantic-embedding floor (~0.65) and an unrelated memory scores higher (~0.81). The rare-token
//! exact hit fails to dominate the fused score once common words dilute the query.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_rare_term_dropout -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

/// Plant via the INCREMENTAL index path (build_index after every write) — the CLI/MCP behaviour.
/// This is load-bearing for the repro: a build-once corpus ranked the gold fine, but incremental
/// indexing surfaced the fusion bug (a full 2-term rare match scored exactly 0.55, which the
/// semantic-rerank gate `text && confidence > 0.55` excluded by a hair, so the float rerank
/// overwrote the strong lexical hit with a ~0.0 whitened cosine and dropped the gold out of top-10).
fn plant_incremental(brain: &mut SaidFile, id: &str, text: &str) {
    brain.remember_with_salience(Some(id), text, None, Pillar::Episodic, vec![]);
    let _ = brain.build_index();
}

/// Rank of `gold` in the ask results (usize::MAX if absent).
fn rank_of(brain: &mut SaidFile, query: &str, gold: &str) -> usize {
    let (cands, _) = sca_core::ask::ask(brain, query, 10, false, None);
    cands.iter().position(|c| c.doc_id == gold).unwrap_or(usize::MAX)
}

#[test]
fn rare_exact_term_survives_common_word_dilution() {
    let path = "test_rare_term_dropout.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "encoder loads");

    // The gold: carries the rare term "cardiologist" + a [[cardiology]] wiki-link.
    plant_incremental(&mut brain, "g_cardiology", "Dr. Sarah Lee is our lead cardiologist [[cardiology]].");
    // A sibling sharing the concept (as on the benchmark) — must NOT displace the gold to outside top-10.
    plant_incremental(&mut brain, "s_cardiology", "It handles every heart-surgery consult [[cardiology]].");
    // An unrelated concept cluster that intruded on the benchmark.
    plant_incremental(&mut brain, "g_oncall", "Carol is the current on-call lead this sprint.");
    plant_incremental(&mut brain, "s_oncall", "It took over from Bob last week.");
    // Filler mass — none contains "cardiologist" or "lead"; these tied at ~0.65 and buried the gold.
    // Incremental index+save per write (the MCP path) is the suspected differentiator vs build-once.
    for i in 0..200 {
        plant_incremental(&mut brain, &format!("f_{i}"),
              &format!("Reference note {i}: internal invoice record {i} for stream {}.", i % 29));
    }

    // Sanity: the rare term ALONE finds the gold at rank 1 (the exact-match path works).
    assert_eq!(rank_of(&mut brain, "cardiologist", "g_cardiology"), 0,
        "control: bare rare term must return the gold at rank 1");

    // THE BUG: the same rare term inside a common-word question must keep the gold in the top-10.
    // (Before the complete-match rarity bump it dropped OUT entirely — rank -1.)
    let r = rank_of(&mut brain, "who is our lead cardiologist", "g_cardiology");
    assert!(r < 10,
        "rare-term memory dropped out of top-10 when common words were added to the query \
         (got rank {}) — the exact 'cardiologist' hit failed to dominate the fused score",
        if r == usize::MAX { -1i64 } else { r as i64 });

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
}
