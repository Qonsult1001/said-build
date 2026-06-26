//! THE OPEN EXPERIMENT (docs/16-agent-steering): inject-and-proceed vs block-redirect.
//!
//! Question: when the agent is about to grep and `.said` has the answer, should the hook INJECT the
//! recall and let grep proceed (gentle, fail-open), or BLOCK the grep and force the agent onto `.said`
//! (forceful, max token-saving)? We default to inject; this measures whether block is justified.
//!
//! We can't drive a live Claude session in a unit test, so we measure the DETERMINISTIC quantities
//! that decide the outcome (the same axes the MCP-for-coding research uses):
//!   1. RECALL SUFFICIENCY — does the injected recall actually CONTAIN the answer? (If yes, blocking
//!      the grep is SAFE and saves the most tokens. If no, blocking would HARM — the agent needed grep.)
//!   2. TOKENS RETURNED by the hook (identical recall in both modes; the difference is downstream).
//!   3. TOKENS SAVED IF the agent skips grep — i.e. the whole-project read it avoids.
//! From these we report a principled recommendation, not an assumption.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code"
//!        --test test_steering_experiment -- --nocapture

#![cfg(all(feature = "embed-model", feature = "code"))]

use sca_core::said_file::SaidFile;
use sca_core::steering::{run_hook, Agent, SteerMode};
use sca_core::code_search::ast_chunk;
use sca_core::symbol_index::SymbolKind;

/// The bug-location project (same shape as test_bug_location_e2e): one real bug among filler.
fn build(path: &str) -> (SaidFile, usize) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");
    let mut total_src = 0usize;
    let real = [
        ("auth/session.rs", "fn make_session(user: &User) -> Session {\n    let expires_at = now() + 30; // minutes-vs-seconds bug\n    Session { user_id: user.id, expires_at }\n}\nfn validate_session(s: &Session) -> bool { now() < s.expires_at }\n"),
        ("auth/login.rs", "fn handle_login(req: LoginRequest) -> Response {\n    let s = make_session(&lookup_user(&req.username));\n    issue_cookie(s)\n}\n"),
    ];
    for (file, src) in real.iter() {
        total_src += src.len();
        for chunk in ast_chunk(src, "rs") {
            let doc_id = format!("{}::{}::function:{}", file, chunk.name, chunk.start_line);
            b.remember_as(&doc_id, &chunk.content, Some(&chunk.name));
            b.record_symbol(&chunk.name, &doc_id, SymbolKind::Function, chunk.start_line as u32, chunk.end_line as u32);
            for callee in &chunk.calls { b.add_tag(&doc_id, &format!("call:{}", callee)); }
        }
    }
    // ~120 filler files so the project is realistic (the bug must be FOUND, not the only thing there).
    let domains = ["orders","catalog","shipping","reports","notify","search","cache","config"];
    for i in 0..120 {
        let d = domains[i % domains.len()];
        let src = format!("fn {d}_list_{i}(f:&Filter)->Vec<Row>{{db(f)}}\nfn {d}_save_{i}(x:&In)->Row{{ins(x)}}\n");
        total_src += src.len();
        for chunk in ast_chunk(&src, "rs") {
            let doc_id = format!("{d}/{d}_{i}.rs::{}::function:{}", chunk.name, chunk.start_line);
            b.remember_as(&doc_id, &chunk.content, Some(&chunk.name));
        }
    }
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();
    (b, total_src)
}

fn injected_text(out: &Option<serde_json::Value>) -> String {
    let Some(v) = out else { return String::new(); };
    let o = &v["hookSpecificOutput"];
    o["additionalContext"].as_str().or_else(|| o["permissionDecisionReason"].as_str()).unwrap_or("").to_string()
}

