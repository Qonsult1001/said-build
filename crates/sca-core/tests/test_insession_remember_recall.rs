//! Red loop for M10: after remember+build_index in ONE process (fresh brain),
//! recall must find the new frame by MEANING. The MCP server hit this — a
//! same-session search after `remember` returned nothing / score 0.000, while a
//! fresh process searching the persisted brain worked. Root suspicion: the
//! in-session incremental index quantizes new fingerprints against an absent/
//! stale corpus mean on a brain that never had a full build.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_insession_remember_recall -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn recall_finds_remembered_frame_same_session() {
    let path = "test_insession_recall.said";
    let _ = std::fs::remove_file(path);

    // Mirror the MCP path: fresh brain, remember two docs, build_index, recall —
    // all in one process, exactly as a single MCP session does.
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    brain.remember_with_salience(Some("medical"),
        "The physician prescribed medication for the ailing patient.", None, Pillar::Episodic, vec![]);
    brain.remember_with_salience(Some("dbms"),
        "The query optimizer chose a hash join plan.", None, Pillar::Episodic, vec![]);
    brain.build_index().expect("build_index");

    // Pure-semantic query: matches `medical` by meaning, shares no vocabulary with
    // either stored doc — so the query words are entirely out-of-corpus-vocabulary.
    // This must route to PureSemantic (1-bit fingerprint) and return real scores.
    // The bug: once the in-session word index was populated by remember/build_index,
    // this all-OOV query routed to FullHybrid, whose BM25-dominant scorer zeroed
    // every doc (score 0.000) since there was no lexical overlap.
    let results = brain.recall("a doctor treating a sick person", 10);
    let _ = std::fs::remove_file(path);

    let medical = results.iter().find(|r| r.doc_id == "medical");
    let dbms = results.iter().find(|r| r.doc_id == "dbms");

    assert!(medical.is_some(), "recall must return the medical doc; got {:?}",
        results.iter().map(|r| &r.doc_id).collect::<Vec<_>>());
    // Real semantic ranking: medical (about doctors) must outscore dbms (unrelated)
    // and carry a non-zero score. Zero score / dbms-on-top == the bug.
    let m = medical.unwrap().score;
    assert!(m > 0.0, "medical score must be > 0 (real SCA), got {}", m);
    if let Some(d) = dbms {
        assert!(m > d.score, "medical ({}) must outrank unrelated dbms ({})", m, d.score);
    }
}
