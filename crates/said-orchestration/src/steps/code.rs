//! STEP 3 — CODE.
//!
//! | Claude does | .said provides |
//! |-------------|----------------|
//! | surgical edits, conventions, "don't over-engineer" | the CODE prompt + recalled verified patterns |
//!
//! Produces the change-set (the edits to apply). The pipeline's caller-supplied
//! `apply` closure turns this output into real, anchored `said edit` ops on disk
//! — never a whole-file rewrite. This step does NOT claim success; STEP 4 (the
//! gate) is the judge.

use crate::steps::run_phase;
use crate::PhaseResult;
use said_llm::LlmProvider;
use said_prompts::coding::Phase;

pub async fn run(
    brain: &mut sca_core::said_file::SaidFile,
    provider: &dyn LlmProvider,
    task: &str,
) -> Result<PhaseResult, String> {
    run_phase(brain, provider, Phase::Code, task, None).await
}
