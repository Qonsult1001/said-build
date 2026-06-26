//! END-TO-END agent steering: the nudge "pipe the hook JSON, read the decision" test, against a REAL
//! `.said` brain. Proves the full path a `said hook` subcommand runs: agent PreToolUse stdin JSON →
//! `steering::run_hook` → agent stdout decision JSON, with the relevant `.said` recall injected.
//!
//! Mirrors nudge's developer-guide test workflow (`printf '{...}' | nudge claude hook`).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code"
//!        --test test_steering_e2e -- --nocapture

#![cfg(all(feature = "embed-model", feature = "code"))]

use sca_core::said_file::SaidFile;
use sca_core::steering::{run_hook, Agent};
use sca_core::code_search::ast_chunk;
use sca_core::symbol_index::SymbolKind;

fn brain_with_code(path: &str) -> SaidFile {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");
    let src = "\
fn make_session(user: &User) -> Session {
    let expires_at = now() + 30; // minutes-vs-seconds bug
    Session { user_id: user.id, expires_at }
}
fn validate_session(s: &Session) -> bool { now() < s.expires_at }
";
    for chunk in ast_chunk(src, "rs") {
        let doc_id = format!("auth/session.rs::{}::function:{}", chunk.name, chunk.start_line);
        b.remember_as(&doc_id, &chunk.content, Some(&chunk.name));
        b.record_symbol(&chunk.name, &doc_id, SymbolKind::Function, chunk.start_line as u32, chunk.end_line as u32);
    }
    for i in 0..20 { b.remember_as(&format!("f{i}"), &format!("Unrelated note {i}."), None); }
    b.build_index().expect("build_index");
    b.rebuild_trigram_index();
    b
}

/// The agent is about to GREP for the session-expiry bug. The hook should ALLOW + inject `.said`
/// recall that points at make_session (so the agent can skip the grep). nudge's inject-and-proceed.
#[test]
fn claude_grep_gets_said_recall_injected() {
    let path = "test_steering_grep.said";
    let mut b = brain_with_code(path);

    // The exact Claude PreToolUse stdin JSON for a Grep tool call.
    let stdin = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Grep",
        "tool_input": { "pattern": "session expires too quickly seconds instead of minutes" }
    });

    let out = run_hook(&mut b, Agent::ClaudeCode, &stdin, sca_core::steering::SteerMode::Inject);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    let out = out.expect("a code-search hook must emit a decision");
    eprintln!("hook decision JSON:\n{}", serde_json::to_string_pretty(&out).unwrap());
    // It must be an ALLOW (never block) with injected context.
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "allow", "must allow (inject-and-proceed)");
    let ctx = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("");
    assert!(ctx.contains("make_session"), "injected .said context must point at the buggy make_session; got:\n{ctx}");
    assert!(ctx.contains(".said memory"), "context must be labelled as .said memory");
}

/// A Bash grep/rg command is also a code search → same inject behaviour.
#[test]
fn claude_bash_grep_gets_recall() {
    let path = "test_steering_bash.said";
    let mut b = brain_with_code(path);
    let stdin = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": "rg 'expires_at' src/" }
    });
    let out = run_hook(&mut b, Agent::ClaudeCode, &stdin, sca_core::steering::SteerMode::Inject);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    let out = out.expect("bash-grep must emit a decision");
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "allow");
}

/// A NON-search tool (Write) must pass through untouched — the hook only acts on code search.
#[test]
fn claude_write_passes_through() {
    let path = "test_steering_write.said";
    let mut b = brain_with_code(path);
    let stdin = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": "x.rs", "content": "fn x(){}" }
    });
    let out = run_hook(&mut b, Agent::ClaudeCode, &stdin, sca_core::steering::SteerMode::Inject);
    // A non-search Bash command (not grep) also passes through.
    let stdin2 = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": "cargo test" }
    });
    let out2 = run_hook(&mut b, Agent::ClaudeCode, &stdin2, sca_core::steering::SteerMode::Inject);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    assert!(out.is_none(), "Write must pass through (emit nothing)");
    assert!(out2.is_none(), "non-grep Bash must pass through (emit nothing)");
}

/// FAIL-OPEN: a code search with NO relevant memory must pass through (never block, never error).
#[test]
fn no_relevant_memory_fails_open() {
    let path = "test_steering_failopen.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder(), "encoder");
    // a brain with only unrelated notes
    for i in 0..20 { b.remember_as(&format!("f{i}"), &format!("Cooking recipe note {i} about pasta."), None); }
    b.build_index().expect("build_index");

    let stdin = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Grep",
        "tool_input": { "pattern": "kubernetes ingress controller TLS termination" }
    });
    let out = run_hook(&mut b, Agent::ClaudeCode, &stdin, sca_core::steering::SteerMode::Inject);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    // No relevant hit → passthrough (None). Must NOT block, must NOT inject noise.
    assert!(out.is_none(), "no relevant memory must fail open (passthrough), got: {out:?}");
}
