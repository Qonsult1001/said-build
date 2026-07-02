//! The facade tools — the LLM's complete interface to .said brains.

use rust_mcp_sdk::macros::{mcp_tool, JsonSchema};
use rust_mcp_sdk::tool_box;

// ════════════════════════════════════════════════════════════════════════════
// Tool 1: SEARCH — the main retrieval verb
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "search",
    description = "Search the brain for information. Handles code, documents, SQL, passkeys, \
                   cross-document synthesis — the engine routes automatically using semantic \
                   search, keyword matching, and symbol lookup all fused together. \
                   Use deep=true for full narrative (all relevant chunks, no cap). \
                   Default returns top-10 (correct answer always in window). \
                   Optional pillar= narrows results to one CLS memory pillar: \
                   episodic (raw turns), semantic (distilled facts), procedural \
                   (action sequences), external (pointers), code (AST source), \
                   memory (legacy remember). Comma-separated for multiple.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct SearchTool {
    /// The question or search query
    pub query: String,
    /// Deep mode: return ALL relevant chunks for full narrative synthesis.
    /// Default false = top-10 (fast, focused answer).
    #[serde(default)]
    pub deep: Option<bool>,
    /// Optional pillar filter — comma-separated list of:
    /// `episodic`, `semantic`, `procedural`, `external`, `code`, `memory`.
    /// Example: `"semantic,code"`. Unknown names are ignored. When omitted,
    /// all pillars are searched (default, same as pre-Decision-2 behavior).
    #[serde(default)]
    pub pillar: Option<String>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 1b: ASK — the smart router (3-engine fusion)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "ask",
    description = "Find code or knowledge in this project's `.said` memory by MEANING. Call this \
                   BEFORE grepping or reading files when you need to LOCATE something: a function by \
                   what it DOES (not its exact name), the source of a bug from a symptom, or a past \
                   fix/decision — it points at the precise file + symbol far cheaper than reading the \
                   codebase, and finds matches grep can't (semantic + symbol + call-graph in one \
                   query). Returns ranked results [confidence][kind] doc_id + snippet; act on what it \
                   returns (and hand symbols to your LSP for type-precise references). It does not \
                   invent results — if it has nothing relevant it returns nothing, then grep normally. \
                   Runs the 3-engine fusion (Sym 1.00 / Grep 0.40-0.95 / SCA semantic 0.30-0.80); \
                   deep=true widens the pool. Same fusion as `said ask` on the CLI. \
                   EFFICIENCY: a single high-confidence hit ([0.95]+ or [symbol]) IS the answer — read \
                   that one frame with `get` and stop; do NOT re-ask the same question many ways or \
                   sweep the whole codebase to double-check. Re-query only if the top result is low \
                   confidence or clearly off-topic. One good `ask` should REPLACE a multi-step \
                   investigation, not kick one off.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct AskTool {
    /// The natural-language question
    pub query: String,
    /// Max results to return (default 10)
    #[serde(default)]
    pub top: Option<u32>,
    /// Deep mode: widen SCA fetch to 100, return all results above cutoff.
    #[serde(default)]
    pub deep: Option<bool>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 2: GET — read exact frame content
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "get",
    description = "Read the exact content of a specific frame by its doc_id. \
                   Use after 'search' to get the full text of a result. \
                   Returns the complete frame content word-for-word.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct GetTool {
    /// The doc_id to retrieve (e.g. "NL.pdf::page_0005" or "main.rs::compact_block_dict")
    pub doc_id: String,
}

