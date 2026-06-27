//! Agent-steering guidance — the prompt text that tells a coding agent (Claude Code) WHEN/WHAT to use
//! `.said`. Lives here in the global prompt library (not hardcoded in the core `steering.rs`
//! mechanism) so all agent-facing language matches the rest of `said-prompts`: imperative voice,
//! ALL-CAPS section headers, `•` bullets, and — most important — the honesty ethos the whole library
//! enforces (`retrieval::NEVER_DO`, `core::SYSTEM_PRINCIPLES`): `.said` RETURNS what it found, the
//! agent decides; never claim it found something it didn't.
//!
//! Style follows `tools.rs`'s Anthropic rule: these describe WHAT `.said` does and WHEN it beats
//! grep/read — they do NOT command the agent ("always do X"); the hook and the agent's own judgement
//! handle the rest.
//!
//! Two surfaces consume these (see docs/said-structure/16-agent-steering.md):
//!   * MCP `instructions` field — loaded when `.said` connects, removed when it disconnects.
//!   * the bundled `said` SKILL written by `said setup` — guidance, NEVER CLAUDE.md.
//! The PreToolUse HOOK (core `steering::decide`) injects the actual recall RESULTS at search time; the
//! text below tells the agent how to think about `.said`, not what any single recall returned.

/// The MCP `instructions` string the said-mcp server returns at connect. Concise (< 2KB — Claude Code
/// truncates server instructions). Loaded on connect, removed on disconnect — no git trace.
pub const MCP_INSTRUCTIONS: &str = "\
.SAID — PROJECT MEMORY + CODE INDEX

Default: before you grep or open files to locate something, query .said first with `ask`. It finds code \
by meaning — semantic + symbol + call-graph in one query — and recalls context from earlier sessions, \
for far fewer tokens than reading files blind.

Query (`ask`) when you need to:
  • find code by what it DOES, not its exact name;
  • trace a bug from a symptom to the function that causes it;
  • recall a past fix, a decision, or anything concluded in an earlier session.
It returns real code and stored facts, never invented. Act on what it returns; hand the symbols to your \
LSP for type-precise references. .said points you at the right place to look — it does not replace your \
editor or LSP.

Write the moment you conclude something a future session would pay to know — not every step. You decide \
what is worth keeping, like an engineer's notes:
  • learned about the code (a verified fix, a root cause, a non-obvious invariant, an architectural \
decision + its why) → `learn_fix`: problem + why + change-set, stored structured (not a one-line label).
  • a user fact, or the user said \"remember …\" → `remember`: one distilled fact, used as an end user \
would (\"remember to revisit this fn\").
  • closing out work → `journal`: wanted, decided, built, blockers, next.
Distil, don't dump. Skip the obvious and the unverified. .said dedupes. Rule of thumb: learned about the \
CODE → learn_fix; the USER asked to keep something → remember.";

/// The bundled `said` SKILL body (`.claude/skills/said/SKILL.md`) written by `said setup`. Bootstrap
/// guidance lives HERE, never in CLAUDE.md — so removing `.said` leaves no committed trace. The leading
/// `---` blocks are SKILL.md YAML frontmatter; bullet/blank lines keep their own newlines (only long
/// prose lines use `\\` continuation, which does NOT survive into the written file).
pub const SKILL_BODY: &str = "\
---
name: said
description: Use .said memory to locate code by meaning and recall project context before searching the codebase.
---

# Using .said

This project has a `.said` brain — portable memory + a code index.

Default: before you grep or open files to locate something, query `.said` first with `ask`. It finds \
code by meaning — semantic + symbol + call-graph in one query — and recalls context from earlier \
sessions, for far fewer tokens than reading files blind. (A hook also injects the top hit on each \
prompt; `ask` is how you query it yourself.)

Query (`ask`) when you need to:
  • find code by what it DOES, not its exact name;
  • trace a bug from a symptom to the function that causes it;
  • recall a past fix, a decision, or anything concluded in an earlier session.
