//! `said-orchestration` — the closed-loop coding orchestrator.
//!
//! Drives ANY external LLM through the Claude-Code lifecycle using three .said
//! pieces, each in its proper home (per docs/said-orchestrator-design.md):
//!
//!   - **said-prompts**  → the phase playbook (plan/design/code/test/repair)
//!   - **sca-core**      → project memory (recall the verified iteration story)
//!   - **said-llm**      → the model (Anthropic / OpenAI-compatible / Claude-CLI)
//!
//! This is a SEPARATE process. The core .said binary (said-cli/said-mcp) NEVER
//! calls an LLM — that BYO-LLM boundary (docs/said-structure/13-integrations.md
//! Rule 2) is why this lives in its own crate, like said-forge / said-think.
//!
//! ## The steps are EXPLICIT
//!
//! The Claude lifecycle is split one-file-per-step under [`steps`], so the order
//! that is ALWAYS followed is visible in the module structure:
//!
//!   [`steps::plan`] → [`steps::design`] → [`steps::code`] → [`steps::test`]
//!   → on red: [`steps::repair`] (loop) → on green: [`steps::learn`]
//!
//! [`steps::run`] is the pipeline that runs them in that fixed order. The
//! build/test gate ([`gate`]) is the SOLE judge of correctness — the LLM only
//! proposes; memory + prompts only inform.

pub mod apply;
pub mod compress;
pub mod gate;
pub mod recall;
pub mod steps;

pub use apply::apply_change_set;
pub use gate::{GateOutcome, GateRunner};
pub use steps::{run, RunConfig, RunOutcome, StepLog};

/// What one phase/step call produced (the LLM's output for that step).
#[derive(Debug, Clone)]
pub struct PhaseResult {
    pub phase: &'static str,
    pub prompt: String,
    pub output: String,
    pub had_recalled_context: bool,
}
