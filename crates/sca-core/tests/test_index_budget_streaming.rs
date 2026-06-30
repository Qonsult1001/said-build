//! 580MB constant-memory ceiling: index_batch must stream doc-encoding in BOUNDED WINDOWS
//! (SAID_INDEX_BUDGET) instead of holding the whole corpus's embeddings at once. The windowing
//! must be RESULT-INVARIANT: a brain built with a tiny budget (many windows) must return the
//! SAME recall as one built with a huge budget (a single window). If windowing changed the
//! corpus mean/fingerprints, recall would drift — so identical recall proves bit-identity of the
//! quantization that windowing must preserve.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_index_budget_streaming -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

fn corpus() -> Vec<(&'static str, &'static str)> {
    // (doc_id, content) — distinct facts so each has an unambiguous best match.
    vec![
        ("d1", "The loan amortization schedule computes monthly principal and interest."),
        ("d2", "FICA verification checks the customer identity against the credit bureau."),
        ("d3", "The stored procedure dbo.PostTransaction writes a ledger entry per account."),
        ("d4", "Interest accrual runs nightly on the business-day calendar, not UTC."),
        ("d5", "The repayment plan supports early settlement with a rebate calculation."),
        ("d6", "A trigger on the Accounts table audits every balance change."),
        ("d7", "The KYC onboarding flow captures proof of residence and income."),
        ("d8", "Foreign keys link the Loan table to the Customer and Branch tables."),
        ("d9", "The view vw_ArrearsSummary aggregates overdue balances by product."),
        ("d10", "Dynamic SQL via sp_executesql builds the per-branch reporting query."),
    ]
}

fn queries() -> Vec<(&'static str, &'static str)> {
    vec![
        ("how is monthly principal and interest worked out", "d1"),
        ("identity check against credit bureau", "d2"),
        ("which proc writes a ledger entry", "d3"),
        ("nightly interest using business days", "d4"),
        ("early settlement rebate", "d5"),
    ]
}

fn build(path: &str, budget: &str) -> SaidFile {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));
    std::env::set_var("SAID_INDEX_BUDGET", budget);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder must load (embed-model)");
    for (id, text) in corpus() {
        b.remember_with_salience(Some(id), text, None, Pillar::Code, vec![]);
    }
    b.build_index().expect("build_index");
    b
}

fn top1(b: &mut SaidFile, q: &str) -> Option<String> {
    let (cands, _) = sca_core::ask::ask(b, q, 5, false, None);
    cands.first().map(|c| c.doc_id.clone())
}

#[test]
fn windowing_is_result_invariant() {
    // Tiny budget → forces MANY windows (1 doc each). Huge budget → ONE window (old behavior).
    let mut many = build("tmp_idxbudget_many.said", "1");                 // ~1 byte budget → window=1
    let mut one = build("tmp_idxbudget_one.said", &format!("{}", 4usize * 1024 * 1024 * 1024)); // 4GB → single window

    for (q, gold) in queries() {
        let a = top1(&mut many, q);
        let b = top1(&mut one, q);
        assert_eq!(a, b, "windowed vs single-window recall must MATCH for query {:?} (got {:?} vs {:?})", q, a, b);
        assert_eq!(a.as_deref(), Some(gold), "expected {} for {:?}, got {:?}", gold, q, a);
    }

    std::env::remove_var("SAID_INDEX_BUDGET");
    for p in ["tmp_idxbudget_many.said", "tmp_idxbudget_one.said"] {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(format!("{}.spill", p));
    }
}
