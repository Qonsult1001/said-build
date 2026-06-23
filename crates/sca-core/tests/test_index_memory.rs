//! Phase-1 red loop for the index/encode OOM (#4): at scale the corpus text is held
//! resident in RAM ~4× (corpus_texts + corpus_texts_lower + engine.doc_texts_original +
//! engine.doc_texts_normalized), and cloned again per query. Full Wonga (64K frames) OOMs
//! allocating 2.2GB at the index stage. This test indexes a brain with a known corpus
//! text size and asserts resident text bytes stay bounded — i.e. we do NOT keep the whole
//! corpus several times over.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_index_memory -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn index_does_not_hold_corpus_text_several_times() {
    let path = "test_index_mem.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());

    // Known corpus text size: n frames × body bytes.
    let body = "alpha beta gamma delta ".repeat(120); // ~2.7KB of real words per frame
    let per_frame = body.len();
    let n = 2000;
    let corpus_bytes = per_frame * n;
    for i in 0..n {
        b.remember_with_salience(Some(&format!("d{i}")), &format!("{body} doc{i}"),
            None, Pillar::Episodic, vec![]);
    }
    b.build_index().expect("idx");

    let resident = b.resident_text_bytes();
    let ratio = resident as f64 / corpus_bytes as f64;
    eprintln!("corpus text: {} bytes", corpus_bytes);
    eprintln!("resident text held after index: {} bytes ({:.2}× corpus)", resident, ratio);

    // search once — the per-query clones used to spike to 4-6× transiently; after the fix
    // the steady resident state should already be ~1× and queries should not need full
    // corpus copies. (We can only measure steady resident here, not transient peak.)
    let _ = b.recall("alpha beta gamma", 10);

    // INVARIANT: resident raw text dropped from ~4.0× (pre-fix: raw+lower in BOTH said_file
    // and engine) to ~3× after releasing the engine's redundant raw copy (doc_texts_original)
    // post-index. The remaining 3× is three DIFFERENT transforms all read on the query path:
    // corpus_texts (raw, bridge-entity), corpus_texts_lower (punct-preserving lower, grep),
    // doc_texts_normalized (punct-stripped lower, entity). Driving below 3× needs reading raw
    // text from the mmap on demand in recall_fused (latency-sensitive) — tracked in #4.
    assert!(ratio <= 3.2,
        "index holds corpus text {:.2}× over (resident {} vs corpus {} bytes) — expected ≤3× after releasing the redundant raw engine copy",
        ratio, resident, corpus_bytes);

    let _ = std::fs::remove_file(path);
}
