#![cfg(all(feature = "embed-model", feature = "code"))]
//! Blueprint steering trigger: when the user's prompt expresses INTENT TO BUILD a shape and .said holds
//! a blueprint for it, decide() injects a factual "you already have this structure -- render it, write
//! only the 20%" nudge on the trusted UserPromptSubmit channel. This is what makes the agent REUSE the
//! 80% instead of recreating it, without being told. Fail-open when there's no build intent or no match.
//!   cargo test -p sca-core --no-default-features --features "embed-model,code" --test test_blueprint_steering -- --nocapture
use sca_core::said_file::SaidFile;
use sca_core::steering::{decide, HookEvent, HookPhase, HookDecision, SteerMode, ToolAction};

fn brain(tag: &str) -> (SaidFile, std::path::PathBuf) {
    let p = std::env::temp_dir().join(format!("said_bpsteer_{}_{}.said", tag, std::process::id()));
    let _ = std::fs::remove_file(&p);
    let mut b = SaidFile::create(p.to_string_lossy().as_ref());
    assert!(b.auto_load_encoder());
    (b, p)
}
fn cleanup(p: &std::path::PathBuf) {
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
}
fn prompt(b: &mut SaidFile, text: &str) -> HookDecision {
    let ev = HookEvent { phase: HookPhase::UserPromptSubmit, action: ToolAction::UserPrompt { prompt: text.into() } };
    decide(b, &ev, SteerMode::Inject)
}

#[test]
fn build_intent_with_a_known_blueprint_injects_the_reuse_nudge() {
    let (mut b, p) = brain("hit");
    sca_core::ask::learn_blueprint(&mut b, "Create<Entity> REST endpoint",
        r#"{"sections":["accept-and-audit","idempotency","guards","save","response"]}"#, None, None, false);

    // The user asks to build a NEW endpoint -> the agent should be nudged to reuse the blueprint.
    let d = prompt(&mut b, "create a new invoice endpoint");
    match d {
        HookDecision::Provide { context } => {
            assert!(context.to_lowercase().contains("blueprint")
                || context.contains("reusable structure"), "should label it a blueprint nudge:\n{}", context);
            assert!(context.contains("idempotency"), "should carry the sections to render:\n{}", context);
            assert!(context.contains("recall_blueprint"), "should name the tool for the authoritative copy:\n{}", context);
            assert!(!context.to_lowercase().contains("instead of grep"), "must stay factual, not imperative");
        }
        other => panic!("build intent + known blueprint must Provide the reuse nudge, got {:?}", other),
    }
    cleanup(&p);
}

#[test]
fn a_plain_question_is_not_treated_as_build_intent() {
    let (mut b, p) = brain("noverb");
    sca_core::ask::learn_blueprint(&mut b, "Create<Entity> REST endpoint",
        r#"{"sections":["accept-and-audit","guards"]}"#, None, None, false);

    // Reading/understanding -- NOT build intent -> no blueprint nudge (we don't pester a reader).
    let d = prompt(&mut b, "how does the invoice endpoint validate input");
    if let HookDecision::Provide { context } = &d {
        assert!(!context.contains("kind=\"blueprint\""),
            "a how-does-it-work question must not trigger the blueprint nudge:\n{}", context);
    }
    cleanup(&p);
}

#[test]
fn build_intent_with_no_blueprint_falls_through() {
    let (mut b, p) = brain("empty");
    // empty brain -> nothing to reuse -> passthrough (fail-open), agent derives it.
    let d = prompt(&mut b, "create a new payment endpoint");
    assert!(matches!(d, HookDecision::Passthrough),
        "build intent with no blueprint should passthrough, got {:?}", d);
    cleanup(&p);
}
