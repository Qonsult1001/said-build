//! Coding-memory templates — the "prompt engine" half of .said's coding memory.
//!
//! .said stores not just verified fixes but the WHOLE coding iteration, mirroring
//! Claude Code's session-memory mechanism (reverse-engineered): a structured note
//! the client LLM authors, which .said stores and later re-injects so the LLM
//! continues the story instead of starting from scratch. .said is the memory +
//! prompt engine; the external LLM is the author/reasoner.
//!
//! These templates are DEFAULTS. They are intended to be overridable (a future
//! `said fix-template set` / config file) so the prompting is editable in .said,
//! per the design — not hardcoded forever.

/// The 10-section coding-iteration template, adapted from Claude Code's
/// `DEFAULT_SESSION_MEMORY_TEMPLATE` for a single verified coding iteration.
/// Each section has a header and an italic instruction describing what belongs.
pub const CODING_ITERATION_TEMPLATE: &str = r#"# Title
_A short, distinctive 5-10 word title for this coding iteration. Info-dense, no filler._

# Current State
_What is the resulting state after this iteration? What is done; what (if anything) remains._

# Task
_What did the user ask to build/fix? Design decisions and explanatory context._

# Files and Functions
_The important files/symbols touched: in short, what they contain and why they matter._

# Workflow
_What commands build/test/run this? In what order? How to read their output if not obvious._

# Errors and Corrections
_Errors hit and how they were fixed. What the user corrected. Approaches that FAILED and should not be retried._

# Codebase and System Documentation
_The important system components involved and how they fit together._

# Learnings
_What worked well, what to avoid. Do not duplicate items from other sections._

# Key Results
_The concrete verified result (the change-set / output that built+passed). Exact where it matters._

# Worklog
_Step by step, terse: what was attempted and done._
"#;

/// The extraction prompt a client LLM uses to AUTHOR a coding-iteration note from
/// a conversation, modelled on Claude Code's session-memory update prompt. The
/// caller substitutes `{{template}}` (and may prepend the conversation). The
/// authored note is then stored verbatim via `learn-fix --note-file`.
pub const CODING_ITERATION_PROMPT: &str = r#"Based on the coding work above, write a coding-iteration memory note that a future LLM (or you, later) can read to continue or reuse this work WITHOUT starting from scratch.

Fill in the template below. Keep the exact section headers and the italic _descriptions_; write info-dense content under each. Include specifics: file paths, symbol names, exact error messages, commands, and the verified change. Record failed approaches under "Errors and Corrections" so they are never retried. Only record work that actually built and passed — this memory is ground truth.

Output ONLY the filled-in note, nothing else.

{{template}}
"#;

/// Section headers in canonical order — used to validate/normalize an authored
/// note and to know which sections exist.
pub const CODING_ITERATION_SECTIONS: &[&str] = &[
    "Title",
    "Current State",
    "Task",
    "Files and Functions",
    "Workflow",
    "Errors and Corrections",
    "Codebase and System Documentation",
    "Learnings",
    "Key Results",
    "Worklog",
];

// ===========================================================================
// PHASE-PROMPT LIBRARY — the "playbook" half of .said's orchestrator.
//
// Standard prompts for each lifecycle phase, distilled faithfully from Claude
// Code's reverse-engineered system prompt (Doing tasks / Using tools /
// Executing actions with care / plan mode / verification). .said fills the
// `{{task}}` and `{{context}}` (recalled project memory + prior attempts) slots
// and sends the result to ANY external LLM — that's how a non-frontier model
// runs the Claude-Code workflow. These are DEFAULTS; intended to be overridable
// per the design ("prompting must be editable in .said").
//
// Boundary (load-bearing): these prompts make the LLM PROPOSE. The build/test
// gate VERIFIES. Never let a phase prompt declare work "done" — only the gate.
// ===========================================================================

/// A lifecycle phase. Stable ordering: plan → design → code → test → repair, with
/// learn applied on green (the iteration note, see CODING_ITERATION_PROMPT).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Plan,
    Design,
    Code,
    Test,
    Repair,
}

impl Phase {
    /// Parse a phase from a CLI/MCP string (case-insensitive).
    pub fn parse(s: &str) -> Option<Phase> {
        match s.trim().to_lowercase().as_str() {
            "plan" => Some(Phase::Plan),
            "design" | "structure" => Some(Phase::Design),
            "code" | "implement" => Some(Phase::Code),
            "test" | "gate" => Some(Phase::Test),
            "repair" | "fix" => Some(Phase::Repair),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Phase::Plan => "plan",
            Phase::Design => "design",
            Phase::Code => "code",
            Phase::Test => "test",
            Phase::Repair => "repair",
        }
    }

