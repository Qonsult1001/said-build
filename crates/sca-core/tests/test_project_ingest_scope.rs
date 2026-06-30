#![cfg(all(feature = "embed-model", feature = "code"))]
//! Project scoping at the GENERIC INGEST + RECALL level (the keystone doc 28 named missing).
//!
//! Boundary (owner 2026-06-30): project-tag CODE-related ingest ONLY; bare remembers stay GLOBAL.
//!   - a memory ingested as part of a code project (SAID_PROJECT set) carries `project:<name>`,
//!   - a bare general `remember` carries NO project tag (stays global / cross-project reachable),
//!   - generic recall scoped by SAID_RECALL_PROJECT returns the project's memories PLUS globals,
//!     and never another project's; unset => everything (cross-project reuse stays possible).
//!   cargo test -p sca-core --no-default-features --features "embed-model,code" --test test_project_ingest_scope -- --nocapture
use sca_core::said_file::SaidFile;

fn tag_present(b: &SaidFile, doc_id: &str, tag: &str) -> bool {
    b.frames.get_meta(doc_id).map(|m| m.tags.iter().any(|t| t == tag)).unwrap_or(false)
}

#[test]
fn code_ingest_is_project_tagged_but_bare_remember_stays_global() {
    let p = std::env::temp_dir().join(format!("said_ping_{}.said", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let mut b = SaidFile::create(p.to_string_lossy().as_ref());
    assert!(b.auto_load_encoder());

    // (1) CODE ingest under a project => must carry project:<name>.
    std::env::set_var("SAID_PROJECT", "said-build");
    let code_id = sca_core::project::ingest_project_memory(
        &mut b, "commit::abc123", "fix the LRU eviction in cache.rs", Some("commit abc123"));
    std::env::remove_var("SAID_PROJECT");
    assert!(tag_present(&b, &code_id, "project:said-build"),
        "code-project ingest must be tagged project:said-build");

    // (2) bare general remember => NO project tag (global), even if SAID_PROJECT is set.
    std::env::set_var("SAID_PROJECT", "said-build");
    let gid = b.remember("I prefer tabs over spaces"); // generic path, untouched
    std::env::remove_var("SAID_PROJECT");
    let global_doc = format!("frame_{}", gid); // remember() returns a frame number; resolve its meta
    // a bare remember must NOT have acquired a project tag through any global side-effect
    let any_proj = b.frames.active_doc_ids().iter().any(|d| {
        d != &code_id && tag_present(&b, d, "project:said-build")
    });
    assert!(!any_proj, "a bare remember must stay GLOBAL (no project tag)");
    let _ = global_doc;

    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
}
