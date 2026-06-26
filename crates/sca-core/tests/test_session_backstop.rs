#![cfg(all(feature = "embed-model", feature = "code"))]
use sca_core::said_file::SaidFile;
use sca_core::steering::{run_hook, Agent, SteerMode};

fn write_transcript(path: &str) {
    let lines = [
        r#"{"type":"user","message":{"content":[{"type":"text","text":"fix the session expiry bug"}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Found it: expires_at was in seconds not minutes; multiplied by 60 in make_session. Tests pass."}]}}"#,
    ];
    std::fs::write(path, lines.join("\n")).unwrap();
}

#[test]
fn backstop_writes_journal_when_agent_did_not() {
    let p = "test_backstop1.said";
    let _ = std::fs::remove_file(p);
    let tx = "test_backstop1_transcript.jsonl";
    write_transcript(tx);
    {
        let mut b = SaidFile::create(p); assert!(b.auto_load_encoder());
        b.remember_as("code/x", "some indexed code", None);
        b.build_index().unwrap(); b.save().unwrap();
    }
    let mut b = SaidFile::open(p).unwrap();
    let stdin = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "sess-AAA",
        "transcript_path": tx
    });
    let out = run_hook(&mut b, Agent::ClaudeCode, &stdin, SteerMode::Inject);
    assert!(out.is_none(), "SessionEnd emits no decision (write path)");
    // a backstop journal must now exist with the session tag
    let found = b.frames.active_doc_ids().iter().any(|d| {
        b.frames.get_meta(d).map(|m|
            m.tags.iter().any(|t| t=="source:backstop") &&
            m.tags.iter().any(|t| t=="session:sess-AAA")
        ).unwrap_or(false)
    });
    assert!(found, "backstop must write a journal tagged source:backstop + session:sess-AAA");
    // and it must be recallable
    let (cands,_) = sca_core::ask::ask(&mut b, "session expiry bug", 5, false, None);
    assert!(cands.iter().any(|c| c.content.contains("expires_at") || c.content.contains("make_session")),
        "the backstop summary must be retrievable");
    let _ = std::fs::remove_file(p); let _ = std::fs::remove_file(tx);
}

#[test]
fn backstop_skips_when_agent_already_wrote_this_session() {
    let p = "test_backstop2.said";
    let _ = std::fs::remove_file(p);
    let tx = "test_backstop2_transcript.jsonl";
    write_transcript(tx);
    {
        let mut b = SaidFile::create(p); assert!(b.auto_load_encoder());
        // simulate the AGENT having journaled this session (tag carries the session id)
        b.remember_as("mem/today/agent-journal", "agent's own distilled summary", Some("Journal"));
        b.add_tag("mem/today/agent-journal", "kind:journal");
        b.add_tag("mem/today/agent-journal", "session:sess-BBB");
        b.build_index().unwrap(); b.save().unwrap();
    }
    let mut b = SaidFile::open(p).unwrap();
    let before = b.frames.active_doc_ids().len();
    let stdin = serde_json::json!({
        "hook_event_name": "SessionEnd", "session_id": "sess-BBB", "transcript_path": tx
    });
    run_hook(&mut b, Agent::ClaudeCode, &stdin, SteerMode::Inject);
    let after = b.frames.active_doc_ids().len();
    assert_eq!(before, after, "backstop must NOT write when the agent already journaled this session");
    // no backstop-tagged frame should exist
    let has_backstop = b.frames.active_doc_ids().iter().any(|d|
        b.frames.get_meta(d).map(|m| m.tags.iter().any(|t| t=="source:backstop")).unwrap_or(false));
    assert!(!has_backstop, "agent's write wins — no backstop frame");
    let _ = std::fs::remove_file(p); let _ = std::fs::remove_file(tx);
}

#[test]
fn backstop_idempotent_on_repeat() {
    let p = "test_backstop3.said";
    let _ = std::fs::remove_file(p);
    let tx = "test_backstop3_transcript.jsonl";
    write_transcript(tx);
    { let mut b = SaidFile::create(p); assert!(b.auto_load_encoder());
      b.remember_as("code/y","z",None); b.build_index().unwrap(); b.save().unwrap(); }
    let mut b = SaidFile::open(p).unwrap();
    let stdin = serde_json::json!({"hook_event_name":"SessionEnd","session_id":"sess-CCC","transcript_path":tx});
    run_hook(&mut b, Agent::ClaudeCode, &stdin, SteerMode::Inject);
    let after_first = b.frames.active_doc_ids().len();
    run_hook(&mut b, Agent::ClaudeCode, &stdin, SteerMode::Inject); // again
    let after_second = b.frames.active_doc_ids().len();
    assert_eq!(after_first, after_second, "repeat SessionEnd for same session must be a no-op");
    let _ = std::fs::remove_file(p); let _ = std::fs::remove_file(tx);
}