It returns real code and stored facts, never invented. Act on what it returns; hand the symbols to \
your LSP for type-precise references. `.said` points you at the right place to look — it does not \
replace your editor or LSP.

Write the moment you conclude something a future session would pay to know — not every step. You \
decide what is worth keeping, like an engineer's notes:
  • learned about the code (a verified fix, a root cause, a non-obvious invariant, an architectural \
decision + its why) → `learn_fix`: problem + why + change-set, stored structured (not a one-line \
label — a weak note gets out-ranked by the source it summarizes).
  • a user fact, or the user said \"remember …\" → `remember`: one distilled fact, used as an end user \
would (\"remember to revisit this fn\", \"remember my mom's birthday\").
  • closing out work → `journal`: wanted, decided, built, blockers, next.

Distil, don't dump. Skip the obvious and the unverified. `.said` dedupes. Rule of thumb: did I learn \
something about the CODE? `learn_fix`. Did the USER ask me to \
keep something (coding or not)? remember.";

/// One-line summary used by `said setup` output / `said plugin list`.
pub const STEERING_SUMMARY: &str =
    "Steers the coding agent to query .said before grepping (PreToolUse hook injects recall; removal-safe).";

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the said-prompts test pattern (lib.rs): assert exact key phrases are present, the
    /// const is substantive, and house-style + honesty-ethos markers appear.
    #[test]
    fn mcp_instructions_aligned_and_bounded() {
        // Substantive, and under Claude Code's 2KB server-instruction truncation.
        assert!(MCP_INSTRUCTIONS.len() > 200, "instructions must be substantive");
        assert!(MCP_INSTRUCTIONS.len() < 2048, "must stay under the 2KB MCP-instruction limit");
        assert!(MCP_INSTRUCTIONS.contains('•'));
        // FUNCTIONAL: the QUERY tool must be named (the read half can't bind to no tool — the bug we fixed).
        assert!(MCP_INSTRUCTIONS.contains("`ask`"), "must name the query tool `ask`");
        // The default/interception point leads (not buried last).
        assert!(MCP_INSTRUCTIONS.contains("Default: before you grep"), "default-first interception line");
        // All three write verbs named, split by domain.
        assert!(MCP_INSTRUCTIONS.contains("`learn_fix`") && MCP_INSTRUCTIONS.contains("`remember`")
            && MCP_INSTRUCTIONS.contains("`journal`"), "all three write verbs named");
        // Honesty ethos: returns real facts, never invented.
        assert!(MCP_INSTRUCTIONS.contains("never invented"));
        // Division of labor: defers type-precise work to the LSP.
        assert!(MCP_INSTRUCTIONS.contains("LSP"));
    }

    #[test]
    fn skill_body_is_valid_skill_md() {
        // Proper SKILL.md frontmatter (name + description between --- fences).
        assert!(SKILL_BODY.starts_with("---\nname: said\n"), "must open with SKILL.md frontmatter");
        assert_eq!(SKILL_BODY.matches("---").count(), 2, "exactly one frontmatter block");
        assert!(SKILL_BODY.contains("# Using .said"));
        // The query tool is named (the read-half-binds-to-no-tool bug fix).
        assert!(SKILL_BODY.contains("`ask`"), "skill must name the query tool `ask`");
        // Honesty ethos again — the skill must not over-claim.
        assert!(SKILL_BODY.contains("never invented"));
        // CRITICAL: no literal backslash may leak into the written file (Rust `\\`-continuations
        // consume the backslash + leading whitespace; if one survived, SKILL.md would be malformed).
        assert!(!SKILL_BODY.contains('\\'), "no literal backslash may reach the written SKILL.md");
    }

    #[test]
    fn summary_is_one_line_and_substantive() {
        assert!(!STEERING_SUMMARY.contains('\n'), "summary is a single line");
        assert!(STEERING_SUMMARY.contains(".said"));
        assert!(STEERING_SUMMARY.contains("removal-safe"));
    }
}
