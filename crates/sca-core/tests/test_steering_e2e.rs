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

/// THE TRUSTED CHANNEL (default): UserPromptSubmit. On the user's prompt the hook recalls `.said` and
/// PROVIDES the result as FACTUAL labeled data alongside the prompt — the channel the model actually
/// USES (proven live; PreToolUse/PostToolUse injection is distrusted as prompt-injection). The envelope
/// is hookEventName=UserPromptSubmit + additionalContext, framed as a `<project_index>` of facts (no
/// imperative, no meta-claim — that framing is what avoids the injection flag).
#[test]
fn claude_user_prompt_gets_factual_project_index() {
    let path = "test_steering_ups.said";
    let mut b = brain_with_code(path);

    // The Claude UserPromptSubmit stdin JSON carries the user's `prompt`.
    let stdin = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "prompt": "why do sessions expire too quickly, seconds instead of thirty minutes?"
    });

    let out = run_hook(&mut b, Agent::ClaudeCode, &stdin, sca_core::steering::SteerMode::Inject);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));

    let out = out.expect("a user prompt with a matching recall must emit a decision");
    eprintln!("UserPromptSubmit decision JSON:\n{}", serde_json::to_string_pretty(&out).unwrap());
    // TRUSTED-channel envelope: UserPromptSubmit + additionalContext, NO permissionDecision.
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
    assert!(out["hookSpecificOutput"].get("permissionDecision").is_none());
    let ctx = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("");
    assert!(ctx.contains("make_session"), "factual index must point at make_session; got:\n{ctx}");
    assert!(ctx.contains("<project_index"), "must be framed as a factual <project_index> block");
    // Honesty/framing: NO imperative command (that's what trips the injection defense).
    assert!(!ctx.to_lowercase().contains("instead of grep") && !ctx.to_lowercase().contains("you must"),
        "context must be FACTUAL, not an imperative instruction; got:\n{ctx}");
}

/// POSTToolUse "allow then redirect" (the trusted channel): grep already RAN; the hook injects the
/// `.said` recall as feedback on the result. The agent acts on it (vs distrusting pre-tool injection).
#[test]
fn claude_post_tool_grep_redirects_with_recall() {
    let path = "test_steering_post.said";
    let mut b = brain_with_code(path);
    // Claude PostToolUse JSON carries tool_name + tool_input + tool_output (the grep result).
    let stdin = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Grep",
        "tool_input": { "pattern": "session expires too quickly seconds instead of minutes" },
        "tool_output": { "type": "text", "text": "(grep found nothing useful)" }
    });
    let out = run_hook(&mut b, Agent::ClaudeCode, &stdin, sca_core::steering::SteerMode::Inject);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}.spill"));
    let out = out.expect("post-tool code search must emit a redirect");
    eprintln!("post-tool decision:\n{}", serde_json::to_string_pretty(&out).unwrap());
    // PostToolUse envelope: hookEventName=PostToolUse + additionalContext (NO permissionDecision).
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert!(out["hookSpecificOutput"].get("permissionDecision").is_none(),
        "PostToolUse must NOT carry a permissionDecision (the tool already ran)");
    let ctx = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("");
    assert!(ctx.contains("make_session"), "redirect must point at the buggy make_session; got:\n{ctx}");
    assert!(ctx.contains("<project_index"), "redirect must be a factual <project_index> block");
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
