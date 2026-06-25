//! CLAIMS TEST — persistence & deployment-mode recall the docs promise: mmap save→reopen gives
//! IDENTICAL recall; enterprise pointer-mode is recallable by summary but stores no content and
//! refuses content ingest; scope-filtered recall returns only the allowed doc_ids.
//! See docs/said-structure/CLAIMS-COVERAGE.md.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_recall_claims_persistence -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::{SaidFile, BrainMode};
use sca_core::frames::Pillar;

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
}

/// CLAIM (02-file-format, public-overview "copy to USB, it works"): after save→reopen, recall returns
/// the IDENTICAL result list (same doc_ids, same order) — no re-embed, no index rebuild needed.
#[test]
fn mmap_save_reopen_recall_identical() {
    let path = "test_claim_mmap.said";
    cleanup(path);
    let query = "which port does the metrics service use";
    let before: Vec<String>;
    {
        let mut b = SaidFile::create(path);
        assert!(b.auto_load_encoder(), "encoder");
        b.remember_with_salience(Some("gold"), "The metrics service listens on port 9090.", None, Pillar::Semantic, vec![]);
        for i in 0..40 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Note {i} about deployment topics and scheduling."), None, Pillar::Episodic, vec![]); }
        b.build_index().expect("build_index");
        before = sca_core::ask::ask(&mut b, query, 10, false, None).0.iter().map(|c| c.doc_id.clone()).collect();
        b.save().expect("save");
    }
    // Reopen from disk (mmap) — must NOT need re-embedding to recall.
    let mut b2 = SaidFile::open(path).expect("open");
    assert!(b2.auto_load_encoder(), "encoder reload (for query encoding only)");
    let after: Vec<String> = sca_core::ask::ask(&mut b2, query, 10, false, None).0.iter().map(|c| c.doc_id.clone()).collect();
    cleanup(path);

    eprintln!("mmap recall before save = {before:?}\n            after reopen = {after:?}");
    assert!(before.iter().any(|d| d == "gold"), "gold recalled before save");
    assert_eq!(before, after, "recall result list must be IDENTICAL after save→reopen (portability claim)");
}

/// CLAIM (row-36-external-pointer): an Enterprise brain stores a POINTER (uri + summary) with no
/// content bytes; recall finds it by the SUMMARY text. And it refuses a content-embedding ingest.
#[test]
fn enterprise_pointer_recall_by_summary_and_refuses_content() {
    let path = "test_claim_enterprise.said";
    cleanup(path);
    let mut b = SaidFile::create_with_mode(path, BrainMode::Enterprise);
    assert!(b.auto_load_encoder(), "encoder");
    assert_eq!(b.mode(), BrainMode::Enterprise);

    // Pointer ingest is allowed: searchable summary, no embedded content.
    b.remember_as_external_pointer(Some("contract"),
        "s3://legal/contracts/orion-2025.pdf", Some("application/pdf"),
        Some("Orion master services agreement"),
        "Master services agreement with Orion Ltd covering managed hosting and a three-year term.",
        vec![]);
    for i in 0..20 { b.remember_as_external_pointer(Some(&format!("p{i}")), &format!("s3://x/{i}"), None, Some(&format!("doc {i}")), &format!("Pointer summary {i} about unrelated filings."), vec![]); }
    b.build_index().expect("build_index");

    // Content-embedding ingest must be REFUSED in enterprise mode.
    let refused = b.ensure_content_ingest_allowed();
    assert!(refused.is_err(), "enterprise mode must refuse content-embedding ingest");

    // Recall by the summary text.
    let (cands, _) = sca_core::ask::ask(&mut b, "which agreement covers managed hosting with Orion", 10, false, None);
    cleanup(path);
    eprintln!("enterprise pointer recall: top = {:?}", cands.iter().take(3).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(cands.iter().any(|c| c.doc_id == "contract"), "pointer must be recallable by its summary");
}

/// CLAIM (3.5 retrieval): scope-filtered recall — passing a scope set of doc_ids restricts results to
/// ONLY those docs. We scope to a subset and assert nothing outside it appears.
#[test]
fn scope_filtered_recall_returns_only_scope() {
    let path = "test_claim_scope.said";
    cleanup(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");
    // Two docs match the same query; we'll scope to only one of them.
    b.remember_with_salience(Some("jan"), "The deployment freeze applies in January for the holiday period.", None, Pillar::Episodic, vec![]);
    b.remember_with_salience(Some("dec"), "The deployment freeze applies in December for the holiday period.", None, Pillar::Episodic, vec![]);
    for i in 0..30 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Filler {i}."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");

    let mut scope = std::collections::HashSet::new();
    scope.insert("jan".to_string());
    let (cands, _) = sca_core::ask::ask(&mut b, "when does the deployment freeze apply", 10, false, Some(&scope));
    cleanup(path);
    eprintln!("scoped recall -> {:?}", cands.iter().map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(!cands.is_empty(), "scoped query should still return the in-scope answer");
    assert!(cands.iter().all(|c| c.doc_id == "jan"), "scope must EXCLUDE out-of-scope docs (got {:?})",
        cands.iter().map(|c| c.doc_id.clone()).collect::<Vec<_>>());
}