#[mcp_tool(
    name = "list_concepts",
    description = "List the concepts memories are linked to ([[wikilink]] vocabulary), with \
                   how many memories carry each. ALWAYS call this BEFORE remembering a new \
                   memory so you reuse an existing concept (e.g. link 'heart', not a new \
                   'heart-health') — this keeps the concept graph converged so recall stays \
                   consistent. Returns [{concept, memories}] sorted by frequency.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ListConceptsTool {
    /// Optional: only return concepts starting with this prefix (case-insensitive).
    pub prefix: Option<String>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 3: INGEST — add files to the brain
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "ingest",
    description = "Ingest a file or folder into the brain. Supports PDF, DOCX, TXT, MD, \
                   MP4/MP3 (if whisper enabled), and SQL files. Auto-detects format, \
                   runs OCR on scanned pages, streams progress. BLAKE3 dedup skips \
                   unchanged files. \
                   \
                   ENTERPRISE MODE: set pointer=true to store a URI + summary only \
                   (no blob embedded), ideal when originals live in SharePoint, S3, \
                   or a system of record. The brain is then a discovery index \
                   layer; callers fetch content at read time.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct IngestTool {
    /// Path to file or directory to ingest
    pub path: String,
    /// Enterprise mode — store URI + summary only, no content embed.
    /// Default false (portable mode: embed full content).
    #[serde(default)]
    pub pointer: Option<bool>,
    /// Optional summary for pointer mode. Falls back to filename if omitted.
    #[serde(default)]
    pub summary: Option<String>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 4: REMEMBER — store text as a memory
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "remember",
    description = "Store a piece of text in the brain as a searchable memory. \
                   Use for notes, decisions, conversation summaries, user preferences. \
                   \
                   Pick the pillar deliberately: \
                     • `episodic` (default) — raw turns, conversations, 'what happened'. \
                     • `semantic` — distilled facts: 'user prefers X', 'API key is Y'. \
                     • `procedural` — action recipes: 'to deploy, run X then Y'. \
                     • `external` — pointers to docs/URLs (Enterprise mode). \
                   \
                   Pillars drive retrieval weighting: Episodic decays by recency, \
                   Semantic is confidence-ranked. Choosing well now pays off once \
                   Dream consolidation lands. Optional id, title, and tags for \
                   organization. The brain learns from every query (S_slow) and \
                   dreams after 100 queries (reconsolidation).",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct RememberTool {
    /// The text content to remember
    pub content: String,
    /// Optional document ID (auto-generated if omitted)
    #[serde(default)]
    pub id: Option<String>,
    /// Optional title/label
    #[serde(default)]
    pub title: Option<String>,
    /// CLS pillar: `episodic` (default, raw turns), `semantic` (distilled
    /// facts), `procedural` (action sequences), `external` (pointer). Unknown
    /// values fall back to `episodic`.
    #[serde(default)]
    pub pillar: Option<String>,
    /// Additional tags (e.g. `["project:xyz", "priority:high"]`). A
    /// `pillar:<name>` tag is auto-added.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 5: STATUS — brain health check
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "status",
    description = "Returns brain health: active frames, file size, dream cycles, \
                   S_slow magnitude, pending dream queries, tombstone count, \
                   SCA index coverage, symbol count. \
                   Use to understand what the brain knows and how active it is.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct StatusTool {}

// ════════════════════════════════════════════════════════════════════════════
// Tool 6: SYM — symbol lookup (proc, table, trigger, class, function)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "sym",
    description = "Look up a code symbol by exact name — stored procedures, tables, \
                   triggers, views, functions, classes, structs, enums. Returns the \
                   symbol type (proc/table/trigger/view/function/class), file location, \
                   and line range. Sub-millisecond. Reveals hidden triggers on tables. \
                   Use when you know the exact object name.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct SymTool {
    /// Symbol name (e.g. "p_dte_Alloc_Card", "chd_Card_Holder_Detail", "MyClass")
    pub name: String,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 7: HISTORY — cognitive lineage (semantic git log)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "history",
    description = "Show the version history of a symbol or document. Walks the \
                   tombstone chain showing each version with its semantic delta \
                   (how much the content changed). Like 'git log' for knowledge. \
                   Use before 'checkout' to see available versions.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct HistoryTool {
    /// Symbol name or doc_id to show history for
    pub name: String,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 8: CHECKOUT — restore a past version
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "checkout",
    description = "Restore a past version of a symbol or document as the new HEAD. \
                   The current version becomes a tombstone (preserved in history). \
                   Use 'history' first to see available versions.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct CheckoutTool {
    /// Symbol name or doc_id
    pub name: String,
    /// Version index from 'history' (0 = original, 1 = first edit, etc.)
    pub version: u32,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 8b: EDIT — surgical, anchored source edit (no whole-file rewrite)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "edit",
    description = "Apply a SURGICAL, anchored edit to a source file on disk — insert, replace, or \
                   delete a small region at a named symbol or an exact-text anchor. There is NO \
                   whole-file-rewrite path, so you cannot accidentally delete the rest of a file. \
                   Prefer this over rewriting a whole file. Modes: insert-after-symbol, \
                   insert-before-symbol, replace-symbol, delete-symbol, insert-after-text, \
                   insert-before-text, replace-text. Provide --symbol for *-symbol modes (resolved \
                   via the symbol index, scoped to `file`) or `anchor` (exact substring) for *-text \
                   modes. Use dry_run to preview. Returns ok/applied_at_line/lines_added/removed.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct EditTool {
    /// Repo-relative path of the source file to change (e.g. src/Program.cs).
    pub file: String,
    /// Edit mode: insert-after-symbol | insert-before-symbol | replace-symbol |
    /// delete-symbol | append-into-symbol | insert-after-text | insert-before-text |
    /// replace-text | insert-after-context | insert-before-context | replace-context.
    /// Optional only when `explain` is true.
    #[serde(default)]
    pub mode: String,
    /// Symbol name for *-symbol modes (resolved scoped to `file`).
    #[serde(default)]
    pub symbol: Option<String>,
    /// Exact substring anchor for *-text modes.
    #[serde(default)]
    pub anchor: Option<String>,
    /// New content to insert/replace (not needed for delete-symbol).
    #[serde(default)]
    pub content: Option<String>,
    /// Preview only: resolve + compute the change but do NOT write the file.
    #[serde(default)]
    pub dry_run: bool,
    /// Allow a replace/delete spanning more than the default max lines.
    #[serde(default)]
    pub allow_large: bool,
    /// Pre-validate only: return the valid scope-correct anchors for `symbol`
    /// or `anchor` (a `valid_anchors` menu) WITHOUT editing. Pick the right
    /// move up front. `mode` may be omitted when explain is true.
    #[serde(default)]
    pub explain: bool,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 8c: EDIT_BATCH — transactional multi-edit (all-or-nothing)
// ════════════════════════════════════════════════════════════════════════════

/// One edit within an `edit_batch`. Same fields as `edit` minus dry_run
/// (the batch controls dry_run for the whole set).
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct BatchEdit {
    /// Repo-relative path to change.
    pub file: String,
    /// Edit mode (same set as the `edit` tool).
    pub mode: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub anchor: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub allow_large: bool,
}

#[mcp_tool(
    name = "edit_batch",
    description = "Apply a SET of surgical edits ALL-OR-NOTHING. Every edit is resolved, applied, \
                   and syntax-verified in memory first; the files are written ONLY if every edit \
                   succeeds. If any edit fails, NOTHING is written — you can never get a \
                   half-applied change set on disk. Use this when a change spans multiple files \
                   (e.g. an endpoint + its test) so they land together or not at all. Send at most \
                   one edit per file per batch. Same modes/anchors as the `edit` tool.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct EditBatchTool {
    /// The edits to apply atomically (one per file).
    pub edits: Vec<BatchEdit>,
    /// Preview only: compute + verify every edit but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 9: DELETE — remove a memory or frame
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "delete",
    description = "Delete memories by doc_id OR by age. Supports enterprise data retention \
                   policies (GDPR, SOX). Examples: delete a specific memory, delete everything \
                   older than 30 days, delete everything before a date. Frames are soft-deleted \
                   (tombstoned) — preserved in history but removed from search results.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct DeleteTool {
    /// Delete a specific frame by doc_id (from search results)
    #[serde(default)]
    pub doc_id: Option<String>,
    /// Delete all frames older than N days (e.g., 30, 90, 365)
    #[serde(default)]
    pub older_than_days: Option<u32>,
    /// Delete all frames created before this date (YYYY-MM-DD format)
    #[serde(default)]
    pub before_date: Option<String>,
    /// Only delete frames matching this tag (e.g. "project:wonga", "ingest:code", "module:card").
    /// Can be used ALONE to remove an entire project — `delete(tag_filter: "project:xyz")` tombstones
    /// every frame carrying that tag (the portable-brain project-wipe). Combined with a time criterion,
    /// it narrows the time-based delete to that tag.
    #[serde(default)]
    pub tag_filter: Option<String>,
    /// Dry run — show what WOULD be deleted without actually deleting
    #[serde(default)]
    pub dry_run: Option<bool>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 11: DISCOVER — auto-detect module boundaries
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "discover",
    description = "Auto-detect module boundaries in a monolithic codebase. \
                   Analyzes all SQL objects (tables, procs, triggers, views, functions), \
                   clusters by FK relationships, identifies hub tables, and classifies \
                   them into modernization strategies: Static Kernel (Enums/Redis), \
                   Identity APIs (Core Identity Microservice), High-Concurrency \
                   Bottlenecks (sequence generators/Kafka). No hardcoded keywords — \
                   pure graph analysis.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct DiscoverTool {}

// ════════════════════════════════════════════════════════════════════════════
// Tool 10: SNAPSHOT — extract a module into its own folder + brain
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "snapshot",
    description = "Extract a module from a monolithic codebase into its own folder. \
                   Creates Exclusive/ (module-only code), Shared/ (dependencies with \
                   usage docs), BOUNDARY.md (architecture rules), MODULE_MAP.md \
                   (object inventory), and a State Synchronized brain. \
                   Use 'discover' first to see available modules.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct SnapshotTool {
    /// Module name (e.g., "card", "billing", "onboarding")
    pub module: String,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 12-pre2: OPEN — attach the MCP server to a different brain file
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "open",
    description = "Attach this MCP server to a different .said brain file. If the file \
                   doesn't exist, it's created empty. If it exists, it's opened and becomes \
                   the active brain for ALL subsequent tool calls (status, search, init, \
                   overview, snapshot, sandbox, clean). This lets you work with many brains \
                   (one per project / client / month) without editing Cursor's MCP config \
                   or restarting. Typical flow: \
                   (1) `open path='G:\\\\work\\\\acme.said'` — switches to the Acme brain \
                   (creates it empty if missing); \
                   (2) `init dir='...'` — populates it. \
                   Does NOT delete or modify the previously-attached brain.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct OpenTool {
    /// Absolute path to the .said file to attach to. Created empty if it
    /// doesn't exist. Accepted: forward or backslash paths on Windows.
    pub path: String,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 12-pre: CREATE — create an empty .said brain file
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "create",
    description = "Create an empty .said brain file. Use this when no brain exists yet \
                   (the MCP server needs an existing file at startup). The empty brain \
                   is ~19 KB with just the header; you'd follow this with 'init' to \
                   bulk-ingest source, or 'remember'/'ingest' to add content piece by \
                   piece. Returns a message confirming the file was created. Note: \
                   the MCP server already has a brain open (the one it was launched \
                   with). This tool creates a NEW file at the given path — you must \
                   restart the MCP server pointing at the new path to use it.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct CreateTool {
    /// Absolute path for the new .said file (e.g. G:\\work\\my-brain.said).
    pub file: String,
    /// Brain deployment mode — `portable` (default, embeds full content,
    /// USB-offline friendly) or `enterprise` (refuses content-embedding
    /// ingests; only `--pointer` ingests and explicit `remember` calls
    /// are allowed). Unknown values fall back to portable.
    #[serde(default)]
    pub mode: Option<String>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 12a: INIT — ingest a directory into the brain (bulk re-index)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "init",
    description = "Ingest a directory of code/SQL into the currently-open brain. \
                   Walks the directory (gitignore-aware), AST-chunks each file, \
                   SCA-encodes, builds the trigram and symbol indexes. Use this \
                   to bootstrap a new monolith or to re-index from scratch. \
                   Takes ~15s for 1,700 SQL files on release build. \
                   After this, use 'overview' to see detected modules.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct InitTool {
    /// Absolute path to the directory containing the source to ingest.
    /// For a Vivere-style SQL Server SSDT project this is usually the
    /// folder containing `dbo/Tables/`, `dbo/Stored Procedures/`, etc.
    pub dir: String,
    /// Set true to tombstone + re-add only changed files (diff-based).
    /// Leave false for a full rebuild.
    #[serde(default)]
    pub incremental: Option<bool>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 12b: OVERVIEW — monolith product catalogue (brain-derived)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "overview",
    description = "List every detected business module/product in the brain with object \
                   counts and the exact name to pass to 'snapshot'. Derived from token- \
                   frequency analysis over frame short-names — no hardcoded keyword \
                   lists, works on any monolith. Pass `check` to probe a specific term \
                   (e.g. 'visa', 'EFT', 'billing') — returns evidence + snapshot command. \
                   Supports comma-separated batch: check='visa,EFT,billing'. \
                   Run this BEFORE 'snapshot' to know what names are valid.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct OverviewTool {
    /// Optional: probe for a specific product/domain term. Prints whether
    /// it exists, matching object examples, and the snapshot command.
    /// Supports comma-separated list for batch probe.
    #[serde(default)]
    pub check: Option<String>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 13: SANDBOX — test environment for a module
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "sandbox",
    description = "Create a live test database for a module. By default: writes \
                   docker-compose + schema + seed files AND starts the container, \
                   waits for SQL Server to be healthy, and loads the full schema. \
                   After this call the sandbox is LIVE on the given port (default \
                   1433) with the named module's procs and triggers deployed against \
                   the full 977-table schema. Pass `modules` to CO-DEPLOY more modules \
                   into the SAME database (so card can call billing, billing triggers \
                   fire on fee inserts, etc.). Pass `up: false` to only write files \
                   without starting docker. Pass `port` to run multiple sandboxes at \
                   once. User phrases like 'create card sandbox', 'spin up card', \
                   'give me a card test DB' all map to this tool with sensible defaults.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct SandboxTool {
    /// Primary module name (e.g., "card", "billing", "onboarding").
    /// If `modules` is set, this is used as the sandbox folder name.
    pub module: String,
    /// Additional modules to include in the SAME sandbox database for
    /// cross-module interaction testing. e.g. ["billing", "fee"] deployed
    /// alongside module="card" exposes any negative interactions across
    /// procs/triggers/FKs. Procs from all listed modules are included.
    #[serde(default)]
    pub modules: Option<Vec<String>>,
    /// SQL Server port on the host (default 1433). Use different ports for
    /// each sandbox so `card` on 1433 and `billing` on 1434 run simultaneously.
    #[serde(default)]
    pub port: Option<u16>,
    /// Optional suffix to distinguish sandboxes that share the same modules.
    /// Used by `said sandbox card --compare before,after` which generates
    /// two card sandboxes tagged `card-before/` and `card-after/`. In most
    /// cases you don't need this.
    #[serde(default)]
    pub label: Option<String>,
    /// Start the container after writing files (docker compose up -d + wait
    /// for SQL Server healthy + load schema). DEFAULT: true — a "sandbox"
    /// without a running database isn't a sandbox. Pass `up: false` if you
    /// only want the generated files (for CI, inspection, or manual review).
    #[serde(default)]
    pub up: Option<bool>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 13a: SYNC — reconcile brain with the filesystem
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "sync",
    description = "Reconcile the brain with the filesystem. Walks every frame that has \
                   a `source:<absolute-path>` tag, checks whether the source file still \
                   exists on disk, and tombstones frames whose source has been deleted. \
                   Also detects files whose content hash changed and flags them as stale. \
                   Use this when: (1) user manually deleted a file and wants the brain \
                   to forget it, (2) after moving/renaming source files, (3) periodically \
                   to keep the brain in sync with disk. Safe to run any time — uses \
                   tombstones (soft-delete), not hard delete, so history is preserved.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct SyncTool {
    /// Preview mode — show what would be tombstoned/re-ingested without
    /// touching the brain.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 13b: JOURNAL — save a dated summary of the current conversation
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "journal",
    description = "Save a dated summary of the current conversation into the brain as \
                   one searchable memory. Use this when the user wraps up a session \
                   (\"that's it for today\", \"let's stop here\") OR at natural \
                   checkpoints (\"ok moving on to X\", \"let's summarise what we have\"). \
                   The entry goes to `mem/YYYY-MM-DD/<topic>` with `kind=journal` tag. \
                   For single facts/decisions mid-conversation, use `remember` instead — \
                   `journal` is for multi-point session summaries.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct JournalTool {
    /// The session summary — write it as a few paragraphs covering: what the
    /// user wanted, what was decided, what was built/extracted/deployed,
    /// any blockers, and next steps. Future-you reading this alone should
    /// understand what happened without the original chat.
    pub summary: String,
    /// Short topic label (e.g. \"vivere-card-snapshot\", \"willie-onboarding\").
    /// Becomes part of the doc_id so the journal entry is easy to find later.
    pub topic: String,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 14: CLEAN — tear down sandboxes and delete generated artifacts
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "clean",
    description = "Stop sandbox containers and delete generated files. Pass `targets` \
                   (a list of module names) to clean specific sandboxes, omit it to \
                   clean all. `all=true` also removes the .said-code/ master folder. \
                   `containers_only=true` stops containers but keeps the generated files \
                   (useful for restart). `dry_run=true` prints the plan without doing it.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct CleanTool {
    /// Specific sandbox module names to clean (e.g. ["card", "card+billing+fee"]).
    /// Empty list = clean everything.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// Also remove the `.said-code/` master folder entirely.
    #[serde(default)]
    pub all: Option<bool>,
    /// Only stop Docker containers; do not delete generated folders.
    #[serde(default)]
    pub containers_only: Option<bool>,
    /// Preview mode — print the plan without executing it.
    #[serde(default)]
    pub dry_run: Option<bool>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 20: SESSION_END — Decision 3 episodic writer (session boundary)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "session_end",
    description = "Call ONCE at the end of a meaningful conversation/session. \
                   Writes an episodic frame tagged `event:session_end`, \
                   `pillar:episodic`, and (if provided) `session:<id>`. \
                   Purpose: the dream function (Decision 3+) distils recent \
                   episodic frames into semantic/procedural memories; without \
                   session_end markers the consolidator can't find the \
                   episode boundary. Brief summary is enough — two or three \
                   sentences covering what happened, what was decided, and \
                   anything surprising worth flagging for future sessions.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct SessionEndTool {
    /// Short summary of the session (2–5 sentences is ideal).
    pub summary: String,
    /// Optional session identifier (user-chosen label or UUID). If supplied,
    /// becomes part of the doc_id and a `session:<id>` tag for later
    /// filtering. If omitted, an auto-generated `ep_N` id is used.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional extra tags (e.g. `["project:xyz", "phase:discovery"]`).
    /// `pillar:episodic` and `event:session_end` are auto-added.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 21: TOOL_COMPLETION — Decision 3 episodic writer (tool/step boundary)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "tool_completion",
    description = "Log the completion of a non-trivial tool call / plan step as an \
                   episodic memory. Use after actions like: deployments, builds, \
                   migrations, long-running ingests, user-facing decisions. \
                   Writes an episodic frame tagged `event:tool_completion`, \
                   `pillar:episodic`, `tool:<name>`, and (if supplied) `status:<ok|fail>`. \
                   Purpose: procedural-memory training data. The dream function \
                   turns recurring tool+input+result patterns into Procedural \
                   frames ('to deploy: A then B then C worked'). Skip for \
                   trivial read-only tool calls (search/get/sym) — those are noise.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ToolCompletionTool {
    /// Name of the tool or plan step that finished (e.g. `deploy`, `ingest`,
    /// `compact`, `manual_migration`).
    pub tool: String,
    /// What the result was — succinct. Include the outcome + anything the
    /// agent should remember next time it runs this tool.
    pub result: String,
    /// Outcome status — `ok` or `fail` (or `partial`). Added as a
    /// `status:<value>` tag for filtering.
    #[serde(default)]
    pub status: Option<String>,
    /// Optional tool arguments summary (keeps the episodic frame
    /// self-describing for future dream consolidation).
    #[serde(default)]
    pub args_summary: Option<String>,
    /// Optional extra tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 22: SALIENCE — Decision 4 heuristic scorer (read-only)
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "salience",
    description = "Score how 'worth remembering' a piece of text is, WITHOUT writing it. \
                   Returns a 0-100 integer score, a band (low/medium/high), and \
                   the tags the writer would auto-attach. Use this to decide \
                   whether to call `remember` or `session_end` at all — cheap \
                   chit-chat scores low and should be dropped, while corrections \
                   and decisions score high and should always be preserved. \
                   Deterministic heuristic (v0). Future upgrade swaps the \
                   internals for a trained model; the return shape is stable.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct SalienceTool {
    /// Content to score (any length).
    pub content: String,
    /// Intended pillar for this memory — affects the pillar-bias signal.
    /// Accepts `episodic` (default), `semantic`, `procedural`, `external`,
    /// `code`, `memory`. Unknown values fall back to `episodic`.
    #[serde(default)]
    pub pillar: Option<String>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 23: DREAM — Decision 5 content consolidation
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "dream",
    description = "Run one content-level dream cycle: cluster recent Episodic frames \
                   by semantic similarity and emit Semantic frames that distil each \
                   cluster. Source Episodic frames stay Active — they're tagged \
                   `dreamed:<cycle>` for audit, not deleted (M365-style preservation). \
                   \
                   Call once at session end after `session_end`, or periodically for \
                   long-running agents. Safe to run on an empty/low-Episodic brain \
                   (returns zero clusters). \
                   \
                   Future automatic triggers (when `SalienceAccumulator` crosses 150, \
                   or on nightly cron) will call this same entry point — so running \
                   it manually is equivalent to forcing an early consolidation.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct DreamTool {
    /// Optional cycle number. If omitted, uses `dream_cycles_total + 1` from
    /// brain state. Providing one lets callers pin a cycle label for tests
    /// / reproducibility.
    #[serde(default)]
    pub cycle: Option<u32>,
    /// Minimum SCA score to join a cluster (0.0..=1.0). Defaults to 0.45.
    /// Lower → more aggressive merging.
    #[serde(default)]
    pub cluster_threshold: Option<f32>,
    /// Minimum cluster size (≥ 2). Defaults to 2.
    #[serde(default)]
    pub min_cluster_size: Option<u32>,
    /// Maximum candidates examined in this cycle (safety cap). Defaults to 500.
    #[serde(default)]
    pub max_candidates: Option<u32>,
}

