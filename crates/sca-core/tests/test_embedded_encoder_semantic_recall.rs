//! Regression test for issue #1: SCA semantic search returned 0 results because
//! the encoder was not loaded on the read path.
//!
//! The bug: a brain opened without an encoder makes `encode_query` return None,
//! so `query()` early-returns empty and SCA semantic recall is silently dead —
//! even though the model is compiled in via `embed-model`. The CLI's read path
//! (`open_brain` → `try_load_encoder`) only checked external file paths and never
//! loaded the embedded encoder.
//!
//! This test locks down the capability the read path must provide: with the
//! embedded encoder loaded (the same loader the write path uses), a query that
//! shares NO vocabulary with the stored doc must still recall it via the 1-bit
//! fingerprint — something only real semantic search can do (grep cannot).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_embedded_encoder_semantic_recall -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;

#[test]
fn pure_semantic_query_recalls_doc_with_embedded_encoder() {
    let path = "test_embedded_semantic_recall.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);
    // The embedded encoder must load from the compiled-in model — this is exactly
    // what the read path failed to do. If this assert fails, the model isn't
    // embedded (wrong build flags); if the recall below fails, the read path bug
    // has regressed.
    assert!(
        brain.auto_load_encoder(),
        "embedded encoder must load (build with --features embed-model)"
    );

    brain.put("doc1", "The feline curled up beside the warm hearth and slept.", None);
    brain.build_index().expect("build_index");

    // Query shares no content words with the doc — only semantic fingerprints
    // can bridge "cat/napping/fireplace" to "feline/slept/hearth". Grep returns 0.
    let results = brain.query("a cat napping near a cozy fireplace", 5);

    let _ = std::fs::remove_file(path);

    assert!(
        results.iter().any(|r| r.doc_id == "doc1"),
        "pure-semantic query must recall doc1 via SCA fingerprints, got {} results: {:?}",
        results.len(),
        results.iter().map(|r| (&r.doc_id, r.score)).collect::<Vec<_>>()
    );
}
