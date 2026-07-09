#![cfg(all(feature = "embed-model", feature = "code"))]
//! Blueprint memory (public name) = canon (internal): the reusable 80% structure, keyed by SHAPE.
//! The ONE engine rule that differs from learn-fix: KEEP-FIRST dedup. Re-learning the SAME shape is a
//! no-op (the existing blueprint stands) -- NOT learn-fix's last-write supersede. promote_blueprint is
//! the deliberate exception (explicit "make this the new standard" -> supersede).
//!   cargo test -p sca-core --no-default-features --features "embed-model,code" --test test_blueprint -- --nocapture
use sca_core::said_file::SaidFile;

fn fresh(tag: &str) -> (SaidFile, std::path::PathBuf) {
    let p = std::env::temp_dir().join(format!("said_bp_{}_{}.said", tag, std::process::id()));
    let _ = std::fs::remove_file(&p);
    let mut b = SaidFile::create(p.to_string_lossy().as_ref());
    assert!(b.auto_load_encoder());
    (b, p)
}
fn cleanup(p: &std::path::PathBuf) {
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
}

// SAID_PROJECT/SAID_RECALL_PROJECT are process-global env vars; cargo runs tests in parallel threads
// sharing one process, so a project-scope test can race a non-scoped one. Serialize via a mutex and
// clear the scoping env at the start of every test (mirrors how the fix tests stay in separate files).
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn clear_scope() {
    std::env::remove_var("SAID_PROJECT");
    std::env::remove_var("SAID_RECALL_PROJECT");
    std::env::remove_var("SAID_RECALL_LANG");
}

#[test]
fn learn_then_recall_roundtrip_and_keep_first_dedup() {
    let _g = ENV_LOCK.lock().unwrap();
    clear_scope();
    let (mut b, p) = fresh("kf");

    // 1) learn a blueprint keyed by SHAPE
    let shape = "Create<Entity> REST endpoint";
    let v1 = r#"{"sections":["accept-and-audit","idempotency","guards","save","response","wrap+return"]}"#;
    let id1 = sca_core::ask::learn_blueprint(&mut b, shape, v1, None, None, false);
    assert!(id1.starts_with("shape::"), "doc_id should be shape::<hash>, got {}", id1);

    // round-trip: recall by a paraphrase of the shape returns it
    let got = sca_core::ask::recall_blueprints(&mut b, "create endpoint for an entity", 5, 0.0);
    assert!(!got.is_empty(), "blueprint should be recallable by shape paraphrase");
    assert_eq!(got[0].doc_id, id1, "recall should return the learned blueprint");
    assert!(got[0].sections_json.contains("idempotency"), "sections payload should round-trip");

    // 2) KEEP-FIRST: re-learning the SAME shape with DIFFERENT sections is a no-op -> original stands.
    let v2 = r#"{"sections":["DIFFERENT","totally","replaced"]}"#;
    let id2 = sca_core::ask::learn_blueprint(&mut b, shape, v2, None, None, false);
    assert_eq!(id2, id1, "same shape must resolve to the same doc_id");
    let body = b.get(&id1).unwrap_or_default();
    assert!(body.contains("idempotency"), "keep-first: ORIGINAL sections must survive");
    assert!(!body.contains("DIFFERENT"), "keep-first: the 2nd learn must NOT overwrite the original");

    cleanup(&p);
}

#[test]
fn verified_edit_auto_updates_the_blueprint() {
    let _g = ENV_LOCK.lock().unwrap();
    clear_scope();
    let (mut b, p) = fresh("verified");
    let shape = "Create<Entity> REST endpoint";
    // first blueprint (unverified learn)
    sca_core::ask::learn_blueprint(&mut b, shape, r#"{"sections":["old"]}"#, None, None, false);
    // structure edited + BUILD PASSED -> verified=true -> auto-update, no prompt.
    let id = sca_core::ask::learn_blueprint(&mut b, shape, r#"{"sections":["better"]}"#, None, None, true);
    let body = b.get(&id).unwrap_or_default();
    assert!(body.contains("better"), "verified edit must auto-update the blueprint");
    assert!(!body.contains("old"), "verified edit must supersede the old structure");
    cleanup(&p);
}

#[test]
fn promote_supersedes_the_blueprint() {
    let _g = ENV_LOCK.lock().unwrap();
    clear_scope();
    let (mut b, p) = fresh("promote");
    let shape = "Update<Entity> REST endpoint";
    sca_core::ask::learn_blueprint(&mut b, shape, r#"{"sections":["old-way"]}"#, None, None, false);

    // promote = the explicit "make this the new standard" (the exception to keep-first).
    let id = sca_core::ask::promote_blueprint(&mut b, shape, r#"{"sections":["my-preferred-way"]}"#, None, None);
    let body = b.get(&id).unwrap_or_default();
    assert!(body.contains("my-preferred-way"), "promote must install the new sections");
    assert!(!body.contains("old-way"), "promote must supersede the old sections");

    cleanup(&p);
}

#[test]
fn recall_is_project_scoped_like_fixes() {
    let _g = ENV_LOCK.lock().unwrap();
    clear_scope();
    let (mut b, p) = fresh("proj");
    let shape = "Create<Entity> REST endpoint";

    std::env::set_var("SAID_PROJECT", "said-build");
    sca_core::ask::learn_blueprint(&mut b, shape, r#"{"sections":["said-build flavour"]}"#, None, None, false);
    std::env::set_var("SAID_PROJECT", "said-echo");
    sca_core::ask::learn_blueprint(&mut b, shape, r#"{"sections":["said-echo flavour"]}"#, None, None, false);
    std::env::remove_var("SAID_PROJECT");

    let q = "create endpoint for an entity";

    // default: both reachable
    std::env::remove_var("SAID_RECALL_PROJECT");
    let open = sca_core::ask::recall_blueprints(&mut b, q, 5, 0.0);
    assert!(open.len() >= 2, "unscoped recall should see both projects' blueprints, got {}", open.len());

    // scoped: only said-build
    std::env::set_var("SAID_RECALL_PROJECT", "said-build");
    let scoped = sca_core::ask::recall_blueprints(&mut b, q, 5, 0.0);
    std::env::remove_var("SAID_RECALL_PROJECT");
    assert!(!scoped.is_empty(), "scoped recall should still find said-build's blueprint");
    for bp in &scoped {
        let body = b.get(&bp.doc_id).unwrap_or_default();
        assert!(!body.contains("said-echo flavour"), "scoped recall leaked said-echo blueprint");
    }

    cleanup(&p);
}