// ════════════════════════════════════════════════════════════════════════════
// Tool 24: ADMIN — enterprise Recycle Bin + compliance surface
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "admin",
    description = "Administrative operations on the attached brain — the enterprise \
                   Recycle Bin, lineage audit, restore, legal-hold, and retention \
                   sweeps. Mirrors `said admin <action>` in the CLI so web/desktop \
                   UIs (Tauri Brain Explorer, admin dashboards) get parity with \
                   terminal users. \
                   \
                   Actions (pass via the `action` field): \
                   \n  - `list-tombstones`     list non-active frames (optional `like`) \
                   \n  - `restore`             re-promote `doc_id`'s newest tombstone \
                   \n  - `who-deleted`         lineage trail with tags for `doc_id` \
                   \n  - `legal-hold-add`      tag every frame with `legal_hold:<case>` \
                   \n  - `legal-hold-release`  strip the hold \
                   \n  - `retention-sweep`     reap tombstones older than `older_than_days` \
                                               keeping `keep_per_doc` most-recent per doc \
                   \
                   Legal holds block retention sweeps. Restores persist immediately. \
                   Every action saves the brain on success.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct AdminTool {
    /// Which admin action to run — one of: `list-tombstones`, `restore`,
    /// `who-deleted`, `legal-hold-add`, `legal-hold-release`, `retention-sweep`.
    pub action: String,

    /// Document id target (required by restore, who-deleted, legal-hold-*).
    #[serde(default)]
    pub doc_id: Option<String>,

    /// Substring filter for list-tombstones (case-insensitive).
    #[serde(default)]
    pub like: Option<String>,

    /// Case / matter identifier for legal-hold-* actions.
    #[serde(default)]
    pub case: Option<String>,

    /// Retention-sweep age cutoff in days (default 365).
    #[serde(default)]
    pub older_than_days: Option<u64>,

    /// Retention-sweep per-doc keep count (default 1).
    #[serde(default)]
    pub keep_per_doc: Option<u32>,
}

