#![cfg(all(feature = "embed-model", feature = "code"))]
//! Project scoping: a fix learned under SAID_PROJECT carries a `project:<name>` tag, and recall
//! filtered by SAID_RECALL_PROJECT returns ONLY that project's fix (cross-project isolation), while
//! unset recall sees both (cross-project reuse remains possible). Mirrors the SAID_RECALL_LANG design.
//!   cargo test -p sca-core --no-default-features --features "embed-model,code" --test test_project_scope -- --nocapture
use sca_core::said_file::SaidFile;

fn learn(b: &mut SaidFile, project: &str, problem: &str, note: &str) -> String {
    std::env::set_var("SAID_PROJECT", project);
    let id = sca_core::ask::learn_coding_fix(b, problem, note, "[]", None);
    std::env::remove_var("SAID_PROJECT");
    id
}

#[test]
fn recall_is_project_scoped_when_requested_but_open_by_default() {
    let p = std::env::temp_dir().join(format!("said_proj_{}.said", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let mut b = SaidFile::create(p.to_string_lossy().as_ref());
    assert!(b.auto_load_encoder());

    // same problem shape, two different projects
    learn(&mut b, "said-build", "implement an LRU cache O(1) eviction", "PROJECT A: said-build LRU note");
    learn(&mut b, "said-echo",  "implement an LRU cache O(1) eviction", "PROJECT B: said-echo LRU note");

    let q = "least-recently-used cache O(1)";

    // (1) DEFAULT (no scope env): both reachable — cross-project reuse stays possible.
    std::env::remove_var("SAID_RECALL_PROJECT");
    let open = sca_core::ask::recall_coding_fixes(&mut b, q, 5, 0.0);
    assert!(open.len() >= 2, "unscoped recall should see both projects' fixes, got {}", open.len());

    // (2) SCOPED to said-build: only said-build's fix may come back.
    std::env::set_var("SAID_RECALL_PROJECT", "said-build");
    let scoped = sca_core::ask::recall_coding_fixes(&mut b, q, 5, 0.0);
    std::env::remove_var("SAID_RECALL_PROJECT");
    assert!(!scoped.is_empty(), "scoped recall should still find said-build's fix");
    for f in &scoped {
        let body = b.get(&f.doc_id).unwrap_or_default();
        assert!(body.contains("said-build"), "scoped recall leaked a non-said-build fix: {}", body);
        assert!(!body.contains("said-echo"), "scoped recall returned said-echo fix under said-build scope");
    }

    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
}
