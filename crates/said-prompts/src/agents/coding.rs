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

pub const PLAN: &str = r#"You are planning a software engineering task. This is read-only: do NOT write or edit any files in this step.

The task:
{{task}}

Project memory below is verified context from prior work — prior decisions, conventions, files, and errors to avoid. IMPORTANT: when it applies, follow it exactly; do not contradict it.
{{context}}

Produce a concise plan:
1. What to read/understand first — specific files and symbols.
2. The approach as concrete steps. Prefer the simplest approach that fully solves the task.
3. Key decisions and why.
4. The exact files and symbols you expect to change.

IMPORTANT: Do not propose changes to code you have not read. Do NOT add features, refactors, or "improvements" beyond what the task asks. Output only the plan."#;

pub const DESIGN: &str = r#"You are deciding the structure of a change, consistent with this project's existing conventions. This step is still read-only — no code yet.

The task:
{{task}}

Project memory below is the verified architecture and conventions. IMPORTANT: follow these exactly; do not invent new conventions where one already exists.
{{context}}

Produce:
1. Where the change belongs — files, modules, symbols — and why.
2. The shape of the change, consistent with the project's existing patterns.
3. What NOT to touch.

Match the existing code's idioms, naming, and structure. The right amount of complexity is what the task actually requires — no speculative abstractions, but no half-finished implementations either. If the project has no convention for something, choose the minimal one and say so. Output only the design."#;

/// System prompt for CODE/REPAIR phases (they emit a change-set, not prose).
pub const SYSTEM_CHANGESET: &str =
    "You are a software engineering assistant driven by .said memory. The instructions \
     in the message are authoritative — follow them exactly. Output ONLY the change-set \
     JSON they specify: a top-level {\"edits\":[...]} object. No prose, no markdown, no \
     code fences.";

/// System prompt for prose phases (plan/design/test): wrap the answer.
pub const SYSTEM_PROSE: &str =
    "You are a software engineering assistant driven by .said memory. The instructions \
     in the message are authoritative — follow them exactly. Be concise and go straight \
     to the point. Return your answer as JSON: {\"output\": \"<your full answer>\"}.";

/// System prompt for the LEARN step (authoring the iteration note).
pub const SYSTEM_LEARN: &str =
    "You write concise, info-dense coding-iteration memory notes for future reuse. \
     Output ONLY the filled-in template as JSON: {\"note\": \"<the full note>\"}.";

/// Preamble that frames a recalled VERIFIED iteration as a strong prior the model
/// REUSES (not re-derives) — Claude's lesson from thousands of interactions: a
/// gate-verified solution is known-good; don't paraphrase its core logic into an
/// untested variant. The `{{directive}}` slot is filled by [`fill_memory_injection`]
/// and scales with match confidence: a near-identical shape means keep the verified
/// structure intact and change only names/paths; a looser match means adapt the
/// approach. The gate is still the backstop.
pub const MEMORY_INJECTION: &str =
    "# Verified solution from memory (match {{score}})\n\
     A previous, gate-verified solution to a task of THIS shape — it built and passed. {{directive}}\n\
     ## What was learned, and the recipe\n{{note}}\n## Reference verified solution\n{{edits}}\n";

/// High-confidence: essentially the same task. Keep the verified structure; only
/// re-target it to this file. Re-deriving the core algorithm risks an inconsistent,
/// untested variant — the exact failure the gate keeps catching.
const INJECT_DIRECTIVE_STRONG: &str =
    "IMPORTANT: this is essentially the same task, just in a different file. REUSE the verified \
     solution's structure and core logic EXACTLY — keep the same data structures, helper functions, \
     and control flow. Change ONLY what must change for this codebase: file paths, anchors, and \
     surrounding names. Do NOT re-derive or \"improve\" the algorithm: its pieces must stay internally \
     consistent (e.g. how items are added must match how they are removed/evicted), and a paraphrase \
     that flips one half while keeping the other silently breaks it. When in doubt, follow the reference.";

/// Lower-confidence: related but not identical. Adapt the approach.
const INJECT_DIRECTIVE_SOFT: &str =
    "This is a related, known-good APPROACH. Adapt it to the current task: reuse the verified logic \
     and structure where they apply rather than re-deriving from scratch, and change names, anchors, \
     and surrounding code to fit this codebase. Keep any internally-consistent logic consistent.";

/// Confidence at/above which we tell the model to reuse the verified structure
/// verbatim (only re-target it). Below this, the softer adapt-the-approach framing.
pub const INJECT_STRONG_MIN: f32 = 0.85;