// ════════════════════════════════════════════════════════════════════════════
// Forge tools (feature-gated). Spec-driven workspace generator.
// Read-only: forge_list, forge_get, forge_status.
// Write (require confirm:true): forge_load, forge_run, forge_reset.
// ════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_list",
    description = "[forge] List stories extracted from the current directive. \
                   Optional filter: method:GET, path:/pet/*, kind:api_endpoint, \
                   tag:<name>, text:<substring>.",
    read_only_hint = true,
    idempotent_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeListTool {
    /// Filter expression (optional).
    #[serde(default)]
    pub filter: Option<String>,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_get",
    description = "[forge] Fetch the bundled story+plan+tasks+brain markdown for one or more slugs. \
                   Output capped at 25k tokens with proportional per-section truncation.",
    read_only_hint = true,
    idempotent_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeGetTool {
    /// One or more slugs to fetch.
    pub story_ids: Vec<String>,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_status",
    description = "[forge] Report pending/incomplete/completed status for given slugs.",
    read_only_hint = true,
    idempotent_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeStatusTool {
    pub story_ids: Vec<String>,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_load",
    description = "[forge] Load a new directive (OpenAPI / Markdown) — REPLACES the active \
                   directive. Ask the user for confirmation before calling and show which \
                   path/URL will be loaded. MUST set confirm:true.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeLoadTool {
    pub path_or_url: String,
    #[serde(default)]
    pub source: Option<String>,
    /// MUST be true — gate against accidental calls.
    pub confirm: bool,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_run",
    description = "[forge] Run the generator against selected stories. Costly — uses the \
                   configured BYO LLM. Show the user a cost estimate before calling. \
                   MUST set confirm:true.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeRunTool {
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub all: Option<bool>,
    #[serde(default)]
    pub force: Option<bool>,
    #[serde(default)]
    pub halt_after: Option<u32>,
    pub confirm: bool,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_reset",
    description = "[forge] Tombstone a story's frames and remove its projection folder + \
                   skill file. Destructive — MUST set confirm:true.",
    destructive_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeResetTool {
    pub story_ids: Vec<String>,
    pub confirm: bool,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_init",
    description = "[forge] Scaffold a new forge workspace — creates 4 authority folders \
                   (1-ground-truth, 2-progress, 3-requirements, 4-expectations), a `.forge/` \
                   metadata dir, per-folder READMEs, and an empty .said brain. Idempotent \
                   when merge:true. Safe to call — does not overwrite existing `.said` files."
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeInitTool {
    /// Project name — becomes the `.said` filename and the workspace title.
    pub project: String,
    /// Target directory. Relative paths resolve against the server's cwd.
    /// If omitted, `./$project` is used.
    #[serde(default)]
    pub target: Option<String>,
    /// Create missing files only; skip those that already exist.
    #[serde(default)]
    pub merge: Option<bool>,
    /// Overwrite existing files (except `.said`).
    #[serde(default)]
    pub force: Option<bool>,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_plan_questions",
    description = "[forge] Scan a forge workspace and return the 6 plan-phase questions \
                   (JSON array with id/prompt/options). Agents answer each question, then \
                   call forge_plan_apply with the full answer set. Read-only — safe to call.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgePlanQuestionsTool {
    /// Workspace root. If omitted, the server's cwd is used.
    #[serde(default)]
    pub target: Option<String>,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_plan_apply",
    description = "[forge] Apply plan-phase answers to `.forge/config.toml`. Input: array of \
                   {question_id, answer_key} pairs matching the questions returned by \
                   forge_plan_questions. Sets plan_complete=true on success, unblocking \
                   forge_sync."
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgePlanApplyTool {
    #[serde(default)]
    pub target: Option<String>,
    pub answers: Vec<ForgePlanAnswer>,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_sync",
    description = "[forge] Ingest every file in the 4 authority folders into the workspace \
                   `.said` brain, tagging each frame with authority:<level>:<scope>. Requires \
                   `.forge/config.toml` with plan_complete=true (run forge_plan_apply first). \
                   Idempotent via mtime+size+authority manifest — re-runs skip unchanged files."
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeSyncTool {
    /// Workspace root. If omitted, the server's cwd is used.
    #[serde(default)]
    pub target: Option<String>,
    /// Plan the sync without writing any frames.
    #[serde(default)]
    pub dry_run: Option<bool>,
    /// Re-ingest unchanged files (ignore the manifest gate).
    #[serde(default)]
    pub force: Option<bool>,
}

#[cfg(feature = "forge")]
#[mcp_tool(
    name = "forge_gaps",
    description = "[forge] Return `.forge/gaps.md` — the cross-authority reconciliation report. \
                   If the file doesn't exist, returns a hint to run `said forge gaps` first. \
                   Read-only.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgeGapsTool {
    /// Workspace root. If omitted, the server's cwd is used.
    #[serde(default)]
    pub target: Option<String>,
}

#[cfg(feature = "forge")]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct ForgePlanAnswer {
    pub question_id: String,
    pub answer_key: String,
}

// ════════════════════════════════════════════════════════════════════════════
// LSP TOOLS — cross-file code intelligence
// ════════════════════════════════════════════════════════════════════════════
//
// All four spawn a language-server child process (rust-analyzer / tsserver /
// pyright) on first call, then cache the result as a frame in the .said
// brain (`doc_id = lsp::<op>::<file>:<line>`). Future calls find the
// cached frame via search instead of re-running LSP. This is the design
// that makes `.said` the only memory format that can claim "remembers
// what your codebase IS, not just what you talked about."
//
// The handlers gracefully degrade to "feature not compiled in" messages
// when `said-mcp` was built without `--features lsp`.

#[mcp_tool(
    name = "lsp_def",
    description = "Go-to-definition via LSP. Pass a position as `file:line:col` (e.g. \
                   `src/lib.rs:42:8`). Returns the definition location. Result is \
                   cached as a brain frame for instant future recall. Requires \
                   --features lsp at build time.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct LspDefTool {
    /// Position in the form `file:line:col` (1-based line/col).
    pub location: String,
}

#[mcp_tool(
    name = "lsp_refs",
    description = "Find-references via LSP. Pass `file:line:col`. Returns every usage \
                   of the symbol (declaration + references). Cached as a brain frame. \
                   Requires --features lsp.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct LspRefsTool {
    pub location: String,
}

#[mcp_tool(
    name = "lsp_hover",
    description = "Type info + documentation via LSP. Pass `file:line:col`. Returns \
                   the type signature and any rustdoc / TSDoc / docstring. Not cached \
                   (cheap to re-run). Requires --features lsp.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct LspHoverTool {
    pub location: String,
}

#[mcp_tool(
    name = "lsp_symbols",
    description = "Cross-project symbol search via LSP `workspace/symbol`. Returns \
                   matches across the entire workspace, scoped by symbol name. \
                   Cached as a brain frame. Requires --features lsp.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct LspSymbolsTool {
    /// Symbol name or prefix to search for (e.g. "ScaEngine", "compact_block").
    pub query: String,
}

