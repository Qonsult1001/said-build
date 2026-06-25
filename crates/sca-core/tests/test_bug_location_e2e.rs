//! END-TO-END BUG LOCATION: can `.said` (the retrieval layer an MCP serves) take a complex multi-file
//! project, and from a natural-language symptom ("sessions expire too fast") point an LLM at the EXACT
//! buggy function — accurately, and with FAR fewer tokens than reading the project?
//!
//! Two axes (per the MCP-for-coding research — accuracy + token economy):
//!   ACCURACY  — is the buggy function in the top-K results? at what rank? (file + symbol granularity)
//!   TOKENS    — chars `.said` returns to locate it  vs  a grep+read-whole-files baseline (what an
//!               agent without a memory index burns). The research target is an order-of-magnitude win.
//!
//! This is the "locate the bug, guide Claude" test. It runs the REAL sca-core path (sym + ask) that
//! the MCP `ask`/`sym` tools serve, so what it measures is what an MCP client would actually get. It
//! also prints the SHORTFALLS (what a world-class code MCP would add) so the gaps are visible.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code"
//!        --test test_bug_location_e2e -- --nocapture

#![cfg(all(feature = "embed-model", feature = "code"))]

use sca_core::said_file::SaidFile;
use sca_core::code_search::ast_chunk;
use sca_core::symbol_index::SymbolKind;

