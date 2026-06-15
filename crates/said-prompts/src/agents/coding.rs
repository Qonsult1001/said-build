//! Coding-lifecycle phase prompts — the Anthropic Plan/Code/Verification analogues
//! named as the planned extension in the crate root.
//!
//! These are the standard prompts that let `.said` drive ANY external LLM through
//! the Claude-Code workflow phase-by-phase. The orchestrator (a separate process,
//! `said-orchestration`, per the BYO-LLM rule) fetches a filled phase prompt, adds
//! recalled project memory as `context`, sends it to the user's configured model,
//! applies the result, and gates it. THESE PROMPTS NEVER DECLARE WORK DONE — the
//! build/test gate is the sole judge. Wording follows Claude Code's prompts.ts
//! (surgical edits, no over-engineering, read-before-edit, faithful reporting).

/// A lifecycle phase. plan → design → code → test → repair (learn happens on green
/// via the iteration memory, not a phase prompt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Plan,
    Design,
    Code,
    Test,
    Repair,
}

impl Phase {
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

    pub fn template(&self) -> &'static str {
        match self {
            Phase::Plan => PLAN,
            Phase::Design => DESIGN,
            Phase::Code => CODE,
            Phase::Test => TEST,
            Phase::Repair => REPAIR,
        }
    }
}

/// Runtime context for a coding phase prompt.
#[derive(Debug, Clone, Default)]
pub struct CodingContext {
    /// The coding task / problem in plain words.
    pub task: String,
    /// Recalled project memory (prior verified iteration's full story,
    /// conventions, errors-to-avoid). Empty → rendered as "starting fresh".
    pub context: String,
}

/// Build the filled phase prompt: standard template with `{{task}}`/`{{context}}`
/// substituted. The orchestrator supplies `ctx.context` from `.said` recall.
pub fn phase_prompt(phase: Phase, ctx: &CodingContext) -> String {
    let context = if ctx.context.trim().is_empty() {
        "(no prior project memory for this task — starting fresh)".to_string()
    } else {
        ctx.context.trim().to_string()
    };
    phase
        .template()
        .replace("{{task}}", ctx.task.trim())
        .replace("{{context}}", &context)
}

pub const PLAN: &str = r#"You are planning a coding task. This is READ-ONLY exploration — DO NOT write or edit any files yet.

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

pub const DESIGN: &str = r#"You are deciding the structure/design for a coding task, consistent with THIS project's existing conventions.

TASK:
{{task}}

PROJECT MEMORY (conventions, architecture, how components fit — follow these exactly; do not invent new conventions):
{{context}}

Produce:
1. Where the change belongs (files, modules, symbols) and why.
2. The shape of the change consistent with the project's existing patterns.
3. What NOT to touch.
Match the existing code's idioms, naming, and structure. If the project has no convention for something, choose the minimal one and note it. Output the design only — no code yet."#;

pub const CODE: &str = r#"You are implementing a coding task with SURGICAL, anchored edits — never whole-file rewrites.

TASK:
{{task}}

PROJECT MEMORY (the plan/design, conventions, verified patterns from past iterations, and errors to AVOID):
{{context}}

Rules (non-negotiable):
- Read the relevant code before editing it.
- Make the smallest change that fully solves the task. Do NOT add unrequested features, error handling for impossible cases, or speculative abstractions.
- Match the surrounding code's style and naming.
- Default to no comments; add one only where the WHY is non-obvious.

Output the change-set as JSON ONLY, in this exact shape:
{"edits":[{"file":"<path relative to repo root>","mode":"<insert-after-text|insert-before-text|replace-text>","anchor":"<an EXACT, UNIQUE existing line/substring in the file>","content":"<the new code>"}]}
- "anchor" must be copied VERBATIM from the current file so it resolves uniquely.
- "insert-after-text"/"insert-before-text": content is inserted relative to the anchor line.
- "replace-text": the first occurrence of "anchor" is replaced by "content".
Do not claim it works — the build/test gate verifies that next."#;

pub const TEST: &str = r#"You are verifying a coding change. The build/test gate is the sole judge of correctness.

