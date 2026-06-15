//! STEP 5 — REPAIR (the loop).
//!
//! | Claude does | .said provides |
//! |-------------|----------------|
//! | feed error back, fix, retry until green | the REPAIR prompt + stored "Errors & Corrections" |
//!
//! Runs only when STEP 4 is red. The gate's failure output is passed as `extra`
//! and combined with the recalled iteration's known errors-to-avoid, so failed
//! approaches are not retried. Produces a corrective change-set; the pipeline
//! applies it and re-runs the gate. Bounded by `RunConfig.max_attempts` — the
//! loop NEVER merges red.

use crate::steps::run_phase;
use crate::PhaseResult;
use said_llm::LlmProvider;
use said_prompts::coding::Phase;

pub async fn run(
    brain: &mut sca_core::said_file::SaidFile,
    provider: &dyn LlmProvider,
    task: &str,
    gate_error: &str,
) -> Result<PhaseResult, String> {
    run_phase(brain, provider, Phase::Repair, task, Some(gate_error)).await
}