    /// The default standard prompt for this phase. Slots: `{{task}}` (the user's
    /// request) and `{{context}}` (recalled project memory + prior attempts).
    pub fn default_prompt(&self) -> &'static str {
        match self {
            Phase::Plan => PHASE_PLAN_PROMPT,
            Phase::Design => PHASE_DESIGN_PROMPT,
            Phase::Code => PHASE_CODE_PROMPT,
            Phase::Test => PHASE_TEST_PROMPT,
            Phase::Repair => PHASE_REPAIR_PROMPT,
        }
    }
}

/// Fill a phase prompt's `{{task}}` / `{{context}}` slots. Empty context renders
/// as an explicit "(no prior context)" so the LLM knows it's starting fresh.
pub fn fill_phase_prompt(template: &str, task: &str, context: &str) -> String {
    let ctx = if context.trim().is_empty() {
        "(no prior project memory for this task — starting fresh)".to_string()
    } else {
        context.trim().to_string()
    };
    template
        .replace("{{task}}", task.trim())
        .replace("{{context}}", &ctx)
}

pub const PHASE_PLAN_PROMPT: &str = r#"You are planning a coding task. This is READ-ONLY exploration — DO NOT write or edit any files yet.

TASK:
{{task}}

PROJECT MEMORY (what .said already knows — prior decisions, conventions, files, errors to avoid):
{{context}}

Produce a concrete plan:
1. What you need to read/understand first (specific files/symbols).
2. The approach, in steps. Prefer the simplest approach that fully solves the task.
3. Any design decisions and why.
4. The exact files/symbols you expect to change.
Do NOT propose changes to code you haven't read. Do NOT add features, refactors, or "improvements" beyond what the task asks. Output the plan only."#;

pub const PHASE_DESIGN_PROMPT: &str = r#"You are deciding the structure/design for a coding task, consistent with THIS project's existing conventions.

TASK:
{{task}}

PROJECT MEMORY (conventions, architecture, how components fit — follow these exactly; do not invent new conventions):
{{context}}

Produce:
1. Where the change belongs (files, modules, symbols) and why.
2. The shape of the change consistent with the project's existing patterns.
3. What NOT to touch.
Match the existing code's idioms, naming, and structure. If the project has no convention for something, choose the minimal one and note it. Output the design only — no code yet."#;

pub const PHASE_CODE_PROMPT: &str = r#"You are implementing a coding task with SURGICAL, anchored edits — never whole-file rewrites.

TASK:
{{task}}

PROJECT MEMORY (the plan/design, conventions, verified patterns from past iterations, and errors to AVOID):
{{context}}

Rules (non-negotiable):
- Read the relevant code before editing it.
- Make the smallest change that fully solves the task. Do NOT add unrequested features, error handling for impossible cases, or speculative abstractions.
- Match the surrounding code's style and naming.
- Default to no comments; add one only where the WHY is non-obvious.
- Apply changes as anchored edits (insert/replace at a named symbol or exact-text anchor).
Output the concrete change-set (the edits to apply). Do not claim it works — the build/test gate verifies that next."#;

pub const PHASE_TEST_PROMPT: &str = r#"You are verifying a coding change. The build/test gate is the sole judge of correctness.

TASK:
{{task}}

PROJECT MEMORY (the project's build/test commands and how to read their output):
{{context}}

State exactly:
1. The build command to run.
2. The test command to run.
3. What "green" looks like (success criteria) and how to interpret failure output.
Run nothing yourself in this step — name the exact commands the gate should run. Report outcomes faithfully; never assume success."#;

pub const PHASE_REPAIR_PROMPT: &str = r#"The build/test gate FAILED. Fix the cause. Do not retry approaches that already failed.

TASK:
{{task}}

GATE FAILURE + PROJECT MEMORY (the error output, prior attempts, and known errors-to-avoid from past iterations):
{{context}}

Diagnose the ROOT CAUSE from the error before changing anything. Then produce a surgical change-set that addresses it. If an approach already failed (see the memory above), do NOT repeat it — try a different one. Output the corrective change-set only; the gate re-verifies."#;