// ════════════════════════════════════════════════════════════════════════════
// CODING MEMORY — the shared learning store the orchestrator learns from.
// recall_fix + learn_fix call the SAME sca_core helpers as the CLI and
// said-orchestration, so an agent contributes to and reads from ONE store.
// Both are LLM-free (Rule 2): the calling agent drives its own LLM.
// ════════════════════════════════════════════════════════════════════════════

#[mcp_tool(
    name = "recall_fix",
    description = "CODING MEMORY — recall a verified fix for a problem WITHOUT calling an \
                   LLM. Describe the coding problem; .said returns a known-good fix recipe \
                   (the full iteration note + the verified change-set) if it has seen the \
                   same SHAPE before (built+passed). Uses .said's 1-bit semantic + \
                   intent fingerprints, so 'LRU cache' won't match an 'LFU cache'. Returns \
                   no match below the threshold → drive your own LLM and, once the gate is \
                   green, store it with learn_fix. Same store + scorer the orchestrator uses.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct RecallFixTool {
    /// The coding problem in plain words (the recall key).
    pub problem: String,
    /// Minimum match confidence to return a fix (default 0.45). Below it: no match.
    #[serde(default)]
    pub min_score: Option<f32>,
    /// How many candidate fixes to return, highest-score first (default 5). Several are returned so
    /// YOU pick the one whose TASK/change-set fits — when a similar problem out-scores the exact one,
    /// the right fix is often rank 2-3 (the documented recall@5 = 100% contract).
    #[serde(default)]
    pub top_k: Option<u32>,
}

