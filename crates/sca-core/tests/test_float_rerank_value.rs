//! VALUE TEST (test-first, per "test first; if no value, remove"): does reranking the
//! top-K ask candidates by FULL 64-dim float cosine (doc embedding kept in memory) fix
//! the same-entity rank-1 misses that 1-bit asymmetric still gets wrong? If this is not
//! materially better than the shipped path, the float-residual idea has no value and we
//! drop it. If it does, it justifies storing a float residual per doc for top-K rerank.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_float_rerank_value -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

fn cos(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    dot / (na * nb)
}

#[test]
fn float_rerank_of_topk_fixes_same_entity() {
    let path = "test_float_rerank.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    let docs = [
        ("hike",   "Mara loves hiking the volcanic trails of Iceland."),
        ("type",   "Mara collects vintage typewriters from the 1930s."),
        ("cello",  "Mara plays the cello in a weekend jazz quartet."),
        ("bake",   "Mara runs a small bakery famous for cardamom buns."),
        ("marine", "Mara is studying marine biology with a focus on coral."),
    ];
    for (id, t) in &docs { brain.remember_with_salience(Some(id), t, None, Pillar::Episodic, vec![]); }
    brain.build_index().expect("build_index");

    // Float doc embeddings kept in memory (this is what a per-doc float residual WOULD
    // give us at rerank time — same 64-dim vector we already truncate to).
    let doc_emb: Vec<(&str, Vec<f32>)> = docs.iter()
        .map(|(id, t)| (*id, brain.engine.encode_query(t).expect("enc"))).collect();

    let queries = [
        ("cello", "which instrument does Mara play at weekends"),
        ("bake",  "what food business does Mara run"),
        ("type",  "what does Mara collect from the 1930s"),
        ("hike",  "what outdoor activity does Mara do in Iceland"),
        ("marine","what science does Mara study about the sea"),
    ];

    let mut shipped_rank1 = 0;
    let mut reranked_rank1 = 0;
    for (gold, q) in &queries {
        // Shipped ask path (1-bit asymmetric + conditional tie-break)
        let (cands, _kw) = sca_core::ask::ask(&mut brain, q, 10, false, None);
        if cands.first().map(|c| c.doc_id.as_str()) == Some(*gold) { shipped_rank1 += 1; }

        // Float rerank: take ask's candidate set, reorder by full-float cosine.
        let qe = brain.engine.encode_query(q).expect("enc q");
        let mut reordered: Vec<(&str, f32)> = cands.iter()
            .filter_map(|c| doc_emb.iter().find(|(id, _)| *id == c.doc_id).map(|(id, e)| (*id, cos(&qe, e))))
            .collect();
        reordered.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top = reordered.first().map(|(id, _)| *id);
        eprintln!("Q '{q}': gold={gold} shipped_top={:?} float_rerank_top={top:?}",
            cands.first().map(|c| c.doc_id.clone()));
        if top == Some(*gold) { reranked_rank1 += 1; }
    }
    let _ = std::fs::remove_file(path);

    eprintln!("SHIPPED rank-1: {shipped_rank1}/5   FLOAT-RERANK rank-1: {reranked_rank1}/5");
    // The value claim: float rerank materially beats the shipped path.
    assert!(reranked_rank1 >= 5,
        "float rerank should fix same-entity (proven 5/5 by cosine); got {reranked_rank1}/5 vs shipped {shipped_rank1}/5");
}
