//! Tool inventory — descriptive, mechanical, no behavioural rules.
//!
//! Anthropic's pattern: tool descriptions describe what the tool DOES,
//! not how often or in what pattern to use it. Behavioural rules (when
//! to prefer X over Y, parallel calls, etc.) live elsewhere — usually
//! in the role-specific overlay.

pub const TOOLS_READ: &str = "\
TOOLS — READ:
You have direct read access to the .said engine. Use these freely.
  • `library_overview`  — call FIRST when you don't know what's loaded
  • `discover`          — overview of one file (sources, pillars, tags)
  • `ask_fused`         — preferred 3-engine retrieval for natural questions
  • `recall_episodic`   — search THIS conversation's prior turns (meta-questions)
  • `semantic_search`   — pure embedding-similarity retrieval
  • `grep`              — exact-substring matches
  • `sym`               — exact symbol/identifier lookup
  • `read`              — full content of one memory by doc_id
  • `lineage`           — version history of one memory
  • `list_memories`     — paginated directory of one file
  • `list_tombstones`   — recycle-bin view (with optional `like` filter)
  • `stats`, `sources`  — file-level metadata
  • `salience`          — score text for save-worthiness BEFORE remember";

pub const TOOLS_CODING: &str = "\
TOOLS — CODING (when working on a project with code in the brain):
  • `code_chunks`       — list AST chunks (functions/classes/SQL procs) in a file
  • `find_callers`      — who calls a symbol (uses cached LSP refs when present)
  • `since_last_session`— what's changed since the most recent session_end";

pub const TOOLS_WRITE: &str = "\
TOOLS — WRITE (mutate the brain; user is asked to confirm):
ONLY use write tools when the user EXPLICITLY asks you to change
the brain. Don't write speculatively. The user must click 'Save .said'
afterwards to persist changes — file on disk is unchanged until they do.
  • `remember`          — add a new memory
  • `edit`               — replace content of a memory (creates a new version)
  • `delete`            — soft-delete (recoverable via `restore`)
  • `restore`           — undo a soft-delete
  • `compact`           — drop tombstones + reclaim bytes
  • `checkout`          — time-travel: restore an old version as new HEAD
  • `legal_hold_add`    — pin a doc_id under a case_id (retention-safe)
  • `legal_hold_release`— lift a hold
  • `dream`             — run a brain consolidation cycle";
