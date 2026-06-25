//! CLAIMS TEST — weird & edge-case recall robustness. These are the inputs that break naive engines:
//! empty/whitespace/stopword-only queries, emoji, unicode/multiscript, a 1-memory brain, an all-near-
//! identical corpus, pure-number memories, a memory superseded many times, very long queries, and
//! duplicate-content dedup. The bar: NEVER PANIC, and behave sensibly (recall or graceful abstain).
//! See docs/said-structure/CLAIMS-COVERAGE.md.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_recall_edge_cases -- --nocapture

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

/// Degenerate queries must NOT panic and must return a (possibly empty) result gracefully.
#[test]
fn degenerate_queries_never_panic() {
    let path = "test_edge_degenerate.said";
    let mut b = fresh(path);
    for i in 0..20 { b.remember_with_salience(Some(&format!("d{i}")), &format!("Real note {i} about networking."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");

    for q in ["", "   ", "\t\n", "x", "a", "the and or of to", "???", "...", "is it"] {
        let (cands, _kw) = sca_core::ask::ask(&mut b, q, 10, false, None);
        eprintln!("query {q:?} -> {} candidates (no panic)", cands.len());
        // No assertion on count — empty is fine. The contract is: it returns, it doesn't crash.
    }
    cleanup(path);
}

/// Emoji and unicode/multiscript memories must be storable AND recallable in their own script.
#[test]
fn unicode_and_emoji_recall() {
    let path = "test_edge_unicode.said";
    let mut b = fresh(path);
    b.remember_with_salience(Some("zh"), "服务器的备份窗口在每晚两点到四点之间运行。", None, Pillar::Semantic, vec![]);
    b.remember_with_salience(Some("ar"), "تعمل نافذة النسخ الاحتياطي للخادم من الساعة الثانية حتى الرابعة صباحاً.", None, Pillar::Semantic, vec![]);
    b.remember_with_salience(Some("emoji"), "🚀 launch checklist: ✅ smoke tests, ✅ rollback plan, 🔐 secrets rotated", None, Pillar::Episodic, vec![]);
    for i in 0..20 { b.remember_with_salience(Some(&format!("f{i}")), &format!("English filler {i}."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");

    // Recall the Chinese memory with a Chinese query (same-script semantic match).
    let (zh, _) = sca_core::ask::ask(&mut b, "备份窗口什么时候运行", 10, false, None);
    eprintln!("zh recall -> {:?}", zh.iter().take(3).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    // Recall the emoji memory with a related query.
    let (em, _) = sca_core::ask::ask(&mut b, "launch checklist smoke tests rollback", 10, false, None);
    cleanup(path);
    assert!(zh.iter().any(|c| c.doc_id == "zh"), "Chinese memory must be recallable by a Chinese query");
    assert!(em.iter().any(|c| c.doc_id == "emoji"), "emoji-bearing memory must be recallable by its words");
}

/// A brain with exactly ONE memory must recall it (or abstain) — no off-by-one in the pipeline.
#[test]
fn single_memory_brain() {
    let path = "test_edge_single.said";
    let mut b = fresh(path);
    b.remember_with_salience(Some("only"), "The wifi SSID for the lab is Sandbox-5G.", None, Pillar::Semantic, vec![]);
    b.build_index().expect("build_index");
    let (cands, _) = sca_core::ask::ask(&mut b, "what is the lab wifi SSID", 10, false, None);
    cleanup(path);
    eprintln!("single-memory recall -> {:?}", cands.iter().map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(cands.iter().any(|c| c.doc_id == "only"), "the one memory must be recalled");
}

/// Pure-number memories must be storable and exactly recallable by their number.
#[test]
fn pure_number_memories() {
    let path = "test_edge_numbers.said";
    let mut b = fresh(path);
    b.remember_with_salience(Some("a"), "8675309", None, Pillar::Semantic, vec![]);
    b.remember_with_salience(Some("b"), "1234567890", None, Pillar::Semantic, vec![]);
    for i in 0..20 { b.remember_with_salience(Some(&format!("f{i}")), &format!("note {i}"), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");
    let (cands, _) = sca_core::ask::ask(&mut b, "8675309", 10, false, None);
    cleanup(path);
    eprintln!("pure-number recall -> {:?}", cands.iter().take(3).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(cands.iter().any(|c| c.doc_id == "a"), "exact number memory must be recalled by its number");
}

/// A memory superseded many times under the same doc_id: the LATEST version wins on recall, and the
/// history is still retrievable (tombstones kept, not deleted).
#[test]
fn supersede_many_times_latest_wins() {
    let path = "test_edge_supersede.said";
    let mut b = fresh(path);
    for v in 1..=20 {
        b.remember_with_salience(Some("config"), &format!("The cache TTL is set to {} seconds.", v * 10), None, Pillar::Semantic, vec![]);
    }
    for i in 0..20 { b.remember_with_salience(Some(&format!("f{i}")), &format!("filler {i}"), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");
    let (cands, _) = sca_core::ask::ask(&mut b, "what is the cache TTL", 5, false, None);
    let top = cands.iter().find(|c| c.doc_id == "config").cloned();
    cleanup(path);
    let top = top.expect("config must be recalled");
    eprintln!("supersede x20 -> top content: {}", top.content);
    assert!(top.content.contains("200 seconds"), "latest (200s = 20*10) must win, got: {}", top.content);
}

/// Duplicate-content memories must collapse to (at most) a couple of results in ask(), not flood the
/// top-K with the same fact under different ids.
#[test]
fn duplicate_content_dedup_in_results() {
    let path = "test_edge_dedup.said";
    let mut b = fresh(path);
    for i in 0..10 { b.remember_with_salience(Some(&format!("dup{i}")), "The emergency shutdown code is OMEGA-9.", None, Pillar::Semantic, vec![]); }
    for i in 0..20 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Unrelated {i}."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");
    let (cands, _) = sca_core::ask::ask(&mut b, "what is the emergency shutdown code", 10, false, None);
    let dup_count = cands.iter().filter(|c| c.content.contains("OMEGA-9")).count();
    cleanup(path);
    eprintln!("dedup: {dup_count} copies of the duplicated fact in top-10");
    assert!(dup_count >= 1, "the fact must be recalled");
    assert!(dup_count <= 2, "identical-content duplicates must be collapsed (got {dup_count})");
}

/// A very long query (≥20 words) must still recall the relevant memory without panic.
#[test]
fn very_long_query_recall() {
    let path = "test_edge_longq.said";
    let mut b = fresh(path);
    b.remember_with_salience(Some("gold"), "The blue-green deployment strategy routes traffic to the new version only after health checks pass.", None, Pillar::Semantic, vec![]);
    for i in 0..30 { b.remember_with_salience(Some(&format!("f{i}")), &format!("Note {i} about unrelated infrastructure topics and budgets and meetings."), None, Pillar::Episodic, vec![]); }
    b.build_index().expect("build_index");
    let q = "I am trying to understand the deployment approach where we keep two environments and only \
             send real user traffic to the newly released version once all of its health checks have \
             completed successfully so there is no downtime";
    let (cands, _) = sca_core::ask::ask(&mut b, q, 10, false, None);
    cleanup(path);
    eprintln!("long-query recall -> {:?}", cands.iter().take(3).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(cands.iter().any(|c| c.doc_id == "gold"), "long descriptive query must still recall the blue-green note");
}

/// An all-near-identical corpus: every memory differs by one token. A query naming a token must
/// surface that exact one (never dropped) — the twin guarantee, exercised here as an edge case.
#[test]
fn all_near_identical_corpus() {
    let path = "test_edge_identical.said";
    let mut b = fresh(path);
    for n in 0..50 {
        let num = 3000 + n;
        b.remember_with_salience(Some(&format!("w{num}")), &format!("The widget at position {num} stores the calibration data."), None, Pillar::Episodic, vec![]);
    }
    b.build_index().expect("build_index");
    let (cands, _) = sca_core::ask::ask(&mut b, "which widget position stores calibration data, position 3027", 10, false, None);
    cleanup(path);
    let found = cands.iter().any(|c| c.doc_id == "w3027");
    eprintln!("all-identical: w3027 found = {found}, top = {:?}", cands.iter().take(3).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(found, "the exact widget (position 3027) must be recalled among 50 near-identical twins");
}
