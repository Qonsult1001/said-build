//! Code knowledge graph: every symbol a node, call-edges traversable. Proves
//! extract_calls + code_calls (what a fn calls) + code_callers (who calls a fn).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code"
//!        --test test_code_graph -- --nocapture

#![cfg(all(feature = "embed-model", feature = "code"))]

use sca_core::said_file::SaidFile;
use sca_core::code_search::{ast_chunk, extract_calls};

#[test]
fn extract_calls_finds_call_targets() {
    let body = "fn validate_session(t: Token) -> bool {\n  let u = lookup_user(t);\n  check_token(t) && u.is_some()\n}";
    let calls = extract_calls(body, "validate_session");
    eprintln!("calls = {:?}", calls);
    assert!(calls.contains(&"lookup_user".to_string()));
    assert!(calls.contains(&"check_token".to_string()));
    assert!(!calls.contains(&"validate_session".to_string()), "must not self-link");
    assert!(!calls.contains(&"if".to_string()), "must not capture keywords");
}

#[test]
fn ast_chunk_populates_calls() {
    // realistic function names (≥2 chars — single letters are filtered as code noise like i/j)
    let src = "\
fn handle_request() {
    parse_body();
    write_response();
}
fn parse_body() {
    write_response();
}
fn write_response() {}
";
    let chunks = ast_chunk(src, "rs");
    for c in &chunks { eprintln!("chunk {} calls={:?}", c.name, c.calls); }
    let h = chunks.iter().find(|c| c.name == "handle_request").expect("chunk");
    assert!(h.calls.contains(&"parse_body".to_string()) && h.calls.contains(&"write_response".to_string()),
        "handle_request should call parse_body and write_response (got {:?})", h.calls);
}

#[test]
fn code_graph_traversal() {
    let path = "test_codegraph.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder());

    // Build a 3-function call chain: validate_session -> check_token -> lookup_user
    let src = "\
fn validate_session(t: i32) -> bool { check_token(t) }
fn check_token(t: i32) -> bool { lookup_user(t) > 0 }
fn lookup_user(t: i32) -> i32 { t }
";
    // ingest the way `init` does: ast_chunk, store each with call: edges
    for chunk in ast_chunk(src, "rs") {
        let doc_id = format!("auth.rs::{}::function:{}", chunk.name, chunk.start_line);
        brain.remember_as(&doc_id, &chunk.content, Some(&chunk.name));
        for callee in &chunk.calls {
            brain.add_tag(&doc_id, &format!("call:{}", callee));
        }
    }
    brain.build_index().expect("idx");

    // code_calls: what does validate_session call? -> check_token
    let calls = brain.code_calls("validate_session");
    eprintln!("validate_session calls -> {:?}", calls);
    assert!(calls.iter().any(|d| d.contains("check_token")),
        "validate_session should call check_token (got {:?})", calls);

    // code_callers: who calls lookup_user? -> check_token
    let callers = brain.code_callers("lookup_user");
    eprintln!("callers of lookup_user -> {:?}", callers);
    assert!(callers.iter().any(|d| d.contains("check_token")),
        "check_token should be a caller of lookup_user (got {:?})", callers);

    let _ = std::fs::remove_file(path);
}
