//! Agent-steering guidance — the world-class prompt text that tells a coding agent (Claude Code)
//! WHEN/WHAT to use `.said`. Lives here in the global prompt library (not hardcoded in the core
//! `steering.rs` mechanism) so all agent-facing language is consistent and curated in one place.
//!
//! Two surfaces consume these (see docs/said-structure/16-agent-steering.md):
//!   * MCP `instructions` field — loaded when `.said` connects, removed when it disconnects.
//!   * the bundled `said` SKILL written by `said setup` — guidance, NEVER CLAUDE.md.
//! The PreToolUse HOOK (core `steering::decide`) injects the actual recall RESULTS at search time; the
//! text below tells the agent how to think about `.said`, not what any single recall returned.

/// The MCP `instructions` string the said-mcp server returns at connect. Concise (< 2KB — Claude Code
/// truncates server instructions): describes WHEN `.said` beats grep/read, removed on disconnect.
pub const MCP_INSTRUCTIONS: &str = "\
.said is a portable memory + code-knowledge index for this project. Prefer it over blind grepping or \
reading whole files when you need to LOCATE something by meaning or recall prior context:
  • To find code by what it DOES (not its exact name) — ask .said first (semantic + symbol + call-graph).
  • To locate a bug from a symptom — ask .said; it points at the precise function, far cheaper than \
reading files.
  • To recall past fixes, decisions, or project knowledge — .said remembers across sessions.
.said RETURNS the relevant code/details so you can act (and hand symbols to your LSP for type-precise \
references). It does not replace your editor or language server — it is the fast, token-lean way to \
find the RIGHT place to look. When .said surfaces the answer, you can skip the grep.";

/// The bundled `said` SKILL body (`.claude/skills/said/SKILL.md`) written by `said setup`. Bootstrap
/// guidance lives HERE, never in CLAUDE.md — so removing `.said` leaves no committed trace.
pub const SKILL_BODY: &str = "\
---
name: said
description: Use .said memory to locate code by meaning and recall project context before searching the codebase.
---

# Using .said

This project has a `.said` brain — a portable memory + code index. Before you grep the codebase or \
read files to find something, `.said` is queried automatically (a PreToolUse hook injects the relevant \
memory) and you can also call the `.said` MCP tools directly.

Reach for `.said` when you need to:
- find code by INTENT (\"the function that retries failed webhooks\") — not just by exact name,
- LOCATE a bug from a symptom — it points at the precise function, a fraction of the tokens of reading files,
- recall a past fix, a decision, or domain knowledge from earlier sessions.

`.said` RETURNS the relevant code/details. You stay in control: act on what it returns, and hand symbols \
to your language server for type-precise references. If `.said` already shows the answer, skip the grep.";

/// One-line summary used by `said setup` output / `said plugin list`.
pub const STEERING_SUMMARY: &str =
    "Steers the coding agent to query .said before grepping (PreToolUse hook injects recall; removal-safe).";