/// SELECTIVE-injection caps (Claude's documented rule: "injecting everything would
/// overflow the context window"). A bloated injection — note that embeds the full
/// solution PLUS the full change-set — measurably DERAILS a small model (gpt-oss-20b
/// regressed an already-solvable task until the injection was made lean). Bound both:
/// the note teaches (approach + gotchas), the reference shows the shape, neither floods.
const INJECT_NOTE_MAX: usize = 2400;   // ~600 tokens of learning — the gotchas, not the codebase
const INJECT_EDITS_MAX: usize = 2000;  // ~500 tokens of reference — the shape, not a second full paste

/// Keep the head of `s` up to `max` chars on a line boundary; append a marker if cut.
fn cap(s: &str, max: usize) -> String {
    if s.len() <= max { return s.to_string(); }
    let mut kept = String::new();
    for line in s.lines() {
        if kept.len() + line.len() + 1 > max { break; }
        kept.push_str(line);
        kept.push('\n');
    }
    kept.push_str("… (truncated — adapt the approach above; the gate verifies)\n");
    kept
}

/// Fill the memory-injection preamble, scaling the directive by match confidence
/// (Claude's "strong prior vs. weak prior" treatment) and BOUNDING the injected
/// content so it teaches without overflowing a small model.
pub fn fill_memory_injection(score: f32, note: &str, edits: &str) -> String {
    let directive = if score >= INJECT_STRONG_MIN {
        INJECT_DIRECTIVE_STRONG
    } else {
        INJECT_DIRECTIVE_SOFT
    };
    MEMORY_INJECTION
        .replace("{{score}}", &format!("{:.2}", score))
        .replace("{{directive}}", directive)
        .replace("{{note}}", &cap(note, INJECT_NOTE_MAX))
        .replace("{{edits}}", &cap(edits, INJECT_EDITS_MAX))
}

pub const CODE: &str = r#"You are implementing a coding task. The current source is shown in the context below — IMPORTANT: do not propose changes to code you have not read. Anchor every edit on lines that actually appear in it.

The task:
{{task}}

Project memory below is the plan/design, this project's conventions, verified patterns from past iterations, and errors to AVOID. Follow it where it applies.
{{context}}

How to edit:
- Make the smallest change that fully solves the task. Don't add features, refactor code, or make "improvements" beyond what was asked. A bug fix doesn't need surrounding code cleaned up.
- Don't add error handling, fallbacks, or validation for scenarios that can't happen. Match the surrounding code's style and naming.
- Default to writing no comments. Add one only where the WHY is non-obvious.

When editing text from the source shown above, preserve the exact indentation (tabs/spaces) as it appears AFTER the line number prefix. The line number prefix format is: spaces + line number + tab. Everything after that is the actual file content to match. Never include any part of the line number prefix in the anchor or content.

The anchor must match exactly: an edit FAILS if its anchor is not found verbatim in the file, or is not unique. Either copy a larger, unique anchor or use write-file.

Output the change-set as JSON ONLY — a top-level {"edits":[ <edit>, ... ]} object, no prose, no markdown. Each edit picks the mode that fits:

A) write-file — for implementing/rewriting a whole function or class, or changing most of a small file. This is the most reliable mode; prefer it when the change is large. The content REPLACES the entire file, so it MUST be the COMPLETE file top to bottom (all existing code you keep, plus your change) and must compile:
   {"file":"<path>","mode":"write-file","content":"<the complete new file content>"}

B) insert-after-text / insert-before-text — for a small insertion into a file you otherwise keep intact:
   {"file":"<path>","mode":"insert-after-text","anchor":"<an EXACT existing line, copied verbatim, that ends a statement (ends in ; } or {)>","content":"<the complete statement(s) to insert>"}
   NEVER anchor on the first line of a multi-line statement — it splits the statement and breaks the file.

C) replace-text — for swapping one exact snippet. The anchor must be the FULL exact substring to remove (single- or multi-line), copied verbatim:
   {"file":"<path>","mode":"replace-text","anchor":"<the exact full text to replace>","content":"<the replacement>"}
   NEVER use a partial or signature-only line as a replace-text anchor — it replaces only that text and leaves the rest orphaned, corrupting the file.

Choosing: stubbed function/class, or logic spanning several methods → write-file. One endpoint or line added → insert. One literal or expression swapped → replace-text. In every mode, "content" must be complete and compilable.

Do not claim the change works — the build/test gate verifies that next."#;

