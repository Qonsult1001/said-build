//! COMBINED END-TO-END recall test — ONE realistic brain, hard multi-mechanism queries fired through
//! the real `ask` path. The point is NOT isolated unit checks (those live in the themed claim tests);
//! it's to prove the WHOLE system holds together on complex scenarios: a single query that must use
//! semantic + lexical-discriminator + concept-graph bridge + recency/update + dedup + abstention at
//! once, against a mixed corpus of notes, legal docs, code, and near-duplicates — all coexisting.
//!
//! Every check prints what came back, so you can SEE it work (or see exactly where it breaks).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code"
//!        --test test_recall_e2e_combined -- --nocapture

#![cfg(all(feature = "embed-model", feature = "code"))]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

/// Build ONE brain that mixes every memory kind, so queries compete against realistic noise.
fn build_world(path: &str) -> SaidFile {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");

    // --- plain factual notes (semantic + paraphrase competitors) ---
    let notes = [
        ("fact_reef", "The Great Barrier Reef lies off the coast of Queensland, Australia."),
        ("fact_peni", "Penicillin was discovered by Alexander Fleming in 1928."),
        ("fact_light", "The speed of light is about 300,000 kilometres per second."),
        ("fact_berlin", "The Berlin Wall fell in November 1989."),
    ];
    for (id, t) in notes { b.remember_with_salience(Some(id), t, None, Pillar::Semantic, vec![]); }

    // --- a CONCEPT-BRIDGE chain (multi-hop via wikilink) ---
    b.remember_with_salience(Some("hop_a"), "Dr. Sarah Lee is the lead cardiologist on the [[cardiology]] team.", None, Pillar::Semantic, vec![]);
    b.remember_with_salience(Some("hop_b"), "The [[cardiology]] team is handling the Vance coronary bypass next Tuesday.", None, Pillar::Semantic, vec![]);

    // --- an UPDATE pair (latest wins) ---
    b.remember_with_salience(Some("db"), "The default application database is PostgreSQL.", None, Pillar::Semantic, vec![]);
    b.remember_with_salience(Some("db"), "Correction: the default application database is now SQLite.", None, Pillar::Semantic, vec![]);

    // --- near-IDENTICAL legal twins (discriminator must win) ---
    for n in 0..20 {
        let code = format!("REF-{:05}", 9000 + n);
        b.remember_with_salience(Some(&format!("legal_{code}")),
            &format!("Invoice reference {code} covers the March managed-services charge."),
            None, Pillar::External, vec![]);
    }

    // --- legal doc with an in-content footer date (temporal-by-content) ---
    b.remember_with_salience(Some("matter"),
        "Matter Vance v. Northwind — Notice of Motion.\nRef: CV-2024-0188 — filed 2024-08-03 — section 4(b).",
        None, Pillar::External, vec![]);

    // --- duplicate-content spam (dedup must collapse) ---
    for i in 0..8 { b.remember_with_salience(Some(&format!("dup{i}")), "The emergency shutdown code is OMEGA-9.", None, Pillar::Semantic, vec![]); }

    // --- realistic filler noise ---
    for i in 0..60 { b.remember_with_salience(Some(&format!("noise{i}")), &format!("Standup note {i}: discussed the recycling depot, rainfall, and the library hours."), None, Pillar::Episodic, vec![]); }

    b.build_concept_links();
    b.build_index().expect("build_index");
    b
}

fn ids(cands: &[sca_core::ask::AskCandidate]) -> Vec<String> {
    cands.iter().take(5).map(|c| c.doc_id.clone()).collect()
}

#[test]
fn combined_complex_recall_holds_together() {
    let path = "test_e2e_combined.said";
    let mut b = build_world(path);
    let mut failures: Vec<String> = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| {
        eprintln!("[{}] {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok { failures.push(name.to_string()); }
    };

    // 1) PARAPHRASE among noise: low-overlap query → the right fact.
    let (c, _) = sca_core::ask::ask(&mut b, "who found the first antibiotic", 10, false, None);
    check("paraphrase", c.iter().take(10).any(|x| x.doc_id == "fact_peni"), format!("top={:?}", ids(&c)));

    // 2) MULTI-HOP bridge: query matches hop_a, answer lives in hop_b (shared [[cardiology]]).
    let (c, _) = sca_core::ask::ask(&mut b, "what procedure is Dr. Sarah Lee's team handling next Tuesday", 10, false, None);
    check("multi-hop bridge", c.iter().take(10).any(|x| x.doc_id == "hop_b"), format!("top={:?}", ids(&c)));

    // 3) UPDATE latest-wins: must return SQLite, never PostgreSQL.
    let (c, _) = sca_core::ask::ask(&mut b, "what is the default application database", 5, false, None);
    let dbhit = c.iter().find(|x| x.doc_id == "db");
    let ok = dbhit.map(|h| h.content.contains("SQLite") && !h.content.contains("PostgreSQL")).unwrap_or(false);
    check("update latest-wins", ok, format!("content={:?}", dbhit.map(|h| h.content.clone())));

    // 4) DISCRIMINATOR among 20 near-identical legal twins: exact REF must be found.
    let (c, _) = sca_core::ask::ask(&mut b, "which invoice reference REF-09013 covers the March managed-services charge", 10, false, None);
    check("legal discriminator", c.iter().take(10).any(|x| x.doc_id == "legal_REF-09013"), format!("top={:?}", ids(&c)));

    // 5) TEMPORAL by in-content footer date.
    let (c, _) = sca_core::ask::ask(&mut b, "which matter was filed on 2024-08-03 under reference CV-2024-0188", 10, false, None);
    check("footer-date temporal", c.iter().take(10).any(|x| x.doc_id == "matter"), format!("top={:?}", ids(&c)));

    // 6) DEDUP: the duplicated OMEGA-9 fact must not flood the top-K.
    let (c, _) = sca_core::ask::ask(&mut b, "what is the emergency shutdown code", 10, false, None);
    let dups = c.iter().filter(|x| x.content.contains("OMEGA-9")).count();
    check("dedup", (1..=2).contains(&dups), format!("{dups} copies of OMEGA-9 in top-10"));

    // 7) ABSTENTION (existence) in a NOISY mixed corpus: a no-answer query must return NOTHING. The
    // threshold-free gate combines score-SHAPE (gap/commitment) with LEXICAL GROUNDING (does the top
    // hit share a query term?) — so the embedding-proximity artifact ("wifi password lodge" → a
    // Penicillin note that shares NO query words) is vetoed with NO magnitude threshold.
    std::env::set_var("SAID_ASK_ABSTAIN_SHAPE", "1");
    let (c, _) = sca_core::ask::ask(&mut b, "what is the wifi password at the mountain lodge", 10, false, None);
    std::env::remove_var("SAID_ASK_ABSTAIN_SHAPE");
    check("abstain on no-answer (noisy corpus, grounding veto)", c.is_empty(),
        format!("returned {} (expected 0): {:?}", c.len(), ids(&c)));

    // 8) CROSS-CONTAMINATION guard: a legal-twin query must NOT return the cardiology bridge or notes.
    let (c, _) = sca_core::ask::ask(&mut b, "which invoice reference REF-09005 covers the March managed-services charge", 5, false, None);
    let top1_is_legal = c.first().map(|x| x.doc_id.starts_with("legal_")).unwrap_or(false);
    check("no cross-contamination", top1_is_legal, format!("top={:?}", ids(&c)));

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    eprintln!("\n=== COMBINED E2E: {}/{} scenarios passed ===", 8 - failures.len(), 8);
    assert!(failures.is_empty(), "combined complex recall FAILED on: {failures:?}");
}
