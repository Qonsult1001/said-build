//! Regression: `ask` collapses identical-content results (kept once, highest
//! confidence), so the same fact stored under several ids does not fill several
//! top-K slots — which would waste the result budget and an LLM's context window.
//! Storage is untouched: every id is still independently retrievable by `get`.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_ask_content_dedup -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn ask_collapses_identical_content_but_storage_keeps_all_ids() {
    let path = "test_ask_dedup.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    // Same fact under three ids + one distinct fact.
    for id in ["codeA", "codeB", "codeC"] {
        brain.remember_with_salience(Some(id), "The launch code is alpha-tango-nine.", None, Pillar::Episodic, vec![]);
    }
    brain.remember_with_salience(Some("dog"), "The dog sleeps on the porch.", None, Pillar::Episodic, vec![]);
    brain.build_index().expect("build_index");

    let (cands, _kw) = sca_core::ask::ask(&mut brain, "what is the launch code", 10, false, None);
    let launch_hits = cands.iter()
        .filter(|c| c.content == "The launch code is alpha-tango-nine.")
        .count();
    eprintln!("launch-code results returned: {launch_hits} (expect 1)");
    assert_eq!(launch_hits, 1, "ask must collapse identical content to one result, got {launch_hits}");

    // Storage intact: each id still resolves to the content.
    for id in ["codeA", "codeB", "codeC"] {
        assert_eq!(brain.get(id).as_deref(), Some("The launch code is alpha-tango-nine."),
            "id {id} must still be independently retrievable (result-dedup, not storage-merge)");
    }
    let _ = std::fs::remove_file(path);
}
