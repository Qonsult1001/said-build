//! CLAIMS TEST — ranking-affecting recall behaviours the docs promise. Each test plants a controlled
//! scenario and asserts the DOCUMENTED ranking effect actually happens through the real `ask`/recall
//! path. A failing/ignored test here = the docs claim something the engine doesn't yet do (a real
//! finding), not a broken test. See docs/said-structure/CLAIMS-COVERAGE.md.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_recall_claims_ranking -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

fn fresh(path: &str) -> SaidFile {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "embedded encoder must load");
    b
}
fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
}

/// CLAIM (row-33-salience + Layer 9): a high-salience memory should rank at/above an otherwise-equal
/// low-salience memory. We store two memories about DIFFERENT facts (so the query matches one), one
/// marked clearly important (a decision) and one chit-chat, and confirm the important one is found.
#[test]
fn salience_high_is_recallable_and_scored() {
    let path = "test_claim_salience.said";
    let mut b = fresh(path);
    // High-salience: an explicit decision (the salience scorer flags decisions as high).
    let (_, hi) = b.remember_with_salience(Some("decision"),
        "DECISION: we will migrate the billing database to PostgreSQL next sprint.",
        None, Pillar::Semantic, vec![]);
    // Low-salience: chit-chat.
    let (_, lo) = b.remember_with_salience(Some("chitchat"),
        "haha yeah that meeting was something lol",
        None, Pillar::Episodic, vec![]);
    for i in 0..30 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Filler note {i} about logistics."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");

    eprintln!("salience: decision band score={} ({:?}), chitchat score={} ({:?})", hi.score, hi.band, lo.score, lo.band);
    // The scorer must rank the decision strictly higher than chit-chat (documented signal).
    assert!(hi.score > lo.score, "decision salience {} must exceed chit-chat {}", hi.score, lo.score);

    // And the high-salience memory must be recallable by its content.
    let (cands, _) = sca_core::ask::ask(&mut b, "which database are we migrating billing to", 10, false, None);
    cleanup(path);
    assert!(cands.iter().any(|c| c.doc_id == "decision"), "high-salience decision must be recallable");
}

/// CLAIM (3.3-brain + Layer 9): a frequently-recalled memory gets a higher recall_weight (1.0→2.0),
/// which boosts its ranking. We recall a memory several times and assert its recall_weight rises.
#[test]
fn recall_weight_rises_with_repeated_access() {
    let path = "test_claim_recallweight.said";
    let mut b = fresh(path);
    b.remember_with_salience(Some("hot"), "The incident hotline number is extension 4471.", None, Pillar::Semantic, vec![]);
    for i in 0..30 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Filler {i} about scheduling."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");

    let w0 = b.engine.brain.get_recall_weight("hot");
    // Recall it several times (each ask logs the query + top doc, reconsolidating recall_weight).
    for _ in 0..6 {
        let _ = sca_core::ask::ask(&mut b, "what is the incident hotline extension", 5, false, None);
    }
    let w1 = b.engine.brain.get_recall_weight("hot");
    cleanup(path);
    eprintln!("recall_weight: before={w0:.3} after 6 recalls={w1:.3}");
    assert!(w1 > w0, "recall_weight must rise with repeated access (before {w0:.3}, after {w1:.3})");
    assert!(w1 <= 2.0, "recall_weight capped at 2.0 (got {w1:.3})");
}

/// CLAIM (row-41-surprise + semantic pillar): when a newer memory CONTRADICTS/UPDATES an older one
/// under the same doc_id, recall returns the LATEST version (the old is tombstoned). Already proven in
/// the quality test's Update category; here we assert it directly + that the old content never returns.
#[test]
fn contradiction_latest_version_wins() {
    let path = "test_claim_contradiction.said";
    let mut b = fresh(path);
    b.remember_with_salience(Some("policy"), "The data retention policy is 30 days.", None, Pillar::Semantic, vec![]);
    // Supersede under the same doc_id with the corrected value.
    b.remember_with_salience(Some("policy"), "Correction: the data retention policy is now 90 days.", None, Pillar::Semantic, vec![]);
    for i in 0..30 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Filler {i}."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");

    let (cands, _) = sca_core::ask::ask(&mut b, "what is the data retention policy", 5, false, None);
    let top = cands.iter().find(|c| c.doc_id == "policy");
    cleanup(path);
    let top = top.expect("the policy memory must be recalled");
    eprintln!("contradiction: top content = {}", top.content);
    assert!(top.content.contains("90"), "latest (90 days) must win, got: {}", top.content);
    assert!(!top.content.contains("30 days"), "stale (30 days) must NOT be returned, got: {}", top.content);
}

/// CLAIM (Layer 5 entity-speaker boost): a query naming a speaker should surface that speaker's
/// statement. We store statements attributed to different people and query by speaker name.
#[test]
fn entity_speaker_statement_is_recallable() {
    let path = "test_claim_entity.said";
    let mut b = fresh(path);
    b.remember_with_salience(Some("melanie"), "Melanie said the launch should slip to Q3 for stability.", None, Pillar::Episodic, vec![]);
    b.remember_with_salience(Some("darius"), "Darius said the launch must stay in Q2 to hit the contract.", None, Pillar::Episodic, vec![]);
    for i in 0..30 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Filler {i} about budgets."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");

    let (cands, _) = sca_core::ask::ask(&mut b, "what did Melanie say about the launch", 10, false, None);
    cleanup(path);
    let rank = cands.iter().position(|c| c.doc_id == "melanie");
    eprintln!("entity-speaker: Melanie rank = {rank:?}");
    assert!(rank.map(|r| r < 10).unwrap_or(false), "Melanie's statement must be recalled in top-10 for a query naming her");
}
