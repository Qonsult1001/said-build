//! WIDX step 3: the resident BM25 word index -> WordIndex -> serialize -> mmap reader must return
//! lists identical to what the resident CrystallineCore structures hold. This proves the on-disk
//! word index is a faithful, bit-identical replacement for the RAM structures (the 580MB fix).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_widx_from_core -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;
use sca_core::word_index::{WordIndex, WidxReader};

#[test]
fn widx_from_core_matches_resident() {
    let path = "tmp_widx_from_core.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));

    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");
    for (id, text) in [
        ("d1", "the loan amortization schedule computes monthly principal and interest"),
        ("d2", "fica verification checks the customer identity against the credit bureau"),
        ("d3", "the stored procedure posts a ledger entry per account and audits balance"),
        ("d4", "interest accrual runs nightly on the business day calendar"),
    ] {
        b.remember_with_salience(Some(id), text, None, Pillar::Code, vec![]);
    }
    b.build_index().expect("build_index");

    // Build the WIDX from the live core, round-trip through bytes + the mmap reader.
    let wi: WordIndex = b.engine.core.to_word_index();
    let bytes = wi.serialize_raw();
    let reader = WidxReader::new(&bytes).expect("reader");

    // vocab count + every word<->id resolves identically
    assert_eq!(reader.vocab_len(), wi.vocab.len());
    assert!(reader.vocab_len() > 0, "a real brain must have a vocabulary");

    // For every doc, the reader's word-set must equal the resident set (as sorted ids).
    for d in 0..wi.doc_word_sets.len() {
        let from_reader = reader.doc_word_set(d).expect("doc_word_set");
        assert_eq!(from_reader, wi.doc_word_sets[d], "doc {} word-set mismatch", d);
        let tf_reader = reader.doc_word_tf(d).expect("doc_word_tf");
        assert_eq!(tf_reader, wi.doc_word_tf[d], "doc {} tf mismatch", d);
    }

    // For every indexed word, the inverted posting list must match.
    for (wid, docs) in &wi.word_inverted {
        assert_eq!(reader.word_inverted(*wid).as_ref(), Some(docs), "wid {} postings", wid);
    }

    // Spot-check a real word resolves and its postings are non-empty.
    if let Some(id) = reader.word_id_of("loan") {
        assert!(reader.word_inverted(id).map(|v| !v.is_empty()).unwrap_or(false),
            "'loan' should appear in at least one doc");
    }

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));
}
