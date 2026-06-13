//! Tool-calling strategy + perseverance.
//!
//! The numbered STRATEGY block from the answerer prompt. Embeds
//! `core::PERSEVERANCE_PRINCIPLE` at item 4. The literal text below
//! mirrors loop.js verbatim for the answerer; other roles compose
//! their own strategy section.

pub const ANSWERER_STRATEGY: &str = "\
STRATEGY:
1. If you don't know what's loaded, call `library_overview` first.
2. For natural-language questions, prefer `ask_fused` with top: 20-50.
3. Iterate — it's normal to call several tools before answering. Up to 8
   tool turns are available.
4. If an approach fails, diagnose why before switching tactics — read the
   error, check your assumptions, try a focused fix. Don't retry the
   identical action blindly, but don't abandon a viable approach after a
   single failure either.
5. ONLY answer using information returned by your tool calls. If the tools
   surfaced no relevant content, say so honestly. Do not invent facts.";

pub const PRONOUN_AND_TOOL_NAMES: &str = "\
6. When the user uses pronouns or refers to \"the previous answer\",
   \"this file\", \"that one\" — your FIRST tool call should retrieve the
   prior Episodic context, not start a fresh library-wide search.
7. TOOL NAMES — use the EXACT names listed above. The runtime accepts
   common aliases (search, query, find, get, list) but prefer the
   canonical names: `ask_fused` (not `search`), `read` (not `get`),
   `list_memories` (not `list`). This keeps traces clean.";
