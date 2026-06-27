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
.SAID — MEMORY + CODE INDEX:
.said is a portable memory and code-knowledge index for this project. Prefer it over blind grepping or \
reading whole files when you need to LOCATE something by meaning, or recall context from earlier work:
  • Find code by what it DOES (not its exact name) — semantic + symbol + call-graph in one query.
  • Locate a bug from a symptom — it points at the precise function, far cheaper than reading files.
  • Recall a past fix, a decision, or project knowledge — .said remembers across sessions.
.said RETURNS the relevant code/details — it never invents them; you act on what it returns and hand \
symbols to your language server (LSP) for type-precise references. It does not replace your editor or \
LSP; it is the fast, token-lean way to find the RIGHT place to look. When .said surfaces the answer, \
you can skip the grep.

RECORD WHAT YOU CONCLUDE (so the next session is cheaper):
.said is a BRAIN, not just an index — it remembers across sessions only if you write to it. Like a good \
engineer's notes, you decide what is worth keeping: save a learning the moment you CONCLUDE something \
that would be useful in a future session, not every step. Concretely:
  • CODE learnings (a verified fix, a bug's root cause, a non-obvious invariant, an architectural \
decision + its WHY) → `learn_fix` (problem + WHY + change-set), stored STRUCTURED not as a label.
  • USER / non-coding facts, or when the user says \"remember …\" → `remember`, used like an end user \
would (\"remember my mom's birthday\", \"remember to revisit this fn\"). One distilled fact.
  • wrapping up work / a checkpoint → `journal` (wanted, decided, built, blockers, next steps).
Rule of thumb: learned something about the CODE → learn_fix; the USER asked to keep something → remember. \
Distil don't dump; skip the obvious/unverified; .said dedupes.";

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

This project has a `.said` brain — a portable memory and code index. Before you grep the codebase or \
read files to find something, `.said` is queried automatically (a PreToolUse hook injects the relevant \
memory) and you can also call the `.said` MCP tools directly.

REACH FOR .said WHEN YOU NEED TO:
  • find code by INTENT (\"the function that retries failed webhooks\") — not just by exact name,
  • LOCATE a bug from a symptom — it points at the precise function, a fraction of the tokens of \
reading files,
  • recall a past fix, a decision, or domain knowledge from earlier sessions.

.said RETURNS the relevant code/details — it does not invent them, and it does not replace your editor \
or language server. You stay in control: act on what it returns, and hand symbols to your LSP for \
type-precise references. If .said already shows the answer, skip the grep.

RECORD WHAT YOU CONCLUDE (this is what makes .said a brain, not a static index):

.said remembers across sessions only if you write to it — so the next session starts where this one \
ended instead of rediscovering everything. Like an engineer's notebook, YOU decide what's worth keeping: \
save a learning the moment you CONCLUDE something that would help a future session, not every step.

PICK THE RIGHT VERB BY DOMAIN:
  • CODING LEARNINGS → learn_fix. A verified code fix (tests pass), a bug's ROOT CAUSE, a non-obvious \
code invariant, an architectural decision and its WHY. Pass the problem + the WHY in the learnings + the \
change-set. This is the code-knowledge store the orchestrator replays — store it STRUCTURED, never as a \
one-line label (a weak note gets out-ranked by the source it summarizes).
  • USER / NON-CODING FACTS, or when the USER SAYS \"remember …\" → remember. Use it exactly as an end \
user would: \"remember my mom's birthday\", \"remember to revisit this function later\", a preference, a \
project fact, anything outside code the user wants kept. One distilled fact.
  • WRAPPING UP a piece of work or a checkpoint (\"moving on to X\") → journal: what was wanted, decided, \
built, blockers, next steps.

Distil, don't dump — store the INVARIANT/decision, not the raw transcript. Skip the obvious and the \
unverified. .said dedupes, so re-recording a known fact is cheap; a clean structured learning beats a \
wall of text. Rule of thumb: did I learn something about the CODE? learn_fix. Did the USER ask me to \
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
        // House style: an ALL-CAPS header + `•` bullets (matches tools.rs / strategy.rs).
        assert!(MCP_INSTRUCTIONS.contains(".SAID — MEMORY + CODE INDEX:"));
        assert!(MCP_INSTRUCTIONS.contains('•'));
        // The three documented capabilities (find-by-intent / locate-bug / recall) are all named.
        assert!(MCP_INSTRUCTIONS.contains("Find code by what it DOES"));
        assert!(MCP_INSTRUCTIONS.contains("Locate a bug from a symptom"));
        assert!(MCP_INSTRUCTIONS.contains("Recall a past fix"));
        // Honesty ethos (aligns with retrieval::NEVER_DO / core::SYSTEM_PRINCIPLES): never invents.
        assert!(MCP_INSTRUCTIONS.contains("never invents"));
        // Division of labor: defers type-precise work to the LSP (matches docs/16-agent-steering).
        assert!(MCP_INSTRUCTIONS.contains("language server"));
    }

    #[test]
    fn skill_body_is_valid_skill_md() {
        // Proper SKILL.md frontmatter (name + description between --- fences).
        assert!(SKILL_BODY.starts_with("---\nname: said\n"), "must open with SKILL.md frontmatter");
        assert_eq!(SKILL_BODY.matches("---").count(), 2, "exactly one frontmatter block");
        assert!(SKILL_BODY.contains("# Using .said"));
        // Honesty ethos again — the skill must not over-claim.
        assert!(SKILL_BODY.contains("it does not invent them"));
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