#[mcp_tool(
    name = "learn_fix",
    description = "CODING MEMORY — store a VERIFIED coding iteration so any future caller \
                   (you, the orchestrator, the CLI) can reload it instead of re-deriving. \
                   ONLY call after a real build/test gate is GREEN — success is the sole \
                   recorded outcome. Stores the full story (problem + learnings + the \
                   verified change-set) in the native Procedural pillar, blake3-keyed, \
                   byte-identical to `said learn-fix`. IMPORTANT: capture the NON-OBVIOUS \
                   invariant in `learnings` (the gotcha a textbook version gets wrong), \
                   not a generic summary — that is what lets a future model adapt it right.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct LearnFixTool {
    /// The problem this iteration solved, in plain words (the recall key).
    pub problem: String,
    /// The verified change-set JSON (the `edits` array that built+passed).
    pub edits: String,
    /// Optional: what worked / what to avoid — the non-obvious invariant + the
    /// textbook trap. The highest-value field for future adaptation.
    #[serde(default)]
    pub learnings: Option<String>,
    /// Optional: important files/functions touched and why.
    #[serde(default)]
    pub files: Option<String>,
    /// Optional: errors hit + how they were fixed; approaches that failed.
    #[serde(default)]
    pub errors: Option<String>,
    /// Optional provenance breadcrumb (e.g. a PR number). Never the lookup key.
    #[serde(default)]
    pub label: Option<String>,
}

