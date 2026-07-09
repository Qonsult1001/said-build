//! Finding #13 red loop: appending to an EXISTING (saved + reopened) brain must
//! hit the O(new) incremental encode path — NOT re-encode the whole brain.
//!
//! Root cause: the incremental-vs-full decision in build_index gates on
//! `corpus_ids` being non-empty (`no_content_mutation = !indexed.is_empty()`).
//! But `corpus_ids` was sourced ONLY from a `CTXT` section that is READ on open
//! yet NEVER written — so every reopened brain had corpus_ids EMPTY, forcing a
//! full re-encode on every `init`/append. The persisted SCA index (SCRM section)
//! already restores `engine.core.get_doc_ids()` on open, so corpus_ids can be
//! reconstructed from it with no format change.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_reopen_incremental_append -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn reopened_brain_has_populated_corpus_ids_for_incremental_append() {
    let path = "test_reopen_incremental.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    // Build an initial brain with several docs, index, save.
    {
        let mut brain = SaidFile::create(path);
        assert!(brain.auto_load_encoder(), "embedded encoder must load");
        for i in 0..8 {
            brain.remember_with_salience(Some(&format!("doc{i}")),
                &format!("Initial document number {i} about topic alpha beta gamma."),
                None, Pillar::Episodic, vec![]);
        }
        brain.build_index().expect("initial build_index");
        brain.save().expect("save");
    }

    // Reopen — the persisted brain. This is where corpus_ids was silently empty.
    let mut brain = SaidFile::open(std::path::Path::new(path)).expect("reopen");
    assert!(brain.auto_load_encoder(), "encoder reloads");

    // INVARIANT (the fix): a reopened, already-indexed brain must report its prior
    // corpus size, so the append below can take the incremental (O(new)) path.
    let prior = brain.corpus_id_count();
    assert!(prior >= 8,
        "reopened brain must expose its {prior} prior corpus ids (was 0 before the #13 fix — \
         which forced every append to a full re-encode)");

    // Append 2 NEW docs and re-index. With corpus_ids populated this is incremental.
    brain.remember_with_salience(Some("doc_new_1"),
        "A freshly appended document about topic delta epsilon.", None, Pillar::Episodic, vec![]);
    brain.remember_with_salience(Some("doc_new_2"),
        "Another freshly appended document about topic zeta eta.", None, Pillar::Episodic, vec![]);
    brain.build_index().expect("append build_index");

    // The corpus grew by the new docs (append preserved the old, did not wipe).
    let after = brain.corpus_id_count();
    assert!(after >= prior + 2,
        "append must GROW the corpus ({prior} -> {after}), preserving prior docs");

    // And recall still works across old + new (no wipe).
    let old_hit = brain.recall("topic alpha beta gamma", 10);
    let new_hit = brain.recall("topic delta epsilon", 10);
    assert!(!old_hit.is_empty(), "prior docs still recallable after append");
    assert!(!new_hit.is_empty(), "newly appended docs recallable");

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
}

/// The REAL-brain sequence that wiped the memory count to 2 on a live `said init`
/// append: build → save → reopen → append new docs → build → SAVE AGAIN → reopen.
/// The first test stops before the second save; the corruption only shows after the
/// post-append save round-trips through disk. Asserts the memory (frame) count SURVIVES.
#[test]
fn append_then_save_then_reopen_preserves_memory_count() {
    let path = "test_append_save_reopen.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    // Initial brain: 12 docs, index, save.
    {
        let mut brain = SaidFile::create(path);
        assert!(brain.auto_load_encoder());
        for i in 0..12 {
            brain.remember_with_salience(Some(&format!("orig{i}")),
                &format!("Original doc {i} covering alpha beta gamma delta."),
                None, Pillar::Episodic, vec![]);
        }
        brain.build_index().expect("build");
        brain.save().expect("save1");
    }
    let count_after_first_save = {
        let b = SaidFile::open(std::path::Path::new(path)).expect("open1");
        b.stats().active_frames
    };
    assert!(count_after_first_save >= 12, "12 docs persisted (got {count_after_first_save})");

    // Reopen, append 3 NEW docs, build (incremental now), SAVE AGAIN.
    {
        let mut brain = SaidFile::open(std::path::Path::new(path)).expect("open2");
        assert!(brain.auto_load_encoder());
        for i in 0..3 {
            brain.remember_with_salience(Some(&format!("new{i}")),
                &format!("Appended doc {i} covering epsilon zeta eta."),
                None, Pillar::Episodic, vec![]);
        }
        brain.build_index().expect("build2");
        brain.save().expect("save2");
    }

    // Reopen and assert the count SURVIVED (bug: it collapsed to ~2).
    let final_count = {
        let b = SaidFile::open(std::path::Path::new(path)).expect("open3");
        b.stats().active_frames
    };
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    assert!(final_count >= count_after_first_save + 3,
        "post-append save must PRESERVE all memories: expected >= {}, got {final_count} \
         (a collapse to a tiny number = the frame-TOC wipe bug)",
        count_after_first_save + 3);
}
