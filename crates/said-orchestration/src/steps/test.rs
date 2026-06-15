//! STEP 4 — TEST (the gate).
//!
//! | Claude does | .said provides |
//! |-------------|----------------|
//! | run build/test, gate | the TEST prompt + known commands (the iteration "Workflow") |
//!
//! This step is NOT an LLM call — it runs the project's real build/test commands
//! and reports green/red. It is the SOLE judge of correctness in the whole loop.
//! The TEST prompt (in said-prompts) is what an LLM uses to NAME the commands;
//! actually running them is deterministic and lives here.

use crate::gate::{GateOutcome, GateRunner};

pub fn run(gate: &GateRunner) -> GateOutcome {
    gate.run()
}
