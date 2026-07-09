#![cfg(all(feature = "embed-model", feature = "code"))]
//! Harvest-at-init: scan a repo, auto-learn blueprints from REPEATED structures only (clone-mining
//! research: support>=2, size floor, similarity gate). A structure repeated >=2x becomes a blueprint;
//! one-offs are skipped. Keep-first, so re-running harvest never clobbers.
//!   cargo test -p sca-core --no-default-features --features "embed-model,code" --test test_harvest -- --nocapture
use sca_core::said_file::SaidFile;
use std::path::PathBuf;

fn brain(tag: &str) -> (SaidFile, PathBuf) {
    let p = std::env::temp_dir().join(format!("said_harvest_{}_{}.said", tag, std::process::id()));
    let _ = std::fs::remove_file(&p);
    let mut b = SaidFile::create(p.to_string_lossy().as_ref());
    assert!(b.auto_load_encoder());
    (b, p)
}
fn cleanup(p: &PathBuf) {
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
}

// A fake repo: two Create endpoints sharing the SAME skeleton (a real pattern), plus a one-off helper.
fn fake_repo() -> std::collections::HashMap<PathBuf, String> {
    let mut m = std::collections::HashMap::new();
    m.insert(PathBuf::from("invoice.rs"), r#"
fn create_invoice(req: Req) -> Resp {
    audit_request(&req);
    check_idempotency(&req);
    validate_fields(&req);
    insert_row(&req);
    wrap_response(&req)
}
"#.to_string());
    m.insert(PathBuf::from("order.rs"), r#"
fn create_order(req: Req) -> Resp {
    audit_request(&req);
    check_idempotency(&req);
    validate_fields(&req);
    insert_row(&req);
    wrap_response(&req)
}
"#.to_string());
    // a one-off: only appears once, must NOT become a blueprint
    m.insert(PathBuf::from("util.rs"), r#"
fn format_currency(x: i64) -> String {
    round_money(x);
    to_display_string(x)
}
"#.to_string());
    m
}

#[test]
fn harvests_a_blueprint_from_a_repeated_structure_only() {
    let (mut b, p) = brain("repeat");
    let repo = fake_repo();
    let files: Vec<PathBuf> = repo.keys().cloned().collect();
    let read = |path: &std::path::Path| repo.get(path).cloned();

    let report = sca_core::harvest::harvest_blueprints(&mut b, files, read);
    println!("harvest: files={} fns={} clusters={} blueprints={:?}",
        report.files_scanned, report.functions_seen, report.clusters_found, report.blueprints);

    // exactly ONE blueprint: the create<Entity> shape (appears 2x). The one-off helper is skipped.
    assert_eq!(report.clusters_found, 1, "only the repeated create-structure should harvest");
    let (shape, support, _id) = &report.blueprints[0];
    assert!(shape.starts_with("create"), "shape should be the create pattern, got {}", shape);
    assert_eq!(*support, 2, "support should be 2 (two create fns)");

    // the harvested blueprint is recallable, and carries the common skeleton.
    let got = sca_core::ask::recall_blueprints(&mut b, "create endpoint for an entity", 5, 0.0);
    assert!(!got.is_empty(), "harvested blueprint should be recallable");
    assert!(got[0].sections_json.contains("insert_row"), "sections = the common skeleton: {}", got[0].sections_json);

    cleanup(&p);
}

#[test]
fn re_harvest_is_keep_first_no_clobber() {
    let (mut b, p) = brain("rerun");
    let repo = fake_repo();
    let files: Vec<PathBuf> = repo.keys().cloned().collect();
    let read = |path: &std::path::Path| repo.get(path).cloned();

    let r1 = sca_core::harvest::harvest_blueprints(&mut b, files.clone(), &read);
    let id1 = r1.blueprints[0].2.clone();
    // a user promotes a better structure for that shape
    sca_core::ask::learn_blueprint(&mut b, &r1.blueprints[0].0,
        r#"{"sections":["my-hand-tuned-way"]}"#, None, None, true);
    // re-run harvest (e.g. next init) -> keep-first: must NOT overwrite the promoted version
    let _r2 = sca_core::harvest::harvest_blueprints(&mut b, files, &read);
    let body = b.get(&id1).unwrap_or_default();
    assert!(body.contains("my-hand-tuned-way"), "re-harvest must keep-first, not clobber the promoted blueprint:\n{}", body);

    cleanup(&p);
}