#[mcp_tool(
    name = "recall_blueprint",
    description = "CODING MEMORY (blueprint) — recall the REUSABLE 80% structure for a SHAPE \
                   WITHOUT calling an LLM. Describe the shape (e.g. 'Create<Entity> REST \
                   endpoint'); .said returns the TOP candidate blueprints (default 3, most \
                   relevant first) — YOU pick the one whose sections fit the task (the right \
                   shape isn't always rank #1 on short skeletons), render it in the active \
                   language, and write only the entity-specific 20%. No match → derive it, then \
                   store with learn_blueprint. Pairs with recall_fix (the specific 20%).",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct RecallBlueprintTool {
    /// The shape in plain words (the recall key).
    pub shape: String,
    /// Minimum match confidence (default 0.45). Below it: no match.
    #[serde(default)]
    pub min_score: Option<f32>,
    /// How many candidate blueprints to return (default 3). Several are returned so YOU pick the one
    /// whose sections fit the task — the right shape isn't always rank #1 on short skeletons.
    #[serde(default)]
    pub top_k: Option<u32>,
}

#[mcp_tool(
    name = "learn_blueprint",
    description = "CODING MEMORY (blueprint) — store the REUSABLE 80% structure for a SHAPE. \
                   KEEP-FIRST by default: if a blueprint for this shape already exists this is a \
                   no-op (the original stands). Set verified=true ONLY after the edited structure's \
                   build/test gate is GREEN — then .said AUTO-UPDATES the blueprint (supersede if it \
                   changed); the green gate is the whole 'is it better' check. Stored in the native \
                   Procedural pillar, blake3-keyed on the shape. Pairs with learn_fix.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct LearnBlueprintTool {
    /// The shape this blueprint covers, in plain words (the recall key).
    pub shape: String,
    /// The sections payload (JSON) — the language-neutral structure the LLM renders per language.
    pub sections: String,
    /// Optional language tag (e.g. "csharp"). Omit for a language-neutral blueprint.
    #[serde(default)]
    pub lang: Option<String>,
    /// Optional provenance breadcrumb. Never the lookup key.
    #[serde(default)]
    pub label: Option<String>,
    /// The structure was edited and the build/test PASSED -> auto-update the blueprint (supersede if it
    /// changed). Omit/false = keep-first (no-op if the shape already has a blueprint).
    #[serde(default)]
    pub verified: Option<bool>,
}

