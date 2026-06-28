#![cfg(all(feature = "embed-model", feature = "code"))]
//! Point 2: process-state persists to .said and resumes via the nudge injection channel.
//! On SessionStart, decide() injects the MOST RECENT journal ("where you left off") so the agent resumes
//! from .said instead of Claude's ephemeral session memory. Mirrors how Claude reloads its own memory,
//! but to the durable, portable, BYO-LLM store.
//!   cargo test -p sca-core --no-default-features --features "embed-model,code" --test test_session_resume -- --nocapture
use sca_core::said_file::SaidFile;
use sca_core::steering::{decide, HookEvent, HookPhase, HookDecision, SteerMode, ToolAction};

#[test]
fn session_start_injects_the_last_journal_as_resume_context() {
    let p = std::env::temp_dir().join(format!("said_resume_{}.said", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let mut b = SaidFile::create(p.to_string_lossy().as_ref());
    assert!(b.auto_load_encoder());

    // The agent journaled its process-state last session (wanted/decided/built/blockers/NEXT).
    b.remember_as("mem/2026-06-28/cart-work",
        "WANTED: interactive cart. DECIDED: round money at the end. BUILT: cart.js + app.js. \
         BLOCKERS: none. NEXT: add the TTL feature to the Store and wire the coupon UI.",
        Some("Journal: cart-work"));
    b.add_tag("mem/2026-06-28/cart-work", "kind:journal");
    b.build_index().unwrap();

    // New session starts (no chat memory). The hook fires SessionStart.
    let ev = HookEvent {
        phase: HookPhase::SessionStart,
        action: ToolAction::UserPrompt { prompt: String::new() }, // SessionStart has no prompt
    };
    let decision = decide(&mut b, &ev, SteerMode::Inject);

    match decision {
        HookDecision::Provide { context } => {
            assert!(context.contains("NEXT") || context.to_lowercase().contains("ttl"),
                "SessionStart should resume the last journal's NEXT/state; got:\n{}", context);
            assert!(context.to_lowercase().contains("session") || context.contains("project_memory")
                || context.contains("resume") || context.contains("left off"),
                "resume context should be labeled as prior-session state; got:\n{}", context);
        }
        other => panic!("SessionStart with a prior journal must Provide resume context, got {:?}", other),
    }
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
}
