//! Decision 4 acceptance tests — heuristic salience scorer (v0).
//!
//! Verifies deterministic behavior of the scorer on common turn shapes plus
//! the SalienceAccumulator threshold trigger. Also exercises the integration
//! path (`SaidFile::remember_with_salience`) to prove the tag set lands on
//! the frame after save→reopen.

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;
use sca_core::salience::{score_turn, Salience, SalienceAccumulator, SalienceBand};

fn tmp_path(label: &str) -> String {
    // System temp dir so test brains don't pile up in the repo (#4 cleanup).
    let pid = std::process::id();
    std::env::temp_dir().join(format!("said_decision4_{}_{}.said", label, pid)).to_string_lossy().into_owned()
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.tmp", path));
}

// ════════════════════════════════════════════════════════════════════════════
// Scorer bands on canonical turn shapes
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn chit_chat_scores_low() {
    let s = score_turn("lol", Pillar::Episodic);
    assert_eq!(s.band, SalienceBand::Low, "lol score={}", s.score);

    let s = score_turn("ok thanks", Pillar::Episodic);
    assert_eq!(s.band, SalienceBand::Low, "ok thanks score={}", s.score);

    let s = score_turn("got it", Pillar::Episodic);
    assert_eq!(s.band, SalienceBand::Low, "got it score={}", s.score);
}

#[test]
fn bare_question_scores_low() {
    let s = score_turn("what time is it?", Pillar::Episodic);
    assert_eq!(s.band, SalienceBand::Low, "bare question score={}", s.score);
}

#[test]
fn explicit_remember_scores_high() {
    let s = score_turn(
        "/remember the deploy key is stored in vault under prod/deploy.",
        Pillar::Semantic,
    );
    assert_eq!(s.band, SalienceBand::High, "explicit remember score={}", s.score);
    assert!(
        s.tags.iter().any(|t| t == "explicit"),
        "explicit tag must be set, got {:?}",
        s.tags
    );
}

#[test]
fn correction_emits_reconsolidation_tag() {
    let s = score_turn(
        "Actually, the endpoint is https://api.v2/foo, not the v1 one I said earlier.",
        Pillar::Semantic,
    );
    assert!(
        s.score >= 30,
        "correction should be medium+ (score={})",
        s.score
    );
    assert!(
        s.tags.iter().any(|t| t == "reconsolidation"),
        "correction must emit reconsolidation tag, got {:?}",
        s.tags
    );
}

#[test]
fn decision_emits_decision_tag() {
    let s = score_turn(
        "We decided to go with Postgres over MySQL for the billing service.",
        Pillar::Semantic,
    );
    assert!(
        s.score >= 30,
        "decision should be medium+ (score={})",
        s.score
    );
    assert!(
        s.tags.iter().any(|t| t == "decision"),
        "decision must emit decision tag, got {:?}",
        s.tags
    );
}

#[test]
fn procedural_pillar_gets_bias() {
    // Same content, different pillar — Procedural should beat Episodic by
    // exactly the pillar_bias delta (10 vs 0) and may push the band up.
    let content = "To deploy the card service run cargo build then scp and restart systemd.";
    let ep = score_turn(content, Pillar::Episodic);
    let proc = score_turn(content, Pillar::Procedural);
    assert!(
        proc.score > ep.score,
        "Procedural bias should beat Episodic: proc={} ep={}",
        proc.score,
        ep.score
    );
}

