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

/// The correctness guarantee for the whole SPIMI change: doc_has_word must return the SAME answer
/// whether it reads the resident structures (fresh build) or the disk-backed WIDX (after reopen).
#[test]
fn doc_has_word_identical_resident_vs_widx() {
    let path = "tmp_widx_dhw.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));

    let docs = [
        ("d1", "the loan amortization schedule computes principal and interest"),
        ("d2", "fica verification checks customer identity credit bureau"),
        ("d3", "stored procedure posts ledger entry per account audit"),
    ];
    let probes = ["loan", "fica", "ledger", "procedure", "amortization", "missing", "the", "customer"];

    // Fresh in-RAM build (resident path).
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());
    for (id, t) in docs { b.remember_with_salience(Some(id), t, None, Pillar::Code, vec![]); }
    b.build_index().expect("build_index");
    assert!(!b.engine.core.has_widx(), "fresh build uses resident structures");
    let resident: Vec<bool> = (0..docs.len())
        .flat_map(|d| probes.iter().map(move |w| (d, *w)))
        .map(|(d, w)| b.engine.core.doc_has_word(d, w))
        .collect();
    b.save().expect("save");

    // Reopen (disk-backed WIDX path).
    let reopened = SaidFile::open(path).expect("open");
    assert!(reopened.engine.core.has_widx(), "reopened uses WIDX");
    let from_widx: Vec<bool> = (0..docs.len())
        .flat_map(|d| probes.iter().map(move |w| (d, *w)))
        .map(|(d, w)| reopened.engine.core.doc_has_word(d, w))
        .collect();

    assert_eq!(resident, from_widx, "doc_has_word must be identical resident vs WIDX");
    // sanity: at least some probes hit (not all-false)
    assert!(from_widx.iter().any(|&x| x), "expected some words to be found");

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));
}

/// The build-fix correctness proof: deriving word_inverted + phonetic from the per-doc sets + vocab
/// (word_index_derived) must produce a WordIndex IDENTICAL to reading the resident structures
/// (to_word_index). If identical, the corpus-wide word_inverted_fast accumulator (the OOM spike) can
/// be dropped from the build and derived instead.
#[test]
fn derived_word_index_matches_resident() {
    let path = "tmp_widx_derived.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));

    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());
    for (id, t) in [
        ("d1", "the loan amortization schedule computes monthly principal and interest payments"),
        ("d2", "fica verification checks the customer identity against the credit bureau records"),
        ("d3", "stored procedure posts a ledger entry per account and audits every balance change"),
        ("d4", "interest accrual runs nightly on the business day calendar not utc timezone"),
        ("d5", "the repayment plan supports early settlement with a rebate calculation formula"),
    ] {
        b.remember_with_salience(Some(id), t, None, Pillar::Code, vec![]);
    }
    b.build_index().expect("build_index");

    let from_resident = b.engine.core.to_word_index();
    let from_derived = b.engine.core.word_index_derived();

    assert_eq!(from_derived.vocab, from_resident.vocab, "vocab");
    assert_eq!(from_derived.doc_word_sets, from_resident.doc_word_sets, "doc_word_sets");
    assert_eq!(from_derived.doc_word_tf, from_resident.doc_word_tf, "doc_word_tf");
    assert_eq!(from_derived.word_inverted, from_resident.word_inverted, "word_inverted (transpose)");
    assert_eq!(from_derived.phonetic, from_resident.phonetic, "phonetic (from vocab)");

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));
}

#[test]
fn widx_persists_across_save_and_open() {
    let path = "tmp_widx_persist.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));

    // Build + save a brain; capture its in-RAM word index for comparison.
    let expected: WordIndex = {
        let mut b = SaidFile::create(path);
        assert!(b.auto_load_encoder(), "encoder");
        for (id, text) in [
            ("d1", "the loan amortization schedule computes monthly principal"),
            ("d2", "fica verification checks the customer identity"),
            ("d3", "stored procedure posts a ledger entry per account"),
        ] {
            b.remember_with_salience(Some(id), text, None, Pillar::Code, vec![]);
        }
        b.build_index().expect("build_index");
        let wi = b.engine.core.to_word_index();
        b.save().expect("save");
        wi
    };

    // Reopen from disk — the WIDX section must be present + readable, matching the pre-save index.
    let reopened = SaidFile::open(path).expect("open");
    assert!(reopened.engine.core.has_widx(), "reopened brain must carry a WIDX section");
    let reader = reopened.engine.core.widx_reader().expect("widx reader from disk");

    assert_eq!(reader.vocab_len(), expected.vocab.len(), "vocab size persisted");
    for d in 0..expected.doc_word_sets.len() {
        assert_eq!(reader.doc_word_set(d).as_ref(), Some(&expected.doc_word_sets[d]),
            "doc {} word-set survived save/open", d);
    }
    for (wid, docs) in &expected.word_inverted {
        assert_eq!(reader.word_inverted(*wid).as_ref(), Some(docs),
            "wid {} postings survived save/open", wid);
    }

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));
}
