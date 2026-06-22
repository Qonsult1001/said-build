//! DIAGNOSTIC (not a gate): is the signal to separate one entity's many memories
//! actually present in the 64-dim embedding, or genuinely lost? Compares raw float
//! cosine of query-vs-doc embeddings. If cosine ranks the right Mara note #1 while
//! symmetric Hamming ties them, the limit is the SCORING (1-bit symmetric), not the
//! embedding — and the asymmetric/float path can fix it.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_signal_diagnostic -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;

fn cos(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    dot / (na * nb)
}

#[test]
fn float_cosine_separates_same_entity_memories() {
    let path = "test_signal.said";
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
    let queries = [
        ("cello", "which instrument does Mara play at weekends"),
        ("bake",  "what food business does Mara run"),
        ("type",  "what does Mara collect from the 1930s"),
        ("hike",  "what outdoor activity does Mara do in Iceland"),
        ("marine","what science does Mara study about the sea"),
    ];

    // Encode all docs once.
    let doc_embs: Vec<(&str, Vec<f32>)> = docs.iter()
        .map(|(id, t)| (*id, brain.engine.encode_query(t).expect("encode")))
        .collect();

    let mut cos_rank1 = 0;
    for (gold, q) in &queries {
        let qe = brain.engine.encode_query(q).expect("encode q");
        let mut scored: Vec<(&str, f32)> = doc_embs.iter().map(|(id, de)| (*id, cos(&qe, de))).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top = scored[0].0;
        eprintln!("Q '{q}': gold={gold} cosine-rank: {:?}", scored.iter().map(|(i,s)| format!("{i}:{s:.3}")).collect::<Vec<_>>());
        if top == *gold { cos_rank1 += 1; }
    }
    let _ = std::fs::remove_file(path);

    eprintln!("FLOAT COSINE rank-1: {cos_rank1}/{}", queries.len());
    // If this is high, the signal IS in the embedding — the 1-bit symmetric scoring
    // is what loses it. This drives the fix (use float/asymmetric for tie-break).
    assert!(cos_rank1 >= 4, "if float cosine can't separate them either, it really is the embedding; got {cos_rank1}/5");
}