#[test]
fn band_tag_always_present() {
    // Whatever the band, `salience:<band>` must be the first or in tags.
    for text in &[
        "lol",
        "The invoice total is $4217.",
        "/remember the API key is abc123",
    ] {
        let s = score_turn(text, Pillar::Episodic);
        assert!(
            s.tags.iter().any(|t| t.starts_with("salience:")),
            "band tag must be present on any turn, got {:?}",
            s.tags
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Accumulator
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn accumulator_fires_at_threshold() {
    let mut acc = SalienceAccumulator::new_with_threshold(100);
    assert!(!acc.add(40));
    assert!(!acc.add(40));
    let fired = acc.add(30); // 40+40+30 = 110 >= 100
    assert!(fired, "accumulator should have fired at threshold");
    // Reset after fire.
    assert_eq!(acc.sum(), 0);
    assert_eq!(acc.count(), 0);
}

#[test]
fn accumulator_never_fires_below_threshold() {
    let mut acc = SalienceAccumulator::new_with_threshold(100);
    for _ in 0..5 {
        assert!(!acc.add(10), "sum={}", acc.sum());
    }
    assert_eq!(acc.sum(), 50);
}

// ════════════════════════════════════════════════════════════════════════════
// Integration: tags land on the frame after save→reopen
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn remember_with_salience_writes_tags_to_frame() {
    let path = tmp_path("integration");
    cleanup(&path);

    let mut sf = SaidFile::create(&path);
    let (frame_id, scored) = sf.remember_with_salience(
        None,
        "/remember the staging API key is key_abc123_staging",
        None,
        Pillar::Semantic,
        vec!["project:demo".to_string()],
    );

    // In-memory: band should already be High.
    assert_eq!(scored.band, SalienceBand::High, "score={}", scored.score);

    sf.save().expect("save");
    let sf = SaidFile::open(&path).expect("reopen");

    let frame = sf
        .frames
        .get_all_frames()
        .into_iter()
        .find(|m| m.id == frame_id)
        .expect("frame must exist after reopen");

    // Expected tags on the frame:
    //   - caller: "project:demo"
    //   - pillar writer: "pillar:semantic"
    //   - salience: "salience:high" + "explicit"
    let has = |t: &str| frame.tags.iter().any(|x| x == t);
    assert!(has("project:demo"), "caller tag preserved, got {:?}", frame.tags);
    assert!(has("pillar:semantic"), "pillar tag preserved, got {:?}", frame.tags);
    assert!(has("salience:high"), "salience band tag, got {:?}", frame.tags);
    assert!(has("explicit"), "explicit marker tag, got {:?}", frame.tags);

    cleanup(&path);
}

#[test]
fn remember_with_salience_does_not_duplicate_tags() {
    // If the caller passes a tag that salience would have added anyway,
    // it should appear once, not twice.
    let path = tmp_path("dedup");
    cleanup(&path);

    let mut sf = SaidFile::create(&path);
    let (frame_id, _) = sf.remember_with_salience(
        None,
        "lol",
        None,
        Pillar::Episodic,
        vec!["salience:low".to_string()], // already in extras
    );
    sf.save().expect("save");
    let sf = SaidFile::open(&path).expect("reopen");

    let frame = sf.frames.get_all_frames().into_iter().find(|m| m.id == frame_id).unwrap();
    let count = frame.tags.iter().filter(|t| *t == "salience:low").count();
    assert_eq!(count, 1, "salience:low must appear exactly once, tags={:?}", frame.tags);

    cleanup(&path);
}

// ════════════════════════════════════════════════════════════════════════════
// Regression fence — plain remember_with_pillar should not gain salience tags
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn plain_remember_with_pillar_has_no_salience_tag() {
    let path = tmp_path("regression");
    cleanup(&path);

    let mut sf = SaidFile::create(&path);
    let frame_id = sf.remember_with_pillar(
        None,
        "/remember important thing",
        None,
        Pillar::Episodic,
        vec![],
    );
    sf.save().expect("save");
    let sf = SaidFile::open(&path).expect("reopen");
    let frame = sf.frames.get_all_frames().into_iter().find(|m| m.id == frame_id).unwrap();
    assert!(
        !frame.tags.iter().any(|t| t.starts_with("salience:")),
        "plain remember_with_pillar must NOT add salience tags — caller opt-in only. tags={:?}",
        frame.tags
    );

    cleanup(&path);
}

// Silence unused-import lint for `Salience` — kept in the import list so the
// API surface is visible in the test file.
#[allow(dead_code)]
fn _api_surface_check(_s: Salience) {}
