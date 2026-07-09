//! CLAIMS TEST — code recall the docs promise: exact symbol lookup (line-exact), multi-language
//! indexing, and SEMANTIC-INTENT code recall (find a function by what it DOES, not its name).
//! Call/caller-graph is covered by test_code_graph.rs; here we test the sym() + ask() surface.
//! See docs/said-structure/CLAIMS-COVERAGE.md.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code"
//!        --test test_recall_claims_code -- --nocapture

#![cfg(all(feature = "embed-model", feature = "code"))]

use sca_core::said_file::SaidFile;
use sca_core::code_search::ast_chunk;
use sca_core::symbol_index::SymbolKind;

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

/// Ingest source the way `init` does: AST-chunk, store each chunk, register its symbol so sym() can
/// find it line-exact, and add call: edges.
fn ingest(b: &mut SaidFile, file: &str, src: &str, lang: &str) {
    for chunk in ast_chunk(src, lang) {
        let doc_id = format!("{file}::{}::function:{}", chunk.name, chunk.start_line);
        b.remember_as(&doc_id, &chunk.content, Some(&chunk.name));
        b.record_symbol(&chunk.name, &doc_id, SymbolKind::Function, chunk.start_line as u32, chunk.end_line as u32);
        for callee in &chunk.calls { b.add_tag(&doc_id, &format!("call:{}", callee)); }
    }
}

/// CLAIM (3.6, code.md): exact symbol lookup returns the defining frame, line-exact.
#[test]
fn symbol_exact_lookup_is_line_exact() {
    let path = "test_claim_sym.said";
    let mut b = fresh(path);
    let src = "\
fn parse_header(buf: &[u8]) -> Header { decode(buf) }
fn validate_checksum(h: &Header) -> bool { h.crc == compute_crc(h) }
fn write_footer(out: &mut Vec<u8>) { out.extend(MAGIC) }
";
    ingest(&mut b, "format.rs", src, "rs");
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();

    let hits = b.sym("validate_checksum", 5);
    cleanup(path);
    eprintln!("sym(validate_checksum) -> {:?}", hits.iter().map(|h| (h.name.clone(), h.doc_id.clone(), h.start_line)).collect::<Vec<_>>());
    assert!(!hits.is_empty(), "exact symbol must be found");
    let h = &hits[0];
    assert_eq!(h.name, "validate_checksum", "exact name match");
    assert!(h.start_line >= 1, "line-exact: start_line populated (got {})", h.start_line);
}

/// CLAIM (code.md: 7 languages): symbols from DIFFERENT languages are all indexed + findable.
#[test]
fn multi_language_symbols_are_recallable() {
    let path = "test_claim_multilang.said";
    let mut b = fresh(path);
    ingest(&mut b, "svc.rs", "fn rust_handler(r: Req) -> Resp { build(r) }", "rs");
    ingest(&mut b, "svc.py", "def python_handler(req):\n    return build(req)\n", "py");
    ingest(&mut b, "svc.js", "function jsHandler(req) { return build(req); }", "js");
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();

    for sym in ["rust_handler", "python_handler", "jsHandler"] {
        let hits = b.sym(sym, 5);
        eprintln!("sym({sym}) -> {}", hits.len());
        assert!(hits.iter().any(|h| h.name == sym), "symbol {sym} from its language must be indexed");
    }
    cleanup(path);
}

/// CLAIM (public-overview): semantic-intent code recall — find a function by WHAT IT DOES, even when
/// the query shares no identifier with the code. This rides the SCA semantic engine over code text.
#[test]
fn semantic_intent_code_recall() {
    let path = "test_claim_codeintent.said";
    let mut b = fresh(path);
    let src = "\
fn retry_failed_webhooks(q: &Queue) { for w in q.dead_letters() { resend(w); } }
fn rotate_encryption_keys(kms: &Kms) { kms.schedule_rotation(90); }
fn compress_old_logs(dir: &Path) { for f in stale(dir) { gzip(f); } }
";
    ingest(&mut b, "jobs.rs", src, "rs");
    for i in 0..30 { b.remember_as(&format!("f{i}"), &format!("Unrelated note {i} about meetings."), None); }
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();

    // Query by INTENT, not name: "resend webhooks that failed" → retry_failed_webhooks.
    let (cands, _) = sca_core::ask::ask(&mut b, "resend the webhook deliveries that failed", 10, false, None);
    cleanup(path);
    let found = cands.iter().any(|c| c.doc_id.contains("retry_failed_webhooks"));
    eprintln!("semantic-intent code recall: top = {:?}", cands.iter().take(3).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(found, "must recall retry_failed_webhooks by intent (got {:?})", cands.iter().take(5).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
}

/// DIVISION OF LABOR (docs/said-structure/06-ingestion-plugins/lsp.md): `ask` is RETRIEVAL — it
/// returns the exact symbol the client asked for, at confidence 1.00, and does NOT auto-walk the
/// call-graph (that's a shallow untyped traversal; the LSP does type-precise references). The
/// call-graph is exposed as the explicit `code_calls`/`code_callers` verbs the caller invokes when
/// it wants the neighbourhood. This test pins that boundary: ask returns the symbol; the verbs
/// (covered in test_code_graph.rs) return the edges.
#[test]
fn ask_returns_symbol_not_auto_walked_graph() {
    let path = "test_claim_no_autowalk.said";
    let mut b = fresh(path);
    let src = "\
fn validate_session(t: i32) -> bool { check_token(t) }
fn check_token(t: i32) -> bool { lookup_user(t) > 0 }
fn lookup_user(t: i32) -> i32 { t }
";
    ingest(&mut b, "auth.rs", src, "rs");
    for i in 0..30 { b.remember_as(&format!("f{i}"), &format!("Unrelated note {i} about budgets."), None); }
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();

    // ask returns the matched symbol itself (the detail the client asked for), at the top.
    let (cands, _) = sca_core::ask::ask(&mut b, "check_token", 10, false, None);
    eprintln!("ask('check_token') top = {:?}", cands.iter().take(3).map(|c| c.doc_id.clone()).collect::<Vec<_>>());
    assert!(cands.iter().any(|c| c.doc_id.contains("check_token")), "ask must return the exact symbol");
    // It does NOT inject the call-graph neighbourhood (no kind:"code_graph" candidates).
    assert!(!cands.iter().any(|c| c.kind == "code_graph"), "ask must NOT auto-walk the call-graph");

    // The call-graph is available as explicit verbs the LLM invokes deliberately, then hands to LSP.
    let callers = b.code_callers("check_token");
    let callees = b.code_calls("check_token");
    cleanup(path);
    assert!(callers.iter().any(|d| d.contains("validate_session")), "code_callers verb returns the caller");
    assert!(callees.iter().any(|d| d.contains("lookup_user")), "code_calls verb returns the callee");
}