pub const TEST: &str = r#"You are verifying a coding change. Before reporting a task complete, it has to actually work: the build/test gate runs the commands and is the sole judge. Your job here is to name the exact commands the gate should run — run nothing yourself.

The task:
{{task}}

Project memory below has this project's build/test commands and how to read their output.
{{context}}

State exactly:
1. The build command.
2. The test command.
3. What "green" looks like — the success criteria — and how to read a failure.

Report outcomes faithfully: never assume success, and never imply a check passed that has not run."#;

pub const REPAIR: &str = r#"The build/test gate FAILED. Fix the cause.

The task:
{{task}}

The gate failure, prior attempts, and known errors-to-avoid are below. The current source is shown too — anchor edits on lines that actually appear in it.
{{context}}

IMPORTANT: diagnose the root cause from the error before changing anything — read the error, check your assumptions. If an approach already failed (see the context above), do NOT repeat it; try a focused different fix. Make the smallest change that addresses the cause — don't refactor adjacent code.

IMPORTANT — choosing the edit mode for a repair: a partial anchor that does not match exactly, or that only replaces part of a block, leaves the rest of the file orphaned and turns one bug into a broken file. So when the fix touches logic inside a function/method, or the file is small, prefer "write-file" and re-emit the COMPLETE corrected file (the full current source with your fix applied). Reserve anchored insert/replace for a single, unambiguous, exact-substring change in a large file you are otherwise leaving intact.

When editing text from the source shown above, the line number prefix format is: spaces + line number + tab. Everything after that is the actual file content to match. Never include any part of the line number prefix in the anchor or content. An anchored edit FAILS if its anchor is not found verbatim or is not unique.

Output the corrective change-set as JSON ONLY, same shape and modes as the code step — a top-level {"edits":[...]} object, no prose:
{"edits":[{"file":"<path>","mode":"<write-file|insert-after-text|insert-before-text|replace-text>","anchor":"<exact existing substring (omit for write-file)>","content":"<new code>"}]}

The gate re-verifies — do not claim it works."#;

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
_The verified OUTCOME in 1-3 lines: what passed, and the single most important
decision/invariant that made it pass. Do NOT paste the full implementation here — the
verified change-set is stored separately as the reference. Keep this a summary, not code._

# Worklog
_Step by step, terse: what was attempted and done._
"#;

/// LEARN extraction prompt — asks the LLM to AUTHOR the iteration note from the
/// just-completed (and gate-verified) work, exactly like Claude Code extracts
/// session memory. The caller substitutes `{{template}}` and `{{transcript}}`
/// (task + plan + code + gate result). Output is ONLY the filled note.
pub const LEARN: &str = r#"You are writing a coding-iteration memory note for a change that ALREADY built and passed the gate. A future LLM — or you, later — reads this to continue or reuse the work without starting from scratch, so write it for that reader.

The completed, verified work:
{{transcript}}

Fill in the template below. Keep the exact section headers and the italic _descriptions_. Under each, write info-dense content: specific file paths and symbol names, the exact error messages hit and how they were fixed, the commands, and the verified change-set. IMPORTANT: put every approach that FAILED under "Errors and Corrections" so it is never retried. Be terse but complete — every line must earn its place; no filler, no restating the obvious.

Report faithfully: record what actually happened, not an idealized version. Output ONLY the filled-in note.

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

    #[test]
    fn injection_scales_with_confidence() {
        // High confidence -> reuse the verified structure verbatim.
        let strong = fill_memory_injection(0.95, "note", "edits");
        assert!(strong.contains("essentially the same task"));
        assert!(strong.contains("REUSE the verified solution's structure"));
        assert!(!strong.contains("{{directive}}"));
        assert!(strong.contains("0.95"));
        // Lower confidence -> softer adapt-the-approach framing.
        let soft = fill_memory_injection(0.55, "note", "edits");
        assert!(soft.contains("related, known-good APPROACH"));
        assert!(!soft.contains("essentially the same task"));
    }

    #[test]
    fn injection_is_bounded() {
        // SELECTIVE injection (Claude's overflow rule): a huge note + huge edits
        // must be capped, not dumped whole — a bloated injection derails small models.
        let huge_note = "line of learning\n".repeat(1000);   // ~17 KB
        let huge_edits = "x".repeat(50_000);                  // 50 KB
        let out = fill_memory_injection(0.95, &huge_note, &huge_edits);
        assert!(out.len() < 8_000, "injection must be bounded, got {}", out.len());
        assert!(out.contains("truncated"), "should mark truncation");
    }
}
