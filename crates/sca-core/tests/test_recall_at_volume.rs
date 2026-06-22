//! verify-scale Phase-1 loop: recall@10 at volume, via the documented `ask` path.
//!
//! Loads N genuinely-distinct labelled facts (each a unique real-world meaning, NOT
//! a templated near-duplicate), fires one paraphrase query per fact through the real
//! `sca_core::ask::ask` fusion (the verb the docs call primary), and measures
//! recall@10 — the documented gate (MTEB MEAN NDCG@10 = 0.9655).
//!
//! RED when recall@10 falls below the gate; GREEN when it meets it.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_recall_at_volume -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

/// (fact stored, paraphrase query that shares minimal vocabulary with the fact).
/// Distinct meanings — a healthy engine must put each fact in the top-10 for its query.
fn dataset() -> Vec<(&'static str, &'static str)> {
    vec![
        ("The Great Barrier Reef lies off the coast of Queensland, Australia.", "where is the world's largest coral system"),
        ("Penicillin was discovered by Alexander Fleming in 1928.", "who found the first antibiotic"),
        ("The violin has four strings tuned in perfect fifths.", "how many strings does a fiddle have"),
        ("Mount Kilimanjaro is the tallest mountain in Africa.", "which African peak rises highest"),
        ("Sourdough bread rises using wild yeast and bacteria.", "what makes tangy bread dough expand"),
        ("The Mariana Trench is the deepest part of the ocean.", "what is the lowest point underwater"),
        ("Vincent van Gogh painted The Starry Night in 1889.", "who made the famous swirly night-sky artwork"),
        ("Honey never spoils if stored in a sealed container.", "which food lasts forever without going bad"),
        ("The speed of light is about 300,000 kilometres per second.", "how fast do photons travel"),
        ("Bamboo can grow nearly a metre in a single day.", "which plant shoots up fastest"),
        ("The human heart beats around 100,000 times per day.", "how often does the cardiac muscle pump daily"),
        ("Saturn's rings are made mostly of ice particles.", "what are the bands around the ringed planet made of"),
        ("The Rosetta Stone helped decode Egyptian hieroglyphs.", "what artefact unlocked ancient pharaonic writing"),
        ("Octopuses have three hearts and blue blood.", "which sea creature has multiple hearts"),
        ("The Amazon produces about a fifth of the world's oxygen.", "which rainforest generates most breathable air"),
        ("Mozart composed his first symphony at age eight.", "which child prodigy wrote orchestral music very young"),
        ("Diamonds are formed under extreme heat and pressure.", "how do the hardest gemstones come to exist"),
        ("The Berlin Wall fell in November 1989.", "when did the barrier dividing Germany come down"),
        ("Sharks existed before trees evolved on land.", "which predator is older than forests"),
        ("The Sahara was once a green and fertile region.", "what desert used to be lush grassland"),
    ]
}

#[test]
fn recall_at_10_holds_at_volume_via_ask() {
    let path = "test_recall_volume.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    let data = dataset();
    // Store each distinct fact with a stable id.
    for (i, (fact, _q)) in data.iter().enumerate() {
        brain.remember_with_salience(Some(&format!("d{i}")), fact, None, Pillar::Episodic, vec![]);
    }
    brain.build_index().expect("build_index");

    // Score recall@10 through the documented primary verb: sca_core::ask::ask.
    let (mut hit1, mut hit10) = (0usize, 0usize);
    let n = data.len();
    let mut misses: Vec<(usize, &str)> = Vec::new();
    for (i, (_fact, q)) in data.iter().enumerate() {
        let gold = format!("d{i}");
        let (cands, _kw) = sca_core::ask::ask(&mut brain, q, 10, false, None);
        let rank = cands.iter().position(|c| c.doc_id == gold);
        match rank {
            Some(0) => { hit1 += 1; hit10 += 1; }
            Some(r) if r < 10 => { hit10 += 1; let _ = r; }
            _ => misses.push((i, q)),
        }
    }
    let _ = std::fs::remove_file(path);

    let r1 = hit1 as f32 / n as f32;
    let r10 = hit10 as f32 / n as f32;
    eprintln!("N={n}  recall@1={r1:.4} ({hit1}/{n})  recall@10={r10:.4} ({hit10}/{n})");
    for (i, q) in &misses { eprintln!("  MISS d{i}: {q}"); }

    // Gate: documented MTEB MEAN NDCG@10 = 0.9655. Allow a small margin for the
    // tiny hand-set; the bar is "nearly everything in top-10", not perfection.
    assert!(r10 >= 0.95, "recall@10 = {r10:.4} is below the documented ~0.9655 gate; misses: {misses:?}");
}
