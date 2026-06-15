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
