//! STEP 2 — DESIGN / STRUCTURE.
//!
//! | Claude does | .said provides |
//! |-------------|----------------|
//! | architecture, file layout conventions | the DESIGN prompt + the project's stored conventions |
//!
//! Decides WHERE the change belongs, consistent with this project's existing
//! patterns. Still no edits. Recalled context carries the project's conventions
//! so a new project gets standards and an old one keeps ITS own.

use crate::steps::run_phase;
use crate::PhaseResult;
use said_llm::LlmProvider;
use said_prompts::coding::Phase;

pub async fn run(
    brain: &mut sca_core::said_file::SaidFile,
    provider: &dyn LlmProvider,
    task: &str,
) -> Result<PhaseResult, String> {
    run_phase(brain, provider, Phase::Design, task, None).await
}