TASK:
{{task}}

PROJECT MEMORY (the project's build/test commands and how to read their output):
{{context}}

State exactly:
1. The build command to run.
2. The test command to run.
3. What "green" looks like (success criteria) and how to interpret failure output.
Run nothing yourself in this step — name the exact commands the gate should run. Report outcomes faithfully; never assume success."#;

pub const REPAIR: &str = r#"The build/test gate FAILED. Fix the cause. Do not retry approaches that already failed.

TASK:
{{task}}

GATE FAILURE + PROJECT MEMORY (the error output, prior attempts, and known errors-to-avoid from past iterations):
{{context}}

Diagnose the ROOT CAUSE from the error before changing anything. Then produce a surgical change-set that addresses it. If an approach already failed (see the memory above), do NOT repeat it — try a different one.

Output the corrective change-set as JSON ONLY, same shape as the code step:
{"edits":[{"file":"<path>","mode":"<insert-after-text|insert-before-text|replace-text>","anchor":"<exact existing substring>","content":"<new code>"}]}
The gate re-verifies."#;

/// The 10-section coding-iteration template a verified iteration is stored as.
/// Modelled on Claude Code's `DEFAULT_SESSION_MEMORY_TEMPLATE`, adapted for a
/// single verified coding iteration. Section headers + italic instructions are
/// the structure; the LLM fills content beneath each.
pub const ITERATION_TEMPLATE: &str = r#"# Title
_A short, distinctive 5-10 word title for this iteration. Info-dense, no filler._

# Current State
_The resulting state after this iteration: what is done; what (if anything) remains._

# Task
_What was asked to build/fix, plus design decisions and context._

# Files and Functions
_The important files/symbols touched: what they contain and why they matter._

# Workflow
_The build/test commands and how to read their output if not obvious._

# Errors and Corrections
_Errors hit and how they were fixed. Approaches that FAILED and must not be retried._

# Codebase and System Documentation
_The system components involved and how they fit together._

# Learnings
_What worked, what to avoid. Do not duplicate other sections._

# Key Results
_The concrete verified change-set/output that built+passed. Exact where it matters._

# Worklog
_Step by step, terse: what was attempted and done._
"#;

/// LEARN extraction prompt — asks the LLM to AUTHOR the iteration note from the
/// just-completed (and gate-verified) work, exactly like Claude Code extracts
/// session memory. The caller substitutes `{{template}}` and `{{transcript}}`
/// (task + plan + code + gate result). Output is ONLY the filled note.
pub const LEARN: &str = r#"You are writing a coding-iteration MEMORY note for a verified change that ALREADY built and passed tests. A future LLM (or you, later) will read this to continue or reuse the work WITHOUT starting from scratch.

THE COMPLETED, VERIFIED WORK:
{{transcript}}

Fill in the template below. Keep the exact section headers and the italic _descriptions_. Write DETAILED, INFO-DENSE content under each: specific file paths, symbol names, exact error messages encountered and how they were fixed, the commands, and the verified change-set. Put failed approaches under "Errors and Corrections" so they are never retried. Be terse but complete — every line must earn its place; no filler, no restating the obvious.

Output ONLY the filled-in note.

{{template}}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_roundtrips() {
        assert_eq!(Phase::parse("plan"), Some(Phase::Plan));
        assert_eq!(Phase::parse("CODE"), Some(Phase::Code));
        assert_eq!(Phase::parse("structure"), Some(Phase::Design));
        assert_eq!(Phase::parse("nope"), None);
    }

    #[test]
    fn fills_slots() {
        let ctx = CodingContext { task: "add endpoint".into(), context: "prior fix".into() };
        let p = phase_prompt(Phase::Code, &ctx);
        assert!(p.contains("add endpoint"));
        assert!(p.contains("prior fix"));
        assert!(!p.contains("{{task}}"));
    }

    #[test]
    fn empty_context_renders_fresh() {
        let ctx = CodingContext { task: "x".into(), context: String::new() };
        let p = phase_prompt(Phase::Plan, &ctx);
        assert!(p.contains("starting fresh"));
    }
}
