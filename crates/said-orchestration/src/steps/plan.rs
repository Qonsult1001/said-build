//! STEP 1 — PLAN.
//!
//! | Claude does | .said provides |
//! |-------------|----------------|
//! | explore read-only, design approach, make todos | the PLAN prompt + recalled prior plans/decisions |
//!
//! Read-only: this step never edits files. It produces the approach the later
//! steps follow. Recalled context = the most relevant verified iteration's story.

use crate::steps::run_phase;
use crate::PhaseResult;
use said_llm::LlmProvider;
use said_prompts::coding::Phase;

pub async fn run(
    brain: &mut sca_core::said_file::SaidFile,
    provider: &dyn LlmProvider,
    task: &str,
) -> Result<PhaseResult, String> {
    run_phase(brain, provider, Phase::Plan, task, None).await
}
