//! Same-entity rank-1: one person, many distinct memories, paraphrase queries with
//! NO keyword overlap. The right memory must rank #1 — proven possible because float
//! cosine on the same 64-dim embedding does it 5/5 (see test_signal_diagnostic). This
//! guards the asymmetric (float-query) ranking that recovers that signal from the
//! 1-bit fingerprints, which symmetric Hamming flattened to ties.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_same_entity_rank1 -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn same_entity_paraphrase_ranks_right_memory_first() {
    let path = "test_same_entity.said";
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
    for (id, t) in &docs {
        brain.remember_with_salience(Some(id), t, None, Pillar::Episodic, vec![]);
    }
    brain.build_index().expect("build_index");

    let queries = [
        ("cello", "which instrument does Mara play at weekends"),
        ("bake",  "what food business does Mara run"),
        ("type",  "what does Mara collect from the 1930s"),
        ("hike",  "what outdoor activity does Mara do in Iceland"),
        ("marine","what science does Mara study about the sea"),
    ];
    let mut rank1 = 0;
    for (gold, q) in &queries {
        let (cands, _kw) = sca_core::ask::ask(&mut brain, q, 5, false, None);
        let top = cands.first().map(|c| c.doc_id.as_str());
        eprintln!("Q '{q}': gold={gold} top={top:?}");
        if top == Some(*gold) { rank1 += 1; }
    }
    let _ = std::fs::remove_file(path);

    eprintln!("same-entity rank-1: {rank1}/5");
    assert!(rank1 >= 4, "same-entity paraphrase must rank the right memory #1 (float cosine does 5/5); got {rank1}/5");
}
