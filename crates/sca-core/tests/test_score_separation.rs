//! Regression: anisotropy-corrected scoring must keep the score band SEPARATED.
//!
//! Static mean-pooled embeddings live in a narrow cone (Mu & Viswanath 2018,
//! "All-but-the-top"; Ethayarajh 2019), so raw cosine clusters every pair at ~0.45 and a
//! real match barely out-scores noise NUMERICALLY. `ask` corrects this by subtracting the
//! corpus mean before the float rerank cosine, which spreads the band: a relevant match
//! scores clearly high, an off-topic one near zero. This test locks that property in — if
//! someone removes the centering, the bands collapse and this goes red.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_score_separation -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

fn cos(a: &[f32], b: &[f32]) -> f32 {
    let (mut d, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len().min(b.len()) { d += a[i]*b[i]; na += a[i]*a[i]; nb += b[i]*b[i]; }
    d / (na.sqrt().max(1e-12) * nb.sqrt().max(1e-12))
}
fn center(v: &[f32], mu: &[f32]) -> Vec<f32> {
    v.iter().enumerate().map(|(i, x)| x - mu.get(i).copied().unwrap_or(0.0)).collect()
}

#[test]
fn centering_separates_relevant_from_noise() {
    let path = "test_score_sep.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");
    let docs = [
        ("bday", "Mom's birthday is June 14."),
        ("dentist", "My dentist is Dr. Sarah Chen."),
        ("garage", "The garage door code is 4827."),
        ("wifi", "The wifi password is sunflower-42."),
    ];
    for (id, t) in &docs { brain.remember_with_salience(Some(id), t, None, Pillar::Episodic, vec![]); }
    brain.build_index().expect("build_index");

    let mu = brain.engine.core.get_corpus_mean().to_vec();
    assert!(!mu.is_empty(), "corpus mean must be populated for centering");

    let doc_emb: Vec<(&str, Vec<f32>)> = docs.iter()
        .map(|(id, t)| (*id, brain.engine.encode_query(t).expect("enc"))).collect();

    // Returns (top_id, top_centered_cos, second_centered_cos)
    let centered_top2 = |q: &str| -> (&'static str, f32, f32) {
        let qc = center(&brain.engine.encode_query(q).expect("encq"), &mu);
        let mut v: Vec<(f32, &'static str)> = doc_emb.iter()
            .map(|(id, e)| (cos(&qc, &center(e, &mu)), *id)).collect();
        v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        (v[0].1, v[0].0, v[1].0)
    };

    // Relevant queries: correct doc leads, and its centered score is clearly high
    // while the runner-up is clearly lower (a real gap).
    for (q, gold) in [
        ("when is moms birthday", "bday"),
        ("who is my dentist", "dentist"),
        ("what is the garage door code", "garage"),
    ] {
        let (top, top_s, second_s) = centered_top2(q);
        assert_eq!(top, gold, "'{q}': expected {gold} to lead, got {top}");
        assert!(top_s > 0.45, "'{q}': leader centered score {top_s:.3} should be clearly high (>0.45)");
        assert!(top_s - second_s > 0.20, "'{q}': gap {:.3} should be wide (>0.20)", top_s - second_s);
    }

    // Irrelevant queries: even the best centered score is LOW — no confident answer.
    for q in ["how do I bake sourdough bread", "what is the capital of France", "stock market today"] {
        let (_top, top_s, _second) = centered_top2(q);
        assert!(top_s < 0.30, "'{q}': off-topic leader centered score {top_s:.3} should be low (<0.30)");
    }

    let _ = std::fs::remove_file(path);
}

/// End-to-end via the real `ask` pipeline: an off-topic query against a tiny brain must
/// NOT return the whole brain. Before the anisotropy fix, every memory scored ~0.45 and
/// `ask` returned all of them; now the floor+gap on the separated score returns only the
/// best guess (or few), never the full set.
#[test]
fn ask_does_not_return_whole_brain_for_offtopic() {
    use sca_core::ask::ask;
    let path = "test_score_sep_e2e.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder());
    for (id, t) in [
        ("bday", "Mom's birthday is June 14."),
        ("dentist", "My dentist is Dr. Sarah Chen."),
        ("garage", "The garage door code is 4827."),
        ("wifi", "The wifi password is sunflower-42."),
    ] { brain.remember_with_salience(Some(id), t, None, Pillar::Episodic, vec![]); }
    brain.build_index().expect("build_index");

    // Relevant query → the correct memory leads and the noise tail is trimmed.
    let (rel, _) = ask(&mut brain, "when should I call mom", 10, false, None);
    assert!(!rel.is_empty(), "relevant query should return the matching memory");
    assert_eq!(rel[0].doc_id, "bday", "mom's birthday should lead 'when should I call mom'");
    assert!(rel.len() <= 2, "relevant query should not drag in the whole brain (got {})", rel.len());

    // Off-topic query → at most a single best guess, never all 4 memories.
    let (off, _) = ask(&mut brain, "how do I bake sourdough bread", 10, false, None);
    assert!(off.len() <= 2, "off-topic query returned {} of 4 memories (should be <=2)", off.len());

    let _ = std::fs::remove_file(path);
}
