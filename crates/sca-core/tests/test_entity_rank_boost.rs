//! Regression: a query naming a SINGLE-WORD entity (a person) must rank that
//! person's own note #1, even when several people share the same topic. The docs'
//! Layer-5 entity-speaker boost claims single-word caps are handled; before the fix
//! only multi-word proper-noun phrases were, so "what does Mara enjoy" ranked other
//! people's same-topic notes above Mara's.
//!
//! Guards the FIX (single-word entity boost) AND the no-regression promise: the
//! multi-word benchmark path is untouched; this only adds single-word handling.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_entity_rank_boost -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn single_word_entity_query_ranks_that_entity_first() {
    let path = "test_entity_boost.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    // Several people, the SAME topic each — only the named entity distinguishes them.
    let people = ["Mara", "Tomas", "Lena", "Owen", "Priya", "Diego", "Yuki", "Hassan"];
    for (i, p) in people.iter().enumerate() {
        brain.remember_with_salience(
            Some(&format!("note{i}")),
            &format!("{p} loves hiking the volcanic trails of Iceland."),
            None, Pillar::Episodic, vec![],
        );
    }
    brain.build_index().expect("build_index");

    // For each person, the entity-named query must rank THAT person's note #1.
    let mut rank1 = 0;
    for (i, p) in people.iter().enumerate() {
        let q = format!("which outdoor activity does {p} enjoy on Icelandic volcanoes");
        let (cands, _kw) = sca_core::ask::ask(&mut brain, &q, 5, false, None);
        let top = cands.first().map(|c| c.doc_id.as_str());
        eprintln!("{p}: top={:?} (want note{i})", top);
        if top == Some(format!("note{i}").as_str()) { rank1 += 1; }
    }
    let _ = std::fs::remove_file(path);

    eprintln!("entity rank-1: {rank1}/{}", people.len());
    assert!(rank1 >= people.len() - 1,
        "named-entity queries must rank that entity #1 (allow 1 slip); got {rank1}/{}", people.len());
}