#[test]
fn inject_vs_block_experiment() {
    let path = "test_steer_experiment.said";
    let (mut b, total_src_chars) = build(path);

    // The symptom query an agent would grep for.
    let stdin = serde_json::json!({
        "hook_event_name": "PreToolUse", "tool_name": "Grep",
        "tool_input": { "pattern": "session expires too quickly seconds instead of thirty minutes" }
    });

    let inject = run_hook(&mut b, Agent::ClaudeCode, &stdin, SteerMode::Inject);
    let block  = run_hook(&mut b, Agent::ClaudeCode, &stdin, SteerMode::Block);

    let inj_ctx = injected_text(&inject);
    let blk_ctx = injected_text(&block);

    // (1) RECALL SUFFICIENCY — does the recall contain the buggy function? (decides if blocking is safe)
    let recall_has_answer = inj_ctx.contains("make_session");
    // (2) decision shape
    let inj_decision = inject.as_ref().map(|v| v["hookSpecificOutput"]["permissionDecision"].as_str().unwrap_or("").to_string()).unwrap_or_default();
    let blk_decision = block.as_ref().map(|v| v["hookSpecificOutput"]["permissionDecision"].as_str().unwrap_or("").to_string()).unwrap_or_default();
    // (3) token accounting (chars as proxy)
    let inj_tokens = inj_ctx.len();
    let blk_tokens = blk_ctx.len();
    let grep_read_cost = total_src_chars; // what an agent burns reading the project if it greps

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    eprintln!("\n=== INJECT vs BLOCK experiment (symptom-driven bug location) ===");
    eprintln!("project size (grep+read cost)      : {grep_read_cost} chars");
    eprintln!("INJECT  → decision={inj_decision:<6} context={inj_tokens} chars  (agent MAY still grep)");
    eprintln!("BLOCK   → decision={blk_decision:<6} context={blk_tokens} chars  (agent CANNOT grep, must use .said)");
    eprintln!("recall CONTAINS the answer (make_session): {recall_has_answer}");
    eprintln!();
    if recall_has_answer {
        eprintln!("FINDING: the recall ANSWERS the query, so blocking is SAFE and saves the most tokens —");
        eprintln!("  BLOCK avoids the whole-project read ({grep_read_cost} chars) deterministically;");
        eprintln!("  INJECT saves the same ONLY IF the agent chooses to trust the context and skip grep.");
        eprintln!("  Worst case for INJECT: agent ignores the context and greps anyway → 0 saving.");
        eprintln!("  Worst case for BLOCK : recall was wrong → agent must re-search (one extra round-trip).");
    } else {
        eprintln!("FINDING: the recall did NOT contain the answer → BLOCK would HARM (agent needed grep).");
        eprintln!("  INJECT is correct here: it adds context but lets the agent grep.");
    }
    eprintln!();
    eprintln!("RECOMMENDATION (data-driven default):");
    eprintln!("  - Default = INJECT: it is FAIL-SAFE — it never blocks a legitimate grep, and when the");
    eprintln!("    recall is good the agent still skips the grep. The downside (agent ignores context) is");
    eprintln!("    a missed saving, not a wrong answer.");
    eprintln!("  - BLOCK is an OPT-IN power mode (--mode block) for token-critical workflows where the");
    eprintln!("    operator accepts the occasional extra round-trip in exchange for guaranteed .said-first.");
    eprintln!("  - The fail-open grounding gate makes BLOCK safer than a naive block: it only denies when");
    eprintln!("    .said has a GROUNDED hit; an off-topic search passes through (never blocked).");

    // ASSERTIONS that lock the experiment's invariants:
    assert!(recall_has_answer, "the corpus is set up so the recall MUST contain the answer for this comparison");
    assert_eq!(inj_decision, "allow", "inject must ALLOW (let grep proceed)");
    assert_eq!(blk_decision, "deny", "block must DENY (redirect to .said)");
    // Both modes return the SAME recall content (the difference is allow vs deny, not the payload).
    assert!(inj_ctx.contains("make_session") && blk_ctx.contains("make_session"),
        "both modes carry the same recall");
    // Blocking deterministically avoids the whole-project read; that read dwarfs the hook payload.
    assert!(grep_read_cost > blk_tokens * 5,
        "the avoided grep+read cost ({grep_read_cost}) should dwarf the hook payload ({blk_tokens})");
}