#[mcp_tool(
    name = "harvest_scan",
    description = "CODING MEMORY (blueprint) — STEP 1 of agent-in-the-loop harvest. Scans a repo and \
                   returns the REPEATED code structures (clusters; support>=2) as JSON: each has the \
                   common call-skeleton, sample code, and members. .said does NOT learn them — YOU (the \
                   coding agent) read each cluster and name its ordered NL INTENT phases (the FRAMEWORK \
                   80% only, e.g. 'accept request + write audit row', 'idempotency check', 'wrap + \
                   return' — NOT the entity-specific slots), then call learn_blueprint with those NL \
                   phases as sections. NL phases (not raw call tokens) are required: they are \
                   language-neutral and recall by intent. Use ONCE when onboarding .said onto a repo.",
    read_only_hint = true
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct HarvestScanTool {
    /// The repo directory to scan. Defaults to the current directory.
    #[serde(default)]
    pub dir: Option<String>,
}

#[mcp_tool(
    name = "harvest_blueprints",
    description = "CODING MEMORY (blueprint) — scan an existing repo and AUTO-LEARN blueprints from \
                   REPEATED structures. A function structure becomes a blueprint only if it repeats \
                   (support>=2, clone-mining standard); one-offs are skipped. Keep-first, so re-running \
                   never clobbers a hand-tuned blueprint. Use ONCE when onboarding .said onto an \
                   existing codebase so future entities reuse what's already there.",
    destructive_hint = false
)]
#[derive(Debug, serde::Deserialize, serde::Serialize, JsonSchema)]
pub struct HarvestBlueprintsTool {
    /// The repo directory to scan. Defaults to the current directory.
    #[serde(default)]
    pub dir: Option<String>,
}

// Generate the tool enum that the handler dispatches on. Feature-gated
// entries are doubled so the macro sees a fixed list in each cfg branch.
#[cfg(not(feature = "forge"))]
tool_box!(SaidTools, [SearchTool, AskTool, GetTool, IngestTool, OpenTool, CreateTool, InitTool, SyncTool, RememberTool, JournalTool, StatusTool, ListConceptsTool,
                      SymTool, HistoryTool, CheckoutTool, EditTool, EditBatchTool, DeleteTool,
                      DiscoverTool, OverviewTool, SnapshotTool, SandboxTool, CleanTool,
                      SessionEndTool, ToolCompletionTool, SalienceTool, DreamTool, AdminTool,
                      LspDefTool, LspRefsTool, LspHoverTool, LspSymbolsTool,
                      RecallFixTool, LearnFixTool, RecallBlueprintTool, LearnBlueprintTool, HarvestBlueprintsTool, HarvestScanTool]);

#[cfg(feature = "forge")]
tool_box!(SaidTools, [SearchTool, AskTool, GetTool, IngestTool, OpenTool, CreateTool, InitTool, SyncTool, RememberTool, JournalTool, StatusTool, ListConceptsTool,
                      SymTool, HistoryTool, CheckoutTool, EditTool, EditBatchTool, DeleteTool,
                      DiscoverTool, OverviewTool, SnapshotTool, SandboxTool, CleanTool,
                      SessionEndTool, ToolCompletionTool, SalienceTool, DreamTool, AdminTool,
                      LspDefTool, LspRefsTool, LspHoverTool, LspSymbolsTool,
                      RecallFixTool, LearnFixTool, RecallBlueprintTool, LearnBlueprintTool, HarvestBlueprintsTool, HarvestScanTool,
                      ForgeListTool, ForgeGetTool, ForgeStatusTool,
                      ForgeLoadTool, ForgeRunTool, ForgeResetTool, ForgeInitTool,
                      ForgePlanQuestionsTool, ForgePlanApplyTool, ForgeSyncTool,
                      ForgeGapsTool]);
