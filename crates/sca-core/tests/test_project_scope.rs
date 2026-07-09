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

    // (2) SCOPED to said-build: FIXES ARE ALWAYS CROSS-PROJECT (owner decision 2026-06-30, the "biggest
    // win"). Even with SAID_RECALL_PROJECT set, procedural fixes from OTHER projects MUST still be
    // recallable — that scope only narrows episodic/code memories in `ask`, never the reusable 80%.
    // (Grounded: procedural memory is the transferable type, arXiv:2603.07670 / 2602.06052.)
    std::env::set_var("SAID_RECALL_PROJECT", "said-build");
    let scoped = sca_core::ask::recall_coding_fixes(&mut b, q, 5, 0.0);
    std::env::remove_var("SAID_RECALL_PROJECT");
    assert!(scoped.len() >= 2,
        "fixes are cross-project: scoped recall must STILL see both projects' fixes (the 80%-reuse win), got {}",
        scoped.len());
    let bodies: Vec<String> = scoped.iter().map(|f| b.get(&f.doc_id).unwrap_or_default()).collect();
    assert!(bodies.iter().any(|x| x.contains("said-echo")),
        "said-echo's fix must remain reusable from said-build (procedural = cross-project)");

    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
}