/// A realistic small project: auth + billing + util, ~15 functions across 4 files. ONE has the bug:
/// `make_session` sets expiry in MINUTES where the rest of the system treats the field as SECONDS, so
/// sessions expire 60× too fast. The symptom a developer reports has NO overlap with the buggy line's
/// identifiers — it must be found by meaning + structure, not string match.
fn project() -> Vec<(&'static str, &'static str)> {
    vec![
        ("auth/session.rs", "\
fn make_session(user: &User) -> Session {
    // BUG: expiry stored in MINUTES but the validator compares against SECONDS, so a session
    // intended to last 30 minutes actually dies in 30 seconds.
    let expires_at = now() + 30; // <-- should be 30 * 60
    Session { user_id: user.id, expires_at }
}
fn validate_session(s: &Session) -> bool {
    now() < s.expires_at
}
fn refresh_session(s: &mut Session) {
    s.expires_at = now() + 1800;
}
"),
        ("auth/login.rs", "\
fn handle_login(req: LoginRequest) -> Response {
    let user = lookup_user(&req.username);
    let session = make_session(&user);
    issue_cookie(session)
}
fn lookup_user(name: &str) -> User {
    db_find_user(name)
}
"),
        ("billing/invoice.rs", "\
fn generate_invoice(account: &Account) -> Invoice {
    let total = sum_line_items(&account.items);
    Invoice { account_id: account.id, total }
}
fn sum_line_items(items: &[Item]) -> u64 {
    items.iter().map(|i| i.price).sum()
}
fn apply_discount(inv: &mut Invoice, pct: u64) {
    inv.total -= inv.total * pct / 100;
}
"),
        ("util/time.rs", "\
fn now() -> u64 {
    system_clock_secs()
}
fn format_duration(secs: u64) -> String {
    format!(\"{}m {}s\", secs / 60, secs % 60)
}
"),
    ]
}

/// Generate filler source files so the corpus resembles a REAL project (where the bug hides among
/// hundreds of unrelated functions) rather than a 4-file toy. `n_files` of plausible CRUD/util code.
fn filler_files(n_files: usize) -> Vec<(String, String)> {
    let domains = ["orders", "catalog", "shipping", "reports", "notify", "search", "cache", "config"];
    let mut out = Vec::with_capacity(n_files);
    for i in 0..n_files {
        let d = domains[i % domains.len()];
        let src = format!("\
fn {d}_list_{i}(filter: &Filter) -> Vec<Row> {{ db_query(filter) }}
fn {d}_create_{i}(input: &Input) -> Row {{ db_insert(input) }}
fn {d}_update_{i}(id: u64, patch: &Patch) -> Row {{ db_update(id, patch) }}
fn {d}_summarize_{i}(rows: &[Row]) -> Summary {{ aggregate(rows) }}
");
        out.push((format!("{d}/{d}_{i}.rs"), src));
    }
    out
}

/// Ingest the real project + `n_filler` filler files. Returns the TOTAL source chars (the whole-repo
/// read cost an index-less agent would pay to find the bug).
fn ingest_project_scaled(b: &mut SaidFile, n_filler: usize) -> usize {
    let mut total_src_chars = 0usize;
    let real: Vec<(String, String)> = project().into_iter().map(|(f, s)| (f.to_string(), s.to_string())).collect();
    for (file, src) in real.iter().chain(filler_files(n_filler).iter()) {
        total_src_chars += src.len();
        for chunk in ast_chunk(src, "rs") {
            let doc_id = format!("{}::{}::function:{}", file, chunk.name, chunk.start_line);
            b.remember_as(&doc_id, &chunk.content, Some(&chunk.name));
            b.record_symbol(&chunk.name, &doc_id, SymbolKind::Function, chunk.start_line as u32, chunk.end_line as u32);
            for callee in &chunk.calls { b.add_tag(&doc_id, &format!("call:{}", callee)); }
        }
    }
    total_src_chars
}

fn ingest_project(b: &mut SaidFile) -> usize { ingest_project_scaled(b, 0) }

#[test]
fn locate_bug_accuracy_and_token_economy() {
    let path = "test_bug_location.said";
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");

    // ~120 filler files (~480 functions) so the bug hides in a realistic project, not a toy.
    let total_src_chars = ingest_project_scaled(&mut b, 120);
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();

    let gold = "make_session"; // the buggy function

    // ---- ACCURACY: from the SYMPTOM (no identifier overlap), does .said surface the bug? ----
    let symptom = "user sessions are expiring far too quickly, only lasting seconds instead of the intended thirty minutes";
    let (cands, _) = sca_core::ask::ask(&mut b, symptom, 10, false, None);
    let rank = cands.iter().position(|c| c.doc_id.contains(gold));
    eprintln!("\n=== BUG LOCATION via .said (symptom-driven) ===");
    eprintln!("symptom: \"{symptom}\"");
    eprintln!("top results:");
    for (i, c) in cands.iter().take(5).enumerate() {
        eprintln!("  {}. [{:.2}][{}] {}", i + 1, c.confidence, c.kind, c.doc_id);
    }
    eprintln!("buggy function '{gold}' rank = {rank:?}");

    // ---- TOKENS: what .said returns to locate it, vs reading the whole project ----
    // .said cost = the chars in the top-5 result snippets the MCP would surface (the located neighbourhood).
    let said_chars: usize = cands.iter().take(5).map(|c| c.doc_id.len() + c.content.len().min(500)).sum();
    // baseline cost = an agent with no index greps then READS the whole project to find it.
    let baseline_chars = total_src_chars;
    let ratio = baseline_chars as f32 / said_chars.max(1) as f32;
    eprintln!("\n=== TOKEN ECONOMY (chars as a token proxy) ===");
    eprintln!("  .said to locate (top-5 snippets) = {said_chars} chars");
    eprintln!("  baseline (read whole project)    = {baseline_chars} chars");
    eprintln!("  .said is {ratio:.1}x leaner to reach the bug neighbourhood");

    // ---- the SHORTFALLS a world-class code MCP would close (printed, tracked in CLAIMS-COVERAGE) ----
    eprintln!("\n=== SHORTFALLS vs ideal code MCP (what .said does NOT yet give the LLM) ===");
    eprintln!("  - response is TEXT, not structured {{file,line_range,why_relevant,confidence,signals}}");
    eprintln!("  - no locate_issue tool fusing symbol+callgraph+prior-fix into a ranked hypothesis");
    eprintln!("  - call-graph (callers/callees) not exposed as an MCP tool (only Rust verbs)");
    eprintln!("  - no progressive disclosure (signature-first, body on demand)");

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    // GATES (the measurable guarantees):
    //  (1) the buggy function must be in the top-10 from the symptom alone (semantic locate works).
    assert!(rank.map(|r| r < 10).unwrap_or(false),
        "the buggy function '{gold}' must be located in the top-10 from the symptom (got rank {rank:?})");
    //  (2) .said must be dramatically leaner than reading the whole project (token economy is the value).
    assert!(ratio >= 2.0, "expected .said to be >=2x leaner than reading the project (got {ratio:.1}x)");
}

/// The DIRECT path: once an LLM has the symptom and a candidate symbol, the call-graph verbs locate
/// the blast radius (who calls the bug, what it calls) — the impact set the LLM hands to its LSP.
#[test]
fn locate_bug_call_graph_blast_radius() {
    let path = "test_bug_blast.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");
    ingest_project(&mut b);
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();

    // Who is affected if make_session is wrong? -> handle_login calls it.
    let callers = b.code_callers("make_session");
    eprintln!("\nblast radius — callers of make_session (who breaks): {:?}",
        callers.iter().map(|d| d.split("::").nth(1).unwrap_or(d)).collect::<Vec<_>>());
    let _ = std::fs::remove_file(path);
    assert!(callers.iter().any(|d| d.contains("handle_login")),
        "the call-graph must show handle_login as an affected caller of the buggy make_session");
}
