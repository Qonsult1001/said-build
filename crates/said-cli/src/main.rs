//! `said` â€” CLI for .said portable brain files.
//!
//! Drop-in replacement for ChromaDB/Pinecone: `said add`, `said query`, `said get`.

mod resolve;
#[cfg(feature = "code")]
mod edit;

use clap::{Parser, Subcommand};
use sca_core::said_file::SaidFile;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// said â€” portable brain files. Vector DB in a single file.
#[derive(Parser)]
#[command(name = "said", version, about = "said - a portable memory you can ask in plain English",
    after_help = "GETTING STARTED (your first memory in 4 commands):\n\
    \x20 said create my-brain.said                         # make an empty memory file\n\
    \x20 said --path my-brain.said add \"The wifi password is sunflower-42\" --id wifi   # store a memory\n\
    \x20 said --path my-brain.said ask \"what is the wifi password\"            # ask in plain English\n\
    \x20 said --path my-brain.said get wifi                # read one memory by its id\n\
    \n\
    Tip: run `said use my-brain.said` once, then drop --path on every command.\n\
    \n\
    EVERYDAY COMMANDS: create | add | ask | get | delete | stats | use\n\
    `ask` is the one you'll use most - it finds memories by meaning, in your own words.\n\
    \n\
    Full step-by-step guide: docs/walkthrough/  (start with tutorial-your-first-brain.md)\n\
    Run `said <command> --help` for the options on any command.")]
struct Cli {
    /// Path to .said file (auto-detects if omitted)
    #[arg(long, global = true)]
    path: Option<String>,

    /// Output as JSON
    #[arg(long, global = true, default_value_t = false)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new, empty memory file
    Create {
        /// File path for the new .said file
        file: String,
        /// Brain deployment mode (portable | enterprise). Default: portable.
        ///
        /// portable  = embed full content; works offline on USB (default)
        /// enterprise = refuse content-embedding ingests; pointer-only
        #[arg(long, default_value = "portable")]
        mode: String,
    },
    /// Store a memory (also available as `remember`)
    #[command(alias = "remember")]
    Add {
        /// Text content to add (omit if using --file or --dir)
        text: Option<String>,
        /// Read content from a file instead
        #[arg(long)]
        file: Option<String>,
        /// Recursively index a directory (AST-aware for code files)
        #[arg(long)]
        dir: Option<String>,
        /// Document ID (auto-generated if omitted)
        #[arg(long)]
        id: Option<String>,
        /// Optional title for the document
        #[arg(long)]
        title: Option<String>,
    },
    /// Read one memory by its id
    Get {
        /// Document ID
        doc_id: String,
    },
    /// Remove a memory by its id
    Delete {
        /// Document ID
        doc_id: String,
    },
    /// Symbol lookup: find a function/struct/class/trait by name.
    /// Tries exact match first, then prefix, then case-insensitive contains.
    /// Use --list to browse symbols by prefix without fuzzy fallback.
    #[cfg(feature = "code")]
    Sym {
        /// Symbol name (or prefix with --list)
        name: String,
        /// Maximum results
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Prefix browse mode â€” lists all symbols whose name starts with `name`
        #[arg(long)]
        list: bool,
    },
    /// Code graph: what a symbol CALLS (functions/procs it references)
    #[cfg(feature = "code")]
    Calls {
        /// Symbol name (function / method / proc)
        name: String,
    },
    /// Code graph: who CALLS a symbol (its callers / reverse edges)
    #[cfg(feature = "code")]
    Callers {
        /// Symbol name (function / method / proc)
        name: String,
    },
    /// Find memories by meaning (ask in plain English) -- the main command
    ///
    /// Tries symbol lookup, trigram grep, and SCA semantic search in parallel,
    /// merges results by confidence, and returns only matches above a threshold.
    /// If the brain has no confident answer, returns empty â€” never fake results.
    ///
    /// The brain learns from every ask: query embeddings accumulate for the
    /// next dream cycle, and recall weights on returned docs get strengthened.
    ///
    /// Examples:
    ///   said ask "how does compact block dict work"
    ///   said ask "what is Alice's email"
    ///   said ask "why do we use block-256 compression"
    Ask {
        /// Natural language question
        query: String,
        /// Maximum results. Default 10 â€” proven by MTEB that the correct
        /// answer is always in top-10 (NDCG@10 â‰¥ 0.89 across all tasks).
        #[arg(long, default_value_t = 10)]
        top: usize,
        /// Deep mode: return ALL relevant chunks (no top-K cap).
        /// Use for full cross-document narrative synthesis â€” "tell me
        /// everything about X from start to finish". Returns every chunk
        /// above the relative threshold instead of capping at --top.
        #[arg(long)]
        deep: bool,
        /// Retrieval engine. `current` (default) = our existing 3-engine
        /// pipeline (sym + grep + SCA semantic with multi-tier ranking).
        /// `semble` = Rust port of MinishLab semble's BM25 + dense + RRF
        /// + 3-boost algorithm â€” for A/B comparison and head-to-head.
        #[arg(long, default_value = "current")]
        engine: String,
    },
    /// Show how many memories you have
    Stats {
        /// Show the full technical breakdown (indexes, brain-state internals).
        #[arg(long)]
        verbose: bool,
    },
    /// List the concepts your memories are linked to ([[wikilink]] vocabulary)
    ///
    /// Shows every distinct concept and how many memories carry it. Use this before
    /// adding a memory so you reuse an existing concept (e.g. "heart") instead of
    /// coining a near-duplicate ("heart-health") — keeps recall consistent.
    #[command(name = "list-concepts")]
    ListConcepts {
        /// Only show concepts starting with this prefix
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Shrink the memory file (reclaim space)
    ///
    /// Pass --drop-history with either --all or --keep N to permanently purge old
    /// versions of deleted/edited memories and reclaim the space they held.
    Compact {
        /// Enable tombstone purge. Requires --all or --keep N to specify
        /// the scope (refuses to run otherwise â€” protects against accidental
        /// full wipes).
        #[arg(long)]
        drop_history: bool,
        /// With --drop-history: purge EVERY tombstone, leaving only current
        /// HEADs. Use for archive/ship workflows.
        #[arg(long)]
        all: bool,
        /// With --drop-history: keep the N most recent tombstones per
        /// doc_id, drop older. "Last N edits per function" model.
        #[arg(long, value_name = "N")]
        keep: Option<usize>,
    },
    /// Get or set config values
    Config {
        /// Config key
        key: Option<String>,
        /// Config value (omit to read)
        value: Option<String>,
    },
    /// Initialize .said brain from current directory
    /// Reads .gitignore, indexes all source files, skips build artifacts
    /// One command to create a searchable brain of your entire project
    #[cfg(feature = "code")]
    Init {
        /// Directory to initialize from (default: current directory)
        #[arg(default_value = ".")]
        dir: String,
        /// Incremental mode: preserve existing brain state and only tombstone + re-add changed files.
        #[arg(long)]
        incremental: bool,
    },
    /// Reindex a single file: tombstones the old frame(s) and inserts the new content,
    /// recording cognitive lineage (semantic delta via 1-bit fingerprint XOR).
    #[cfg(feature = "code")]
    Reindex {
        /// File to reindex
        file: String,
    },
    /// CODING MEMORY — learn a verified coding iteration. Stores the WHOLE
    /// iteration (like Claude Code's session memory, but for code) so any LLM can
    /// reload full context, not just a diff: task + files/functions + steps +
    /// errors&corrections + learnings + the verified change-set. ONLY call after a
    /// real build/test gate is green — `success` is the sole recorded outcome and
    /// ground truth. Recalled memory-first by `recall-fix`.
    ///
    /// REQUIRED surface stays dead-simple: --problem + --edits[-file]. The other
    /// fields are OPTIONAL — pass them when the client has them (richer context),
    /// omit them otherwise.
    #[cfg(feature = "code")]
    LearnFix {
        /// The problem this iteration solved, in plain words (the recall key).
        #[arg(long)]
        problem: String,
        /// The change-set JSON (the `edits` array applied via `said edit`).
        #[arg(long)]
        edits: Option<String>,
        /// Read the change-set JSON from a file instead of --edits (recommended
        /// on Windows PowerShell, which mangles inline JSON quotes).
        #[arg(long)]
        edits_file: Option<String>,
        /// Read a full client-authored iteration NOTE (the 10-section template
        /// filled in, like Claude Code's session memory) from a file. When given,
        /// it becomes the stored story verbatim; --files/--errors/--learnings are
        /// ignored. Get the template via `said fix-template`.
        #[arg(long)]
        note_file: Option<String>,
        /// Optional: important files/functions touched and why (Claude's "Files
        /// and Functions"). Free text.
        #[arg(long)]
        files: Option<String>,
        /// Optional: errors hit and how they were fixed; approaches that failed
        /// and should not be retried (Claude's "Errors & Corrections").
        #[arg(long)]
        errors: Option<String>,
        /// Optional: what worked / what to avoid (Claude's "Learnings").
        #[arg(long)]
        learnings: Option<String>,
        /// Optional provenance tag (e.g. a PR number). Never required, never the
        /// lookup key — just a breadcrumb for traceability.
        #[arg(long)]
        label: Option<String>,
    },
    /// CODING MEMORY — recall a verified fix for a problem WITHOUT calling an LLM.
    /// Describe the problem; `.said` returns a known-good fix recipe if it has
    /// seen the same SHAPE before (built+passed). Uses action/intent-isolated
    /// 1-bit matching so "add an endpoint" never matches "document an endpoint".
    /// Returns no match below the threshold → caller falls through to the LLM.
    #[cfg(feature = "code")]
    RecallFix {
        /// The problem to find a known-good fix for, in plain words.
        #[arg(long)]
        problem: String,
        /// Minimum match score (0.0–1.0) to return a fix. Default 0.55.
        #[arg(long, default_value_t = 0.55)]
        min_similarity: f32,
    },
    /// Surgical, anchored edit of a source file on disk â€” insert/replace/delete
    /// at a named symbol or exact-text anchor. There is NO whole-file rewrite
    /// path, so an autonomous caller cannot delete the rest of a file.
    ///
    /// Modes: insert-after-symbol | insert-before-symbol | replace-symbol |
    ///        delete-symbol | append-into-symbol | insert-after-text |
    ///        insert-before-text | replace-text | insert-after-context |
    ///        insert-before-context | replace-context
    ///
    /// When a --symbol name matches more than one span in --file (e.g. a C#
    /// class and its same-named constructor), pass --line <N> to pick the span
    /// that starts at line N. `append-into-symbol` defaults to the largest
    /// (enclosing) span â€” the class body â€” when ambiguous.
    ///
    /// Examples:
    ///   said edit --file src/Program.cs insert-after-text \
    ///     --anchor 'app.MapGet("/api/pid"' --content '<new line>' --dry-run
    ///   said edit --file tests/HealthTests.cs append-into-symbol \
    ///     --symbol HealthTests --line 10 --content-file new_test.txt --json
    #[cfg(feature = "code")]
    Edit {
        /// Repo-relative path of the source file to change (e.g. src/Program.cs)
        #[arg(long)]
        file: String,
        /// Edit mode (see command help for the list). Optional only with --explain.
        #[arg(default_value = "")]
        mode: String,
        /// Symbol name (for *-symbol modes); resolved scoped to --file
        #[arg(long)]
        symbol: Option<String>,
        /// Disambiguator: when --symbol matches multiple spans in --file, pick
        /// the one whose start line == this value (from the error/--explain).
        #[arg(long)]
        line: Option<usize>,
        /// Exact substring anchor (for *-text modes)
        #[arg(long)]
        anchor: Option<String>,
        /// New content (inline). Mutually exclusive with --content-file.
        #[arg(long)]
        content: Option<String>,
        /// New content read from a file (preferred for multi-line code).
        #[arg(long)]
        content_file: Option<String>,
        /// Resolve + compute the change and report it, but do NOT write.
        #[arg(long)]
        dry_run: bool,
        /// Allow a replace/delete that spans more than the default max lines.
        #[arg(long)]
        allow_large: bool,
        /// Skip the post-edit syntax check (code bundles verify the edited file
        /// still parses via tree-sitter and reject syntax-breaking edits).
        #[arg(long)]
        no_verify: bool,
        /// Pre-validate only: do NOT edit. Returns the valid scope-correct
        /// anchors (a `valid_anchors` menu) for `--symbol` or `--anchor` so a
        /// caller can pick the right move up front. Implies no file write.
        #[arg(long)]
        explain: bool,
    },
    /// Show past versions of a memory
    ///
    /// Lists each saved version of a memory, oldest to newest.
    History {
        /// The memory's id (or symbol name in coding brains)
        name: String,
    },
    /// Restore a memory to an earlier version
    ///
    /// Run `said history <id>` first to see the version list (v0, v1, ...), then
    /// pass that number to --version. Restoring is a real, logged event: the current
    /// content becomes a past version and the restored content becomes current.
    Checkout {
        /// Symbol name or doc_id
        name: String,
        /// Version index from `said history` (v0, v1, ...), e.g. 0 for genesis
        #[arg(long)]
        version: Option<usize>,
        /// Alternatively, the frame_id to restore (from `said history --json`)
        #[arg(long)]
        frame: Option<u64>,
        /// Also write the restored content back to the source file on disk.
        /// Only works for whole-file frames (not AST-chunked symbols) since
        /// restoring one chunk can't rewrite the whole file safely.
        #[arg(long)]
        write: bool,
    },
    /// Import memories from another system (mem0, memvid, â€¦) into this brain.
    ///
    /// Each adapter reads the competitor's export format and maps records
    /// into the right `.said` pillar with source metadata preserved as tags.
    /// Enterprise brains refuse content-embedding imports â€” use `--list` to
    /// see the adapters registered today.
    Import {
        /// Source system: `mem0`, `memvid`. Use `--list` to see current adapters.
        #[arg(long)]
        from: Option<String>,
        /// Path to the competitor's export (file or directory).
        #[arg(long)]
        source: Option<String>,
        /// List registered adapters and exit.
        #[arg(long)]
        list: bool,
    },
    /// Recover deleted memories and manage retention
    ///
    /// Subcommands expose the tombstone lineage for audit, byte-exact
    /// restore (GDPR / SOX / HIPAA friendly), and legal-hold tagging that
    /// blocks retention sweeps. All admin actions are per-brain â€” they
    /// don't reach across files.
    Admin {
        #[command(subcommand)]
        action: AdminAction,
    },
    /// Set default .said file
    Use {
        /// Path to .said file
        file: String,
    },
    /// Ingest a document, video, or folder into the brain.
    ///
    /// Auto-routes by extension:
    ///   .pdf .docx .txt .md         â†’ document_ingest (feature: docs)
    ///   .mp4 .mp3 .wav .m4a .flac   â†’ whisper_ingest  (feature: whisper)
    ///   <dir>                       â†’ walk recursively, pick up every
    ///                                 supported file via the same routing
    ///
    /// Streams progress live for every format â€” one line per page /
    /// paragraph / chunk / segment, same UX as `said init .`.
    #[cfg(feature = "docs")]
    Ingest {
        /// File or directory to ingest (positional)
        target: String,

        /// Enterprise mode â€” store a searchable POINTER (URI + mime + title
        /// + summary) instead of embedding the file's content.
        ///
        /// The original file stays in its system of record (SharePoint, S3,
        /// shared drive, etc.); `.said` records only the discoverable
        /// metadata + short summary. No blob, no decompression, no risk of
        /// stale duplication. The caller fetches the URI at read time if
        /// full content is needed.
        ///
        /// Default (no flag) = Portable mode: embed full content (current
        /// behavior, USB-offline friendly).
        #[arg(long)]
        pointer: bool,

        /// Optional summary text for pointer ingest. If omitted, the first
        /// 200 chars of the file's detected text (or filename for binaries)
        /// are used.
        #[arg(long)]
        summary: Option<String>,
    },
    /// Discover module boundaries in a monolithic codebase.
    ///
    /// Analyzes all SQL objects (tables, procs, triggers, views, functions)
    /// and clusters them into logical modules by:
    ///   1. Naming convention prefixes (chd_, acl_, vb1_)
    ///   2. Table co-occurrence (procs that touch the same tables)
    ///   3. FK relationships (tables linked by foreign keys)
    ///
    /// Outputs discovered modules with object counts, anchor tables,
    /// and shared boundaries between modules.
    #[cfg(feature = "code")]
    Discover {},

    /// Monolith product catalogue â€” lists all detected business modules
    /// (card, account, billing, visa, fica, etc.) with confidence scores
    /// and the exact name to pass to `said snapshot`.
    ///
    /// Example:
    ///   said overview                     â€” list all detected products
    ///   said overview --check visa        â€” does Visa/ISO exist? show evidence
    ///   said overview --check EFT         â€” probe for a specific domain term
    #[cfg(feature = "code")]
    Overview {
        /// Probe the brain for a specific product/domain term. Prints whether
        /// it exists, evidence (matching tables/procs), and suggests the
        /// `said snapshot` command to extract it. Pass multiple comma-separated
        /// terms to probe several domains in a single brain open
        /// (e.g. --check visa,EFT,billing).
        #[arg(long)]
        check: Option<String>,
    },

    /// Extract a module from a monolithic codebase into its own folder.
    ///
    /// Uses semantic search to find all objects related to the module name,
    /// then physically copies SQL files into Exclusive/ (safe to move) and
    /// Shared/ (hub tables with usage analysis). Creates a lens .said file
    /// for querying just the module.
    ///
    /// Example: said snapshot card
    ///   â†’ creates card.vivere/ with all card-related SQL + brain
    #[cfg(feature = "code")]
    Snapshot {
        /// Module name (e.g., "card", "billing", "onboarding")
        /// The engine searches for all objects related to this term.
        module: String,
        /// Output directory (default: .said-code/<module>.<brain_name>/)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Spin up a test database sandbox for one or more modules.
    ///
    /// The simple cases:
    ///   said sandbox card                     # card alone on an auto port
    ///   said sandbox card +billing +fee       # card + billing + fee, one DB
    ///   said sandbox card --compare v1,v2     # two card sandboxes side-by-side
    ///
    /// All three modules above share one 977-table schema â€” procs and triggers
    /// from all listed modules are deployed into the SAME database so cross-
    /// module calls (card proc â†’ billing table, billing trigger â†’ fee function)
    /// run for real. That's the whole point of a sandbox.
    ///
    /// Separate containers are only used when you explicitly pass `--compare`
    /// (to A/B-test two versions) or a different `--port` (to keep two
    /// sandboxes alive at once).
    #[cfg(feature = "code")]
    Sandbox {
        /// Primary module, and any "+module" additions.
        /// Example: said sandbox card +billing +fee
        ///   â†’ one sandbox with card, billing AND fee procs active.
        /// The "+" prefix is what marks an additional module (so the first
        /// arg is unambiguously the primary).
        #[arg(required = true, num_args = 1..)]
        modules: Vec<String>,
        /// SQL Server host port (default 1433 for the first sandbox, +1 for
        /// each additional one when --compare is used). Pass explicitly to
        /// keep two unrelated sandboxes alive side-by-side.
        #[arg(short, long)]
        port: Option<u16>,
        /// A/B compare mode â€” takes a comma-separated list of labels and
        /// spins up one sandbox per label for the SAME module, on adjacent
        /// ports. Useful for "does the refactor break anything?" checks.
        /// Example: said sandbox card --compare before,after
        ///   â†’ two `card` sandboxes on ports 1433 and 1434.
        #[arg(long)]
        compare: Option<String>,
        /// Start the Docker container(s) immediately after generating files.
        /// Without this flag, the command just writes the compose/schema files
        /// and prints the `cd ...; bash run.sh` next step.
        #[arg(long)]
        up: bool,
    },

    /// Tear down sandbox containers and delete generated artifacts.
    ///
    /// Examples:
    ///   said clean                  # stop ALL said-sbx-* containers, keep folders
    ///   said clean card             # tear down card sandbox + delete card.vivere/
    ///   said clean card+billing+fee # tear down the combined sandbox + folder
    ///   said clean --all            # stop all containers AND remove .said-code/ entirely
    ///   said clean --dry-run        # show what would be deleted, don't do it
    #[cfg(feature = "code")]
    Clean {
        /// Specific sandbox folder(s) to delete (e.g. "card", "card+billing+fee",
        /// or "card-before"). Matches folders under .said-code/ by prefix.
        /// Leave empty to act on all sandboxes.
        targets: Vec<String>,
        /// Also delete the .said-code/ master folder entirely.
        #[arg(long)]
        all: bool,
        /// Only stop containers â€” do NOT delete folders.
        #[arg(long)]
        containers_only: bool,
        /// Print what would be done without doing it.
        #[arg(long)]
        dry_run: bool,
    },

    /// Document vault â€” ingest, dedupe, rebuild, restore (Track B).
    #[cfg(feature = "code")]
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },

    /// Go-to-definition via LSP (result cached in .said)
    #[cfg(feature = "lsp")]
    LspDef {
        /// file:line:col (e.g., src/auth.rs:42:10)
        location: String,
    },
    /// Find all references via LSP (cached in .said)
    #[cfg(feature = "lsp")]
    LspRefs {
        /// file:line:col (e.g., src/auth.rs:42:10)
        location: String,
    },
    /// Hover/type info via LSP (cached in .said)
    #[cfg(feature = "lsp")]
    LspHover {
        /// file:line:col (e.g., src/auth.rs:42:10)
        location: String,
    },
    /// Workspace symbol search via LSP
    #[cfg(feature = "lsp")]
    LspSymbols {
        /// Symbol query string
        query: String,
    },

    /// Spec-driven workspace generator. Load a directive (OpenAPI / Markdown),
    /// list stories, run the generator against a configured LLM, project
    /// each story to a .forge/<slug>/ folder + a .claude skill file.
    #[cfg(feature = "forge")]
    Forge {
        #[command(subcommand)]
        verb: ForgeVerb,
    },
    /// Multi-client orchestrator. Auto-discovers clients from
    /// `1-ground-truth/<Client>/` and runs the full ingest â†’
    /// sandbox â†’ spec pipeline for one or many at a time.
    #[cfg(feature = "forge")]
    Clients {
        #[command(subcommand)]
        verb: ClientsVerb,
    },
    /// Dev Spec source-of-truth pipeline.
    #[cfg(feature = "forge")]
    DevSpec {
        #[command(subcommand)]
        action: DevSpecAction,
    },
    /// Step 10 â€” execution-level testing of the OpenAPI contract via a
    /// synthetic HTTP server backed by direct stored-procedure calls.
    /// Walks the entity build order, runs L3 lifecycle per entity
    /// (POST â†’ GET â†’ PUT â†’ GET â†’ LIST), stops at first failure.
    #[cfg(feature = "forge-sql-verify")]
    Test {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<std::path::PathBuf>,
        /// Client name; selects the deliverables subdir + sandbox.
        #[arg(long, value_name = "NAME")]
        client: String,
        /// Bruno collection root. Defaults to
        /// `dt/<CLIENT>/feapiTxnGlobal/.bruno/<Client>-Global/1. Local`.
        #[arg(long, value_name = "DIR")]
        bruno: Option<std::path::PathBuf>,
    },

    /// Agent-steering hook: read a coding agent's PreToolUse JSON on STDIN, inject relevant `.said`
    /// recall on STDOUT when the agent is about to search code (else stays silent / passthrough).
    /// Invoked as a subprocess by the agent's hook system — you don't normally run this by hand.
    /// See `said setup`. (nudge-style: docs/said-structure/16-agent-steering.md)
    #[cfg(feature = "code")]
    Hook {
        /// Which agent's hook protocol to speak. Default: claude.
        #[arg(long, default_value = "claude")]
        agent: String,
        /// Steer mode: `inject` (recall + let grep proceed, default) or `block` (deny + redirect to
        /// .said first). The open experiment — inject is the safer default.
        #[arg(long, default_value = "inject")]
        mode: String,
    },

    /// Register the `.said` agent-steering hook with a coding agent (opt-in). Writes the hook into
    /// the agent's GITIGNORED local settings (`.claude/settings.local.json`) and bundles a `said`
    /// skill — NEVER edits CLAUDE.md, so removal leaves no git trace. `--remove` cleanly undoes it.
    #[cfg(feature = "code")]
    Setup {
        /// Which agent to set up. Default: claude.
        #[arg(long, default_value = "claude")]
        agent: String,
        /// Remove the hook + bundled skill instead of installing.
        #[arg(long)]
        remove: bool,
        /// Print what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
}

#[cfg(feature = "forge")]
#[derive(Subcommand, Debug)]
enum ClientsVerb {
    /// List clients discovered under `1-ground-truth/<Client>/` with
    /// file count, last spec timestamp, and freshness indicator.
    List {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
    },
    /// For each named client (or all if omitted): bring up its
    /// sandbox, generate the OpenAPI spec, then tear the sandbox down.
    /// Sequential by default â€” only one SQL Server container alive at
    /// a time to keep memory bounded.
    Run {
        /// Specific client names to run. Omit to run all discovered clients.
        clients: Vec<String>,
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Starting SQL Server host port. Each client gets the next
        /// free port (incremented by 1 per client).
        #[arg(long, default_value_t = 1433)]
        port: u16,
        /// Cap on row count for closed-set lookup detection.
        #[arg(long, default_value_t = 25)]
        max_enum_rows: usize,
        /// Skip `forge docs` after sandbox up â€” useful when you only
        /// want the database deployed for manual inspection.
        #[arg(long)]
        no_docs: bool,
        /// Leave the container running after spec emission
        /// (default: tear down to free RAM for the next client).
        #[arg(long)]
        keep_running: bool,
        /// Generate the spec from SQL alone â€” bypass Dev Planning.
        /// Produces one op per matchable proc instead of restricting
        /// to a curated `cardholder` slice. Use for clients without
        /// a written Dev Planning catalog (e.g. Vivere).
        #[arg(long)]
        from_sql: bool,
    },
    /// Show per-client state without spinning anything up.
    /// Reads only filesystem mtimes â€” no docker, no SQL.
    Status {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
    },
}

#[cfg(feature = "forge")]
#[derive(Subcommand, Debug)]
enum DevSpecAction {
    /// Parse Dev Planning markdown into canonical JSON.
    Parse {
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        client: String,
    },
    /// Derive ERD from parsed Dev Spec.
    Erd {
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        client: String,
    },
    /// Generate idempotent CREATE TABLE SQL.
    GenerateTables {
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        client: String,
    },
    /// Emit MERGE statements for ars_Api_Rule_Settings.
    AmendRegistry {
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        client: String,
    },
}

#[cfg(feature = "forge")]
#[derive(Subcommand, Debug)]
enum ForgeVerb {
    /// Scaffold a new forge workspace â€” creates 4 authority folders
    /// (1-ground-truth, 2-progress, 3-requirements, 4-expectations), a
    /// `.forge/config.toml` stub, per-folder READMEs, and an empty `.said`
    /// brain. Users drop their source content into the folders, then run
    /// `said forge plan` â†’ `sync` â†’ `run`.
    Init {
        /// Project name â€” becomes the `.said` filename and the workspace title.
        project: String,
        /// Target directory (default: `./$project`). Accepts absolute or
        /// relative paths. Use `--target` instead of the global `--path`
        /// (which points at an existing `.said` for other verbs).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Create missing files only; skip those that already exist.
        #[arg(long)]
        merge: bool,
        /// Overwrite existing files (except `.said`, which is never clobbered).
        #[arg(long)]
        force: bool,
    },
    /// Run the interactive plan phase â€” resolves authority + directive choice
    /// through a speckit-style six-question Q&A. Writes `.forge/config.toml`
    /// on approval. Required before `forge sync`.
    Plan {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Re-run Q&A with previously-saved answers pre-filled (not yet
        /// implemented â€” this just loads the existing config as a base).
        #[arg(long)]
        reconfigure: bool,
    },
    /// Ingest every file in the 4 authority folders into the workspace
    /// `.said` brain. Reads `.forge/config.toml` (required â€” run `forge plan`
    /// first). Tags every frame with `authority:<level>:<scope>`. Idempotent
    /// via mtime+size+authority manifest.
    Sync {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Print the plan without writing any frames.
        #[arg(long)]
        dry_run: bool,
        /// Re-ingest unchanged files (ignore the manifest gate).
        #[arg(long)]
        force: bool,
    },
    /// Generate `.forge/gaps.md` â€” cross-authority reconciliation showing
    /// which directive operations have ground-truth SQL support, which are
    /// partial, and which are missing. Dual-directive mode (both OpenAPI
    /// and Dev Planning chosen) additionally flags operations in one but
    /// not the other, and paths that diverge between them.
    Gaps {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Write the report to a custom path instead of `.forge/gaps.md`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Load a directive (OpenAPI YAML/JSON URL or local file, Markdown).
    Load {
        /// Path or URL
        path_or_url: String,
        /// Force a specific adapter (e.g. "openapi", "markdown").
        #[arg(long)]
        source: Option<String>,
    },
    /// List stories extracted from the most recent directive.
    List {
        #[arg(long)]
        filter: Option<String>,
    },
    /// Show the bundled story + plan + tasks + brain markdown for one slug.
    Show { slug: String },
    /// Status for the most recent batch or a single story.
    Status {
        #[arg(long)]
        story: Option<String>,
    },
    /// Run the generator against selected stories.
    Run {
        #[arg(long)]
        all: bool,
        /// Comma-separated list of slugs.
        #[arg(long)]
        ids: Option<String>,
        /// Filter expression â€” not supported in MVP CLI (use --ids).
        #[arg(long)]
        filter: Option<String>,
        /// Re-run even if a story is already complete.
        #[arg(long)]
        force: bool,
        /// Skip cost-preflight confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Override circuit-breaker threshold (default 5).
        #[arg(long)]
        halt_after: Option<u32>,
    },
    /// Tombstone a story's frames and remove its projection.
    Reset {
        slug: String,
        #[arg(long)]
        yes: bool,
    },
    /// Emit a Mermaid visualisation document at `.forge/viz.md` with
    /// the authority flow, entity relationship diagram (per schema),
    /// and one op-dependency flowchart per directive op. Edges are
    /// tinted by MappingService confidence so reviewers see weak
    /// links at a glance.
    Viz {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Output path (defaults to `<target>/.forge/viz.md`).
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Emit per-op business-story markdown deliverables (one `.md`
    /// per op under `5-deliverables/stories/<Epic>/`).
    Docs {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Output directory (defaults to `<target>/5-deliverables/stories/`).
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Only emit the .md for one op slug (substring match on
        /// `op.slug` or `op.label`). Useful for iterating on a single
        /// example before mass-regenerating 100+ files.
        #[arg(long, value_name = "SLUG")]
        only: Option<String>,
        /// Verify the generated OpenAPI spec against a running
        /// `said sandbox` SQL Server container. Auto-discovers the
        /// container by name (`said-sbx-*`) and pulls closed-set
        /// lookup values to emit as `enum:` in the spec. Read-only;
        /// requires the `forge-sql-verify` feature on the binary.
        #[arg(long)]
        verify_against_sandbox: bool,
        /// Cap on row count for a lookup table to count as a closed
        /// enum. Default 25. Bump to 100 if your domain has bigger
        /// closed sets (currency codes, locale codes, etc.).
        #[arg(long, default_value_t = 25)]
        max_enum_rows: usize,
        /// Client name when the workspace contains multiple client
        /// folders under `1-ground-truth/<Client>/`. Filters the SQL
        /// catalog walk to the client's subtree, scopes sandbox
        /// auto-discovery to a container whose name contains the
        /// client token, and writes output under
        /// `5-deliverables/<Client>/`. Omit for single-client
        /// workspaces (legacy behavior preserved).
        #[arg(long, value_name = "NAME")]
        client: Option<String>,
        /// Generate the OpenAPI spec from SQL alone â€” bypass
        /// `4-expectations/Dev Planning/`. Verb and path are derived
        /// from each proc's name using dt conventions
        /// (`p_<schema>_Get_<entity>` â†’ `GET /<entity>`). Use this for
        /// clients without Dev Planning content.
        #[arg(long)]
        from_sql: bool,
    },
    /// Forge-owned snapshot â€” module-only smoke test. For authoritative
    /// verification, use `said sandbox dt --up` (full monolith) and
    /// `said forge docs --verify-against-sandbox` (auto-discovers it).
    Snapshot {
        /// Module substring matched against frame doc_ids (e.g. `cardholder`).
        module: String,
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
    },
    /// Forge-owned sandbox â€” module-only smoke test. For authoritative
    /// verification, use `said sandbox dt --up` (full monolith).
    Sandbox {
        /// Module substring matched against frame doc_ids.
        module: String,
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// SQL Server host port. Default 1434 to leave 1433 free for any
        /// running `said sandbox` container.
        #[arg(long)]
        port: Option<u16>,
        /// Skip `docker compose up -d`. Just generate the files.
        #[arg(long)]
        no_up: bool,
    },
    /// Run contract tests against the live sandbox. For each operation
    /// in the generated OpenAPI spec, EXEC the backing stored proc and
    /// assert: (a) it parses + runs without SQL error, (b) closed-set
    /// enums actually reject invalid values. Writes a markdown report
    /// to `<workspace>/5-deliverables/<Client>/test-report.md`.
    Test {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Client when the workspace contains multiple clients under
        /// `1-ground-truth/<Client>/`. Required to select the right
        /// sandbox + the right per-client spec.
        #[arg(long, value_name = "NAME")]
        client: Option<String>,
        /// Only test ops whose path or operationId contains this
        /// substring. Useful for iterating on one op without running
        /// the whole suite.
        #[arg(long, value_name = "SUBSTR")]
        only: Option<String>,
    },
    /// Regenerate a Claude Code skill from current SQL ground truth +
    /// an existing skill as starting-point evidence. Writes to
    /// `<output>/.claude/skills/<name>/` with REVIEW.md summarising
    /// what was preserved, regenerated, or generated new.
    Regen {
        /// Workspace root (default: current directory).
        #[arg(long, value_name = "DIR")]
        target: Option<PathBuf>,
        /// Path to the OLD skill folder to read as starting-point
        /// evidence (defaults to `<target>/.claude/skills/<name>/`
        /// and falls back to empty if absent).
        #[arg(long, value_name = "DIR")]
        from: Option<PathBuf>,
        /// Output directory â€” the `<output>/.claude/skills/<name>/`
        /// path is created under this root. Defaults to `<target>`.
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Name of the regenerated skill. Defaults to `dt-api-generator`.
        #[arg(long, default_value = "dt-api-generator")]
        name: String,
        /// Proceed even when regenerated files have a large diff vs
        /// the old ones.
        #[arg(long)]
        force: bool,
    },
    /// Apply an OpenAPI-standard rule to a string and print the result.
    /// One source of truth for singular/plural/casing rules â€” Python
    /// tooling (`fix_bruno.py`, `seed_validation.py`, `generate_bruno.py`)
    /// shells out here instead of duplicating the logic.
    ///
    /// Examples:
    ///   said forge rule singularise binsponsors    # â†’ binsponsor
    ///   said forge rule pluralise   binsponsor     # â†’ binsponsors
    ///   said forge rule camel       account_id     # â†’ accountId
    Rule {
        /// Which rule to apply: `singularise` / `pluralise` / `camel`.
        rule: String,
        /// The input string.
        input: String,
    },

    /// Render a proc skeleton from the proc framework (additive â€” does not
    /// modify deployed files; writes to `<framework>/_rendered/`).
    ///
    /// Reads `<framework>/bundles/<bundle>/bundle.toml` + the matching
    /// shape in `<framework>/profiles/<profile>/sql/_shapes/`. Author then
    /// fills the empty Ignore slot stubs and copies the populated file
    /// into the deployed tree.
    ///
    /// Equivalent to the Python prototype at
    /// `dtcard/.forge/proc-framework/core/render.py`.
    Render {
        /// Framework root, e.g. `dtcard/.forge/proc-framework`.
        #[arg(long, value_name = "DIR")]
        framework: PathBuf,
        /// Profile name, e.g. `webapi-sqlserver-cardissuing`.
        #[arg(long)]
        profile: String,
        /// Bundle name, e.g. `Account`. Reads endpoints with strategy
        /// `thick-sp` for this profile.
        #[arg(long)]
        bundle: Option<String>,
        /// Render only the endpoint with this id (e.g. `Account.CreateAccount`).
        #[arg(long)]
        id: Option<String>,
    },

    /// Render C# Query classes from the proc framework (Phase 2a).
    ///
    /// Mirrors `render` but emits `.cs` Query class files instead of
    /// `.sql` procs. Reads the same bundle/EndpointRow data plus the
    /// `cs/_shapes/` compose-lists and `cs/_shared/` fragments. Output
    /// lands in the profile's `generated_paths.cs_query_root` (under
    /// `5-deliverables/<CLIENT>/cs-api-generated/`).
    RenderCs {
        /// Framework root, e.g. `dtcard/.forge/proc-framework`.
        #[arg(long, value_name = "DIR")]
        framework: PathBuf,
        /// Profile name, e.g. `webapi-sqlserver-cardissuing`.
        #[arg(long)]
        profile: String,
        /// Bundle name, e.g. `Account`.
        #[arg(long)]
        bundle: Option<String>,
        /// Render only the endpoint with this id.
        #[arg(long)]
        id: Option<String>,
    },

    /// Audit deployed procs against the proc framework's expected output.
    ///
    /// Reads every manifest + bundle row for the profile and diffs Fully
    /// regions against the deployed file. Legacy procs (no `@said-managed`
    /// markers) are reported informationally â€” not as defects (Option 3
    /// touch-it-migrate-it model).
    ///
    /// Equivalent to the Python prototype at
    /// `dtcard/.forge/proc-framework/core/audit.py`.
    Audit {
        /// Framework root, e.g. `dtcard/.forge/proc-framework`.
        #[arg(long, value_name = "DIR")]
        framework: PathBuf,
        /// Profile name, e.g. `webapi-sqlserver-cardissuing`.
        #[arg(long)]
        profile: String,
        /// Audit only this bundle.
        #[arg(long)]
        bundle: Option<String>,
        /// Audit only this endpoint id.
        #[arg(long)]
        id: Option<String>,
        /// Root of the deployed proc tree, e.g.
        /// `dtcard/1-ground-truth/TXN/sqlMasterTxnGlobal/src/TxnMasterSQL`.
        #[arg(long, value_name = "DIR")]
        deployed_root: PathBuf,
        /// Show every endpoint, not just defects.
        #[arg(long)]
        verbose: bool,
    },

    /// Audit deployed C# Query classes against the proc framework's marker
    /// convention. Phase 1: classify legacy vs framework-managed via
    /// `// [SaidFully]` / `// [SaidEnd]` markers. Drift detection against
    /// canonical (Phase 2 â€” full generation) is deferred.
    AuditCs {
        /// Framework root, e.g. `dtcard/.forge/proc-framework`.
        #[arg(long, value_name = "DIR")]
        framework: PathBuf,
        /// Profile name, e.g. `webapi-sqlserver-cardissuing`.
        #[arg(long)]
        profile: String,
        /// Audit only this bundle.
        #[arg(long)]
        bundle: Option<String>,
        /// Audit only this endpoint id.
        #[arg(long)]
        id: Option<String>,
        /// Root of the deployed C# Query tree, e.g.
        /// `dt/TXN/feapiTxnGlobal/src/DirectTransact.TxnGlobal.API/Data/Repositories/SqlQueries`.
        #[arg(long, value_name = "DIR")]
        cs_query_root: PathBuf,
        /// Show every endpoint, not just defects.
        #[arg(long)]
        verbose: bool,
    },
}

/// Subcommands for `said admin â€¦`. Kept separate so clap renders a clean
/// nested help menu and each action can grow its own flags over time.
#[derive(Subcommand)]
enum AdminAction {
    /// Show the recycle bin — deleted memories you can still recover, newest first.
    /// Nothing is lost until you permanently clear it with `compact --drop-history`.
    /// (Also available as `list-tombstones`.)
    #[command(alias = "list-tombstones", alias = "recycle-bin")]
    ListTombstones {
        /// Optional substring filter on the memory id (case-insensitive).
        #[arg(long)]
        like: Option<String>,
    },
    /// Recover a deleted memory by its id (brings it back from the recycle bin).
    /// (Also available as `recover`.)
    #[command(alias = "recover")]
    Restore {
        /// id of the memory to recover
        doc_id: String,
    },
    /// Show the deletion trail for a doc_id â€” full lineage with timestamps,
    /// superseded_by chain, and any tags (user_id:, deleted_by:, session:).
    /// This is what an audit log would show for "who deleted X".
    WhoDeleted {
        /// doc_id to trace
        doc_id: String,
    },
    /// Place a legal hold on every frame (active + tombstoned) with this
    /// doc_id. Tags `legal_hold:<case_id>` â€” retention sweeps skip them.
    LegalHoldAdd {
        /// doc_id to hold
        doc_id: String,
        /// Case / matter identifier the hold references
        case: String,
    },
    /// Release a legal hold (strip `legal_hold:<case_id>` tag).
    LegalHoldRelease {
        /// doc_id to release
        doc_id: String,
        /// Case / matter identifier to lift
        case: String,
    },
    /// Apply a retention policy â€” drop tombstones older than `days`, keeping
    /// the most recent `keep_per_doc` per doc_id. Legal holds are honored
    /// (held frames are never touched regardless of age).
    RetentionSweep {
        /// Drop tombstones older than N days. Default 365.
        #[arg(long, default_value_t = 365)]
        older_than_days: u64,
        /// Keep the most recent N tombstones per doc_id even if older.
        /// Default 1 â€” one prior version per doc stays restorable.
        #[arg(long, default_value_t = 1)]
        keep_per_doc: usize,
    },
    /// Show the append-only audit log. BLAKE3-chained â€” tampering breaks the
    /// chain. Use `--verify` to just check integrity without listing entries.
    Audit {
        /// Only verify the chain's integrity â€” print "ok" or the break point.
        #[arg(long)]
        verify: bool,
        /// Filter by actor (substring match).
        #[arg(long)]
        actor: Option<String>,
        /// Filter by event kind (exact match).
        #[arg(long)]
        kind: Option<String>,
    },
}

/// Subcommands for `said vault â€¦`. The enterprise document vault.
#[derive(Subcommand)]
enum VaultAction {
    /// Initialize a new vault file with a bootstrap admin user.
    Init {
        /// Path to the new vault .said file
        path: String,
        /// Admin user identifier (e.g. "admin@example.com")
        #[arg(long)]
        admin: String,
    },
    /// Ingest a document (DOCX or PDF). Default is SLIM mode: dedup parts only,
    /// 100% structural/visual rebuild, ~half the storage. Pass --legal to also
    /// store the byte-exact original for bit-identical restore (compliance
    /// tier). Identity comes from --as <user> or $SAID_VAULT_USER.
    Ingest {
        /// Path to the vault .said file
        vault: String,
        /// Path to the document to ingest
        file: String,
        /// Tag(s) to attach to the manifest (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Legal/compliance tier: also store the byte-exact original so
        /// `restore` returns a bit-identical copy. Default (off) is slim mode
        /// (dedup parts only, structural rebuild via `rebuild`).
        #[arg(long)]
        legal: bool,
        /// User identity to record as ingested_by. Falls back to $SAID_VAULT_USER.
        #[arg(long = "as")]
        as_user: Option<String>,
    },
    /// Bulk-ingest every DOCX/PDF in a directory, compacting + saving once at
    /// the end. Much faster than calling `ingest` per file. Default SLIM mode;
    /// pass --legal for byte-exact tombstones (compliance tier).
    IngestDir {
        /// Path to the vault .said file
        vault: String,
        /// Directory to scan for *.docx / *.pdf (non-recursive)
        dir: String,
        /// Tag(s) to attach to every ingested doc's manifest (repeatable)
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Legal/compliance tier: also store byte-exact originals for
        /// bit-identical restore. Default (off) is slim mode.
        #[arg(long)]
        legal: bool,
        /// User identity to record as ingested_by. Falls back to $SAID_VAULT_USER.
        #[arg(long = "as")]
        as_user: Option<String>,
    },
    /// List ingested documents the authenticated user is allowed to read.
    List {
        /// Path to the vault .said file
        vault: String,
        /// User identity (authorizes the read). Falls back to $SAID_VAULT_USER.
        #[arg(long = "as")]
        as_user: Option<String>,
    },
    /// Rebuild a document from its dedup parts. Requires `rebuild` permission
    /// on the doc's tags.
    Rebuild {
        /// Path to the vault .said file
        vault: String,
        /// doc_id (BLAKE3 of original)
        doc_id: String,
        /// Output directory (will be created if missing)
        #[arg(long, default_value = "./export")]
        out: String,
        /// User identity (authorizes the rebuild). Falls back to $SAID_VAULT_USER.
        #[arg(long = "as")]
        as_user: Option<String>,
    },
    /// Byte-exact restore from the tombstone section (enterprise only).
    /// Requires `restore` permission on the doc's tags.
    Restore {
        /// Path to the vault .said file
        vault: String,
        /// doc_id (BLAKE3 of original)
        doc_id: String,
        /// Output directory (will be created if missing)
        #[arg(long, default_value = "./export")]
        out: String,
        /// User identity (authorizes the restore). Falls back to $SAID_VAULT_USER.
        #[arg(long = "as")]
        as_user: Option<String>,
    },
    /// Compare byte-exact restore vs parts-based rebuild for a doc. Reports
    /// text-match, byte counts, and first diffs. Requires `compare` permission.
    Compare {
        /// Path to the vault .said file
        vault: String,
        /// doc_id (BLAKE3 of original)
        doc_id: String,
        /// User identity (authorizes the compare). Falls back to $SAID_VAULT_USER.
        #[arg(long = "as")]
        as_user: Option<String>,
    },
    /// Statistics: total objects, bytes stored, breakdown by kind. Admin-only.
    Stats {
        /// Path to the vault .said file
        vault: String,
        /// User identity (authorizes the read). Falls back to $SAID_VAULT_USER.
        #[arg(long = "as")]
        as_user: Option<String>,
    },
}

/// Encoder search paths (tried in order).
const ENCODER_PATHS: &[&str] = &[
    "said-lam-static",
    "../said-lam-static",
    "SAID-LAM-private/said-lam-static",
    "../../SAID-LAM-private/said-lam-static",
];

/// Try to load the static encoder from known paths.
fn try_load_encoder(brain: &mut SaidFile) {
    // Embedded encoder first (baked in via the `embed-model` feature) — the same
    // loader `add` uses. Without this, read commands (query/ask/recall) opened a
    // brain with no encoder, so encode_query returned None and SCA semantic search
    // silently died even though the model was compiled into the binary.
    // auto_load_encoder is embedded-first, then falls back to well-known file paths.
    if brain.auto_load_encoder() {
        return;
    }
    for p in ENCODER_PATHS {
        if Path::new(p).exists() {
            if brain.load_encoder(p).is_ok() {
                return;
            }
        }
    }
    // Not fatal â€” grep-only mode still works
}

/// Open an existing .said file, resolving the path.
/// Open .said file + load encoder.
/// SCA index loaded from SCRM (instant). Corpus texts cached lazily on first query/grep.
fn open_brain(path: Option<&str>) -> Result<SaidFile, String> {
    let resolved = resolve::resolve(path)?;
    let mut brain = SaidFile::open(&resolved)?;
    try_load_encoder(&mut brain);
    Ok(brain)
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Create { ref file, ref mode } => cmd_create(file, mode, cli.json),
        Commands::Add { ref text, ref file, ref dir, ref id, ref title } => {
            if let Some(d) = dir {
                cmd_add_dir(cli.path.as_deref(), d, cli.json)
            } else if let Some(t) = text {
                // Smart detect: if text arg is a directory path, index it
                if Path::new(t).is_dir() {
                    cmd_add_dir(cli.path.as_deref(), t, cli.json)
                } else if Path::new(t).is_file() {
                    // It's a file path â€” index the file
                    cmd_add(cli.path.as_deref(), None, Some(t.as_str()), id.as_deref(), title.as_deref(), cli.json)
                } else {
                    // It's text content
                    cmd_add(cli.path.as_deref(), Some(t.as_str()), file.as_deref(), id.as_deref(), title.as_deref(), cli.json)
                }
            } else {
                cmd_add(cli.path.as_deref(), None, file.as_deref(), id.as_deref(), title.as_deref(), cli.json)
            }
        }
        Commands::Get { ref doc_id } => cmd_get(cli.path.as_deref(), doc_id, cli.json),
        Commands::Delete { ref doc_id } => cmd_delete(cli.path.as_deref(), doc_id, cli.json),
        #[cfg(feature = "code")]
        Commands::Sym { ref name, max, list } => cmd_sym(cli.path.as_deref(), name, max, list, cli.json),
        #[cfg(feature = "code")]
        Commands::Calls { ref name } => cmd_code_edges(cli.path.as_deref(), name, false, cli.json),
        #[cfg(feature = "code")]
        Commands::Callers { ref name } => cmd_code_edges(cli.path.as_deref(), name, true, cli.json),
        Commands::Ask { ref query, top, deep, ref engine } => cmd_ask(cli.path.as_deref(), query, top, deep, engine, cli.json),
        #[cfg(feature = "code")]
        Commands::Init { ref dir, incremental } => cmd_init(cli.path.as_deref(), dir, incremental, cli.json),
        #[cfg(feature = "code")]
        Commands::Reindex { ref file } => cmd_reindex(cli.path.as_deref(), file, cli.json),
        #[cfg(feature = "code")]
        Commands::LearnFix { ref problem, ref edits, ref edits_file, ref note_file, ref files, ref errors, ref learnings, ref label } =>
            cmd_learn_fix(cli.path.as_deref(), problem, edits.as_deref(), edits_file.as_deref(),
                note_file.as_deref(), files.as_deref(), errors.as_deref(), learnings.as_deref(), label.as_deref(), cli.json),
        #[cfg(feature = "code")]
        Commands::RecallFix { ref problem, min_similarity } =>
            cmd_recall_fix(cli.path.as_deref(), problem, min_similarity, cli.json),
        #[cfg(feature = "code")]
        Commands::Edit {
            ref file, ref mode, ref symbol, line, ref anchor, ref content, ref content_file,
            dry_run, allow_large, no_verify, explain,
        } => cmd_edit(
            cli.path.as_deref(), file, mode, symbol.as_deref(), line, anchor.as_deref(),
            content.as_deref(), content_file.as_deref(), dry_run, allow_large, no_verify, explain, cli.json,
        ),
        #[cfg(feature = "code")]
        Commands::Hook { ref agent, ref mode } => cmd_hook(cli.path.as_deref(), agent, mode),
        #[cfg(feature = "code")]
        Commands::Setup { ref agent, remove, dry_run } => cmd_setup(cli.path.as_deref(), agent, remove, dry_run),
        Commands::History { ref name } => cmd_history(cli.path.as_deref(), name, cli.json),
        Commands::Checkout { ref name, version, frame, write } => cmd_checkout(cli.path.as_deref(), name, version, frame, write, cli.json),
        Commands::Stats { verbose } => cmd_stats(cli.path.as_deref(), cli.json, verbose),
        Commands::ListConcepts { prefix } => cmd_list_concepts(cli.path.as_deref(), prefix.as_deref(), cli.json),
        Commands::Compact { drop_history, all, keep } => cmd_compact(cli.path.as_deref(), drop_history, all, keep, cli.json),
        Commands::Config { ref key, ref value } => {
            cmd_config(key.as_deref(), value.as_deref(), cli.json)
        }
        Commands::Admin { ref action } => cmd_admin(cli.path.as_deref(), action, cli.json),
        #[cfg(feature = "code")]
        Commands::Vault { ref action } => cmd_vault(action, cli.json),
        Commands::Import { ref from, ref source, list } =>
            cmd_import(cli.path.as_deref(), from.as_deref(), source.as_deref(), list, cli.json),
        Commands::Use { ref file } => cmd_use(file, cli.json),
        #[cfg(feature = "docs")]
        Commands::Ingest { ref target, pointer, ref summary } =>
            cmd_ingest(cli.path.as_deref(), target, pointer, summary.as_deref(), cli.json),
        #[cfg(feature = "code")]
        Commands::Discover {} => cmd_discover(cli.path.as_deref(), cli.json),
        #[cfg(feature = "code")]
        Commands::Overview { ref check } => cmd_overview(cli.path.as_deref(), check.as_deref(), cli.json),
        #[cfg(feature = "code")]
        Commands::Snapshot { ref module, ref output } => cmd_snapshot(cli.path.as_deref(), module, output.as_deref(), cli.json),
        #[cfg(feature = "code")]
        Commands::Sandbox { ref modules, port, ref compare, up } => cmd_sandbox(cli.path.as_deref(), modules, port, compare.as_deref(), up, cli.json),
        #[cfg(feature = "code")]
        Commands::Clean { ref targets, all, containers_only, dry_run } => cmd_clean(cli.path.as_deref(), targets, all, containers_only, dry_run, cli.json),
        #[cfg(feature = "lsp")]
        Commands::LspDef { ref location } => cmd_lsp_def(cli.path.as_deref(), location, cli.json),
        #[cfg(feature = "lsp")]
        Commands::LspRefs { ref location } => cmd_lsp_refs(cli.path.as_deref(), location, cli.json),
        #[cfg(feature = "lsp")]
        Commands::LspHover { ref location } => cmd_lsp_hover(cli.path.as_deref(), location, cli.json),
        #[cfg(feature = "lsp")]
        Commands::LspSymbols { ref query } => cmd_lsp_symbols(cli.path.as_deref(), query, cli.json),
        #[cfg(feature = "forge")]
        Commands::Forge { verb } => forge_cli::dispatch(cli.path.as_deref(), verb, cli.json),
        #[cfg(feature = "forge")]
        Commands::Clients { verb } => forge_cli::clients_dispatch(cli.path.as_deref(), verb, cli.json),
        #[cfg(feature = "forge")]
        Commands::DevSpec { action } => match action {
            DevSpecAction::Parse { target, client } => cmd_dev_spec_parse(target.as_deref(), &client),
            DevSpecAction::Erd { target, client } => cmd_dev_spec_erd(target.as_deref(), &client),
            DevSpecAction::GenerateTables { target, client } => {
                cmd_dev_spec_generate_tables(target.as_deref(), &client)
            }
            DevSpecAction::AmendRegistry { target, client } => {
                cmd_dev_spec_amend_registry(target.as_deref(), &client)
            }
        },
        #[cfg(feature = "forge-sql-verify")]
        Commands::Test { target, client, bruno } =>
            cmd_test(target.as_deref(), &client, bruno.as_deref()),
    };

    if let Err(e) = result {
        if cli.json {
            println!("{}", serde_json::json!({"error": e}));
        } else {
            eprintln!("Error: {}", e);
        }
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

fn cmd_create(file: &str, mode: &str, json: bool) -> Result<(), String> {
    if Path::new(file).exists() {
        return Err(format!("File already exists: {}", file));
    }
    let brain_mode = sca_core::said_file::BrainMode::parse(mode)
        .ok_or_else(|| format!("Unknown mode '{}'. Use 'portable' or 'enterprise'.", mode))?;
    // Mode is chosen here and is immutable for the life of the file.
    // Portable and Enterprise are licensed separately; there is no later
    // `said mode` command.
    let mut brain = SaidFile::create_with_mode(file, brain_mode);
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({"created": file, "mode": brain_mode.as_str()}));
    } else {
        println!("Created: {} (mode: {}, immutable)", file, brain_mode.as_str());
    }
    Ok(())
}


// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// said admin <action> â€” enterprise recycle bin + compliance surface
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn cmd_admin(path: Option<&str>, action: &AdminAction, json: bool) -> Result<(), String> {
    match action {
        AdminAction::ListTombstones { like } => {
            let brain = open_brain(path)?;
            let records = brain.admin_tombstones();
            let filter = like.as_deref().map(str::to_lowercase);
            let rows: Vec<&sca_core::frames::FrameMeta> = records.into_iter()
                .filter(|m| match &filter {
                    Some(needle) => m.doc_id.to_lowercase().contains(needle.as_str()),
                    None => true,
                })
                .collect();
            if json {
                let list: Vec<serde_json::Value> = rows.iter().map(|m| serde_json::json!({
                    "doc_id": m.doc_id,
                    "frame_id": m.id,
                    "status": match m.status {
                        sca_core::frames::FrameStatus::Tombstone => "tombstone",
                        sca_core::frames::FrameStatus::Deleted => "deleted",
                        sca_core::frames::FrameStatus::Active => "active",
                    },
                    "created_at": m.created_at,
                    "superseded_by": m.superseded_by,
                    "uncompressed_len": m.uncompressed_len,
                    "tags": m.tags,
                    "title": m.title,
                    "pillar": format!("{:?}", m.pillar).to_lowercase(),
                    "on_legal_hold": m.tags.iter().any(|t| t.starts_with("legal_hold:") || t.as_str() == "legal_hold"),
                })).collect();
                println!("{}", serde_json::json!({"tombstones": list, "count": rows.len()}));
            } else {
                if rows.is_empty() {
                    println!("Recycle bin is empty — no deleted memories to recover.");
                } else {
                    println!("Deleted memories you can recover ({}):", rows.len());
                    for m in rows {
                        let hold = m.tags.iter()
                            .filter(|t| t.starts_with("legal_hold:"))
                            .map(|t| t.as_str())
                            .collect::<Vec<_>>()
                            .join(",");
                        let hold_marker = if hold.is_empty() { String::new() } else { format!(" [{}]", hold) };
                        let superseded = m.superseded_by
                            .map(|id| format!(" superseded_by=#{}", id))
                            .unwrap_or_default();
                        println!("  {} (frame #{}, {} bytes, created_at={}){}{}",
                            m.doc_id, m.id, m.uncompressed_len, m.created_at, superseded, hold_marker);
                    }
                }
            }
        }
        AdminAction::Restore { doc_id } => {
            let mut brain = open_brain(path)?;
            let (restored_id, displaced) = brain.admin_restore(doc_id)?;
            brain.save()?;
            if json {
                println!("{}", serde_json::json!({
                    "restored_frame_id": restored_id,
                    "displaced_active_id": displaced,
                    "doc_id": doc_id,
                }));
            } else {
                let _ = restored_id;
                println!("âœ“ Recovered memory '{}'.", doc_id);
                if let Some(old) = displaced {
                    let _ = old;
                    println!("  (the version that was current has been kept as a past version)");
                }
            }
        }
        AdminAction::WhoDeleted { doc_id } => {
            let brain = open_brain(path)?;
            let lineage = brain.lineage(doc_id);
            if lineage.is_empty() {
                return Err(format!("No frames found for doc_id '{}'", doc_id));
            }
            if json {
                let entries: Vec<serde_json::Value> = lineage.iter().map(|m| serde_json::json!({
                    "frame_id": m.id,
                    "status": match m.status {
                        sca_core::frames::FrameStatus::Tombstone => "tombstone",
                        sca_core::frames::FrameStatus::Deleted => "deleted",
                        sca_core::frames::FrameStatus::Active => "active",
                    },
                    "created_at": m.created_at,
                    "superseded_by": m.superseded_by,
                    "tags": m.tags,
                })).collect();
                println!("{}", serde_json::json!({"doc_id": doc_id, "lineage": entries}));
            } else {
                println!("Deletion / lineage trail for '{}':", doc_id);
                for m in lineage {
                    let status = match m.status {
                        sca_core::frames::FrameStatus::Tombstone => "tombstoned",
                        sca_core::frames::FrameStatus::Deleted => "deleted",
                        sca_core::frames::FrameStatus::Active => "active",
                    };
                    let superseded = m.superseded_by
                        .map(|id| format!(" â†’ superseded by #{}", id))
                        .unwrap_or_default();
                    // Surface user/session attribution tags if present â€” these
                    // are the closest thing we have to a "who deleted" signal
                    // until the AUDT section (step 11) ships.
                    let attribution: String = m.tags.iter()
                        .filter(|t| t.starts_with("user_id:") || t.starts_with("session:")
                            || t.starts_with("deleted_by:") || t.starts_with("actor:"))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" ");
                    let attribution = if attribution.is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", attribution)
                    };
                    println!("  #{} [{}] created_at={}{}{}", m.id, status, m.created_at, superseded, attribution);
                }
            }
        }
        AdminAction::LegalHoldAdd { doc_id, case } => {
            let mut brain = open_brain(path)?;
            let n = brain.admin_legal_hold_add(doc_id, case);
            brain.save()?;
            if json {
                println!("{}", serde_json::json!({"doc_id": doc_id, "case": case, "frames_tagged": n}));
            } else {
                println!("âœ“ Placed legal hold '{}' on {} frame(s) for doc_id '{}'.", case, n, doc_id);
                if n == 0 { println!("  (no frames found with that doc_id)"); }
            }
        }
        AdminAction::LegalHoldRelease { doc_id, case } => {
            let mut brain = open_brain(path)?;
            let n = brain.admin_legal_hold_release(doc_id, case);
            brain.save()?;
            if json {
                println!("{}", serde_json::json!({"doc_id": doc_id, "case": case, "frames_released": n}));
            } else {
                println!("âœ“ Released legal hold '{}' from {} frame(s) for doc_id '{}'.", case, n, doc_id);
            }
        }
        AdminAction::Audit { verify, actor, kind } => {
            let brain = open_brain(path)?;
            let log = brain.audit();
            if *verify {
                match log.verify() {
                    Ok(()) => {
                        if json {
                            println!("{}", serde_json::json!({"ok": true, "entries": log.len()}));
                        } else {
                            println!("âœ“ Audit chain intact ({} entries, BLAKE3-verified).", log.len());
                        }
                    }
                    Err(e) => {
                        if json {
                            println!("{}", serde_json::json!({"ok": false, "error": e}));
                        } else {
                            return Err(format!("âœ— audit chain broken: {}", e));
                        }
                    }
                }
                return Ok(());
            }
            let entries: Vec<&sca_core::audit::AuditEntry> = log.entries().iter()
                .filter(|e| actor.as_deref().map(|n| e.actor.contains(n)).unwrap_or(true))
                .filter(|e| kind.as_deref().map(|k| e.kind == k).unwrap_or(true))
                .collect();
            if json {
                let list: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({
                    "seq": e.seq,
                    "timestamp": e.timestamp,
                    "actor": e.actor,
                    "kind": e.kind,
                    "target": e.target,
                    "detail": e.detail,
                    "hash": e.hash.iter().map(|b| format!("{:02x}", b)).collect::<String>(),
                })).collect();
                println!("{}", serde_json::json!({"entries": list, "total": log.len()}));
            } else {
                if entries.is_empty() {
                    println!("No audit entries match filter.");
                } else {
                    println!("Audit log ({} of {} entries):", entries.len(), log.len());
                    for e in entries {
                        println!("  #{:<6} [{}] {:<14} actor={:<12} target={:<40} {}",
                            e.seq, e.timestamp, e.kind, e.actor, e.target, e.detail);
                    }
                }
            }
        }
        AdminAction::RetentionSweep { older_than_days, keep_per_doc } => {
            let mut brain = open_brain(path)?;
            // For the day-based filter, we need to tombstone frames created
            // more than `older_than_days` days ago BEFORE we ask drop_tombstones
            // to reap. Frames already tombstoned and older than the threshold
            // become candidates for drop. We keep the drop itself in the
            // existing `drop_history_keep` function which honors legal holds.
            let cutoff_ts = {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs()).unwrap_or(0);
                now.saturating_sub(older_than_days.saturating_mul(86_400))
            };
            // The existing drop function doesn't filter by age directly, so
            // we filter the set: only reap tombstones that are (a) already
            // tombstoned AND (b) older than cutoff. Easiest path: iterate the
            // admin tombstone list to mark Deleted on qualifying frames while
            // respecting `keep_per_doc`. Done in-brain below.
            let records: Vec<(String, u64, u64)> = brain.admin_tombstones().iter()
                .filter(|m| m.status == sca_core::frames::FrameStatus::Tombstone)
                .filter(|m| !m.tags.iter().any(|t| t.starts_with("legal_hold:") || t.as_str() == "legal_hold"))
                .filter(|m| m.created_at < cutoff_ts)
                .map(|m| (m.doc_id.clone(), m.created_at, m.id))
                .collect();
            // Group by doc_id, sort each newest-first, skip the first
            // keep_per_doc, schedule the rest for deletion.
            use std::collections::HashMap;
            let mut by_doc: HashMap<String, Vec<(u64, u64)>> = HashMap::new();
            for (did, ts, fid) in records { by_doc.entry(did).or_default().push((ts, fid)); }
            let mut dropped = 0usize;
            for (did, mut v) in by_doc {
                v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
                for (_ts, fid) in v.into_iter().skip(*keep_per_doc) {
                    if brain.mark_frame_deleted(fid) { dropped += 1; }
                    let _ = did; // silence warning
                }
            }
            brain.save()?;
            if json {
                println!("{}", serde_json::json!({
                    "older_than_days": older_than_days,
                    "keep_per_doc": keep_per_doc,
                    "dropped": dropped,
                }));
            } else {
                println!("âœ“ Retention sweep: dropped {} tombstones older than {} days (kept {} per doc_id).",
                    dropped, older_than_days, keep_per_doc);
                println!("  Legal holds honored â€” no held frame was touched.");
                println!("  Run `said compact` to physically reclaim the freed bytes.");
            }
        }
    }
    Ok(())
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// said vault <action> â€” enterprise document vault (Track B)
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[cfg(feature = "code")]
fn cmd_vault(action: &VaultAction, json: bool) -> Result<(), String> {
    match action {
        VaultAction::Init { path, admin } => {
            said_vault::SaidVault::init(path, admin)?;
            if json {
                println!("{}", serde_json::json!({"ok": true, "path": path, "admin": admin}));
            } else {
                println!("initialized: {} (admin: {})", path, admin);
            }
            Ok(())
        }
        VaultAction::Ingest { vault, file, tags, legal, as_user } => {
            let user = resolve_vault_user(as_user.as_deref())?;
            let mut v = said_vault::SaidVault::open(vault)?;
            let id = v.ingest(file, tags, *legal, &user)?;
            let tier = if *legal { "legal" } else { "slim" };
            if json {
                println!("{}", serde_json::json!({"doc_id": id, "ingested_by": user, "tier": tier}));
            } else {
                println!("ingested: {} (by {}, {} tier)", id, user, tier);
            }
            Ok(())
        }
        VaultAction::IngestDir { vault, dir, tags, legal, as_user } => {
            let user = resolve_vault_user(as_user.as_deref())?;
            let mut paths: Vec<String> = Vec::new();
            for entry in std::fs::read_dir(dir).map_err(|e| format!("read dir {}: {}", dir, e))? {
                let p = entry.map_err(|e| format!("dir entry: {}", e))?.path();
                let is_doc = p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| {
                        let e = e.to_lowercase();
                        e == "docx" || e == "pdf"
                    })
                    .unwrap_or(false);
                if is_doc {
                    paths.push(p.to_string_lossy().to_string());
                }
            }
            paths.sort();
            let mut v = said_vault::SaidVault::open(vault)?;
            let ids = v.ingest_batch(&paths, tags, *legal, &user)?;
            let tier = if *legal { "legal" } else { "slim" };
            if json {
                println!("{}", serde_json::json!({"count": ids.len(), "ingested_by": user, "doc_ids": ids, "tier": tier}));
            } else {
                println!("ingested {} docs (by {}, {} tier)", ids.len(), user, tier);
            }
            Ok(())
        }
        VaultAction::List { vault, as_user } => {
            let user = resolve_vault_user(as_user.as_deref())?;
            let (mut v, roles) = said_vault::SaidVault::open_as(vault, &user)?;
            // Collect candidate doc_ids, then filter to those the user may read.
            let candidates: Vec<String> = v.store().brain().frames.active_doc_ids().iter()
                .filter_map(|did| did.strip_prefix("vault:manifest:").map(|s| s.to_string()))
                .collect();
            let mut ids = Vec::new();
            for id in candidates {
                if v.authorize_doc(&roles, &id, "read").is_ok() {
                    ids.push(id);
                }
            }
            if json {
                println!("{}", serde_json::json!({"docs": ids}));
            } else {
                for id in &ids {
                    println!("{}", id);
                }
            }
            Ok(())
        }
        VaultAction::Rebuild { vault, doc_id, out, as_user } => {
            let user = resolve_vault_user(as_user.as_deref())?;
            let (mut v, roles) = said_vault::SaidVault::open_as(vault, &user)?;
            v.authorize_doc(&roles, doc_id, "rebuild")?;
            std::fs::create_dir_all(out).map_err(|e| format!("create out dir: {}", e))?;
            let path = v.rebuild(doc_id, out)?;
            if json {
                println!("{}", serde_json::json!({"path": path}));
            } else {
                println!("rebuilt: {}", path);
            }
            Ok(())
        }
        VaultAction::Restore { vault, doc_id, out, as_user } => {
            let user = resolve_vault_user(as_user.as_deref())?;
            let (mut v, roles) = said_vault::SaidVault::open_as(vault, &user)?;
            v.authorize_doc(&roles, doc_id, "restore")?;
            std::fs::create_dir_all(out).map_err(|e| format!("create out dir: {}", e))?;
            let path = v.restore(doc_id, out)?;
            if json {
                println!("{}", serde_json::json!({"path": path}));
            } else {
                println!("restored: {}", path);
            }
            Ok(())
        }
        VaultAction::Compare { vault, doc_id, as_user } => {
            let user = resolve_vault_user(as_user.as_deref())?;
            let (mut v, roles) = said_vault::SaidVault::open_as(vault, &user)?;
            v.authorize_doc(&roles, doc_id, "compare")?;
            let r = v.compare(doc_id)?;
            if json {
                let diffs: Vec<serde_json::Value> = r.first_diffs.iter()
                    .map(|(i, a, b)| serde_json::json!({"index": i, "restored": a, "rebuilt": b}))
                    .collect();
                println!("{}", serde_json::json!({
                    "doc_id": r.doc_id,
                    "restored_bytes": r.restored_bytes,
                    "rebuilt_bytes": r.rebuilt_bytes,
                    "restored_paragraphs": r.restored_paragraphs,
                    "rebuilt_paragraphs": r.rebuilt_paragraphs,
                    "text_match": r.text_match,
                    "byte_identical": r.byte_identical,
                    "first_diffs": diffs,
                }));
            } else {
                println!("doc:                {}", r.doc_id);
                println!("restored bytes:     {}", r.restored_bytes);
                println!("rebuilt bytes:      {}", r.rebuilt_bytes);
                println!("restored paragraphs:{}", r.restored_paragraphs);
                println!("rebuilt paragraphs: {}", r.rebuilt_paragraphs);
                println!("text match:         {}", r.text_match);
                println!("byte identical:     {} (false expected â€” rebuild re-zips)", r.byte_identical);
                for (i, a, b) in &r.first_diffs {
                    println!("  diff @ {}: restored={:?} rebuilt={:?}", i, a, b);
                }
            }
            Ok(())
        }
        VaultAction::Stats { vault, as_user } => {
            let user = resolve_vault_user(as_user.as_deref())?;
            let (v, roles) = said_vault::SaidVault::open_as(vault, &user)?;
            // Stats is corpus-wide and admin-facing â€” require the `read`
            // operation be permitted by at least one of the user's roles.
            if !said_vault::SaidVault::authorize_operation(&roles, "read") {
                return Err(format!(
                    "permission denied: user '{}' has no role permitting 'read'", user,
                ));
            }
            let stats = v.stats()?;
            if json {
                let by_kind: serde_json::Map<String, serde_json::Value> = stats.objects_by_kind.iter()
                    .map(|(k, c)| (k.clone(), serde_json::json!(c)))
                    .collect();
                println!("{}", serde_json::json!({
                    "total_objects": stats.total_objects,
                    "total_bytes": stats.total_data_size,
                    "by_kind": by_kind,
                }));
            } else {
                println!("Total objects: {}", stats.total_objects);
                println!("Total bytes:   {}", stats.total_data_size);
                let mut kinds: Vec<_> = stats.objects_by_kind.iter().collect();
                kinds.sort_by_key(|(k, _)| (*k).clone());
                for (k, c) in kinds {
                    println!("  {:<12} {}", k, c);
                }
            }
            Ok(())
        }
    }
}

/// Resolve vault identity from --as <user> or $SAID_VAULT_USER. Errors
/// clearly if neither is set; identity is mandatory for ingest so the
/// manifest's ingested_by is always populated.
#[cfg(feature = "code")]
fn resolve_vault_user(as_flag: Option<&str>) -> Result<String, String> {
    if let Some(u) = as_flag {
        return Ok(u.to_string());
    }
    std::env::var("SAID_VAULT_USER")
        .map_err(|_| "no vault identity â€” pass --as <user> or set SAID_VAULT_USER".to_string())
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// said import --from <adapter> --source <path> â€” competitor migration
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn cmd_import(
    path: Option<&str>,
    from: Option<&str>,
    source: Option<&str>,
    list: bool,
    json: bool,
) -> Result<(), String> {
    if list {
        let names = sca_core::migrate::registered_adapters();
        if json {
            println!("{}", serde_json::json!({"adapters": names}));
        } else {
            println!("Registered migration adapters:");
            for n in names { println!("  - {}", n); }
        }
        return Ok(());
    }

    let from = from.ok_or_else(|| "--from is required (e.g. --from mem0); use --list to see options".to_string())?;
    let source = source.ok_or_else(|| "--source <path> is required".to_string())?;

    let adapter = sca_core::migrate::adapter_for(from).ok_or_else(|| format!(
        "Unknown source '{}'. Registered: {}",
        from,
        sca_core::migrate::registered_adapters().join(", "),
    ))?;

    let mut brain = open_brain(path)?;
    let report = sca_core::migrate::run_migration(
        adapter.as_ref(),
        std::path::Path::new(source),
        &mut brain,
    )?;
    brain.build_index().map_err(|e| format!("build_index: {}", e))?;
    brain.save()?;

    if json {
        let per_pillar: serde_json::Map<String, serde_json::Value> = report.per_pillar.iter()
            .map(|(k, v)| (k.clone(), serde_json::json!(v)))
            .collect();
        println!("{}", serde_json::json!({
            "source_system": report.source_system,
            "records_read": report.records_read,
            "records_written": report.records_written,
            "records_skipped": report.records_skipped,
            "per_pillar": per_pillar,
            "errors": report.errors,
        }));
    } else {
        println!("âœ“ Imported from {}:", report.source_system);
        println!("  read:    {}", report.records_read);
        println!("  written: {}", report.records_written);
        if report.records_skipped > 0 {
            println!("  skipped: {}", report.records_skipped);
        }
        if !report.per_pillar.is_empty() {
            let parts: Vec<String> = report.per_pillar.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            println!("  pillars: {}", parts.join(", "));
        }
        if !report.errors.is_empty() && report.errors.len() <= 5 {
            for e in &report.errors { println!("  ! {}", e); }
        } else if !report.errors.is_empty() {
            println!("  ! {} warnings (first 3):", report.errors.len());
            for e in report.errors.iter().take(3) { println!("    - {}", e); }
        }
    }
    Ok(())
}

fn cmd_add(
    path: Option<&str>,
    text: Option<&str>,
    file: Option<&str>,
    id: Option<&str>,
    title: Option<&str>,
    json: bool,
) -> Result<(), String> {
    let content = if let Some(f) = file {
        std::fs::read_to_string(f).map_err(|e| format!("Cannot read file '{}': {}", f, e))?
    } else if let Some(t) = text {
        t.to_string()
    } else {
        return Err("Provide text or --file or --dir".into());
    };

    let doc_id = if let Some(i) = id {
        i.to_string()
    } else if let Some(f) = file {
        Path::new(f)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("doc_{}", chrono_free_timestamp()))
    } else {
        format!("doc_{}", chrono_free_timestamp())
    };

    let mut brain = open_brain(path)?;

    // BLAKE3 dedup for --file
    if let Some(f) = file {
        let file_bytes = std::fs::read(f).map_err(|e| format!("Cannot read '{}': {}", f, e))?;
        let hash = blake3::hash(&file_bytes);
        let hash_tag = format!("blake3:{}", hash.to_hex());
        let already_indexed = brain.frames.active_doc_ids().iter().any(|did| {
            brain.frames.get_meta(did)
                .map(|m| m.tags.iter().any(|t| t == &hash_tag))
                .unwrap_or(false)
        });
        if already_indexed {
            if json {
                println!("{}", serde_json::json!({"skipped": doc_id, "reason": "unchanged (blake3 match)"}));
            } else {
                println!("Skipped '{}' (unchanged)", doc_id);
            }
            return Ok(());
        }
    }

    if let Some(t) = title {
        brain.add_with_title(&doc_id, &content, t);
    } else {
        brain.add(&doc_id, &content);
    }
    brain.save()?;

    if json {
        println!("{}", serde_json::json!({
            "added": doc_id,
            "bytes": content.len()
        }));
    } else {
        println!("Added '{}' ({} bytes)", doc_id, content.len());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Known file extensions for directory indexing
// ---------------------------------------------------------------------------

// SQL is special-cased: it has no tree-sitter grammar in our deps and uses
// its own GO-batch parser. Listed here so the file-walker still picks up
// `.sql`/`.ddl`/`.tsql` files for ingest.
const SQL_EXTENSIONS: &[&str] = &["sql", "ddl", "tsql"];

// Pure-text formats with no AST chunker; whole-file storage at ingest.
const PLAIN_TEXT_EXTENSIONS: &[&str] = &[
    "txt", "html", "xml", "csv", "cfg", "ini",
];

/// File-enrollment filter for `init` (which extensions get walked + ingested).
/// Drives off the central grammar registry â€” adding a language to
/// `sca_core::grammars::register_languages()` automatically enables init
/// for that extension.
fn code_extension(ext: &str) -> bool {
    #[cfg(feature = "code")]
    {
        if sca_core::grammars::supported_extensions().iter().any(|e| *e == ext) {
            return true;
        }
    }
    SQL_EXTENSIONS.contains(&ext)
}

fn text_extension(ext: &str) -> bool {
    PLAIN_TEXT_EXTENSIONS.contains(&ext)
}

/// Binary document formats that `init` can ingest when the `docs` feature is built —
/// extracted to text via `document_ingest` (DOCX/PDF) rather than decoded as raw text.
/// Enables `said init <dir-of-docx>` to build a queryable brain from a document corpus
/// (e.g. the legal bench-corpus), with the OKF cross-link pass on top.
fn doc_extension(ext: &str) -> bool {
    #[cfg(feature = "docs")]
    { matches!(ext, "docx" | "pdf") }
    #[cfg(not(feature = "docs"))]
    { let _ = ext; false }
}

/// AST-aware (chunker is invoked) â€” same as `code_extension` since the
/// registry only contains entries we can chunk. SQL is included via the
/// dedicated SQL chunker.
fn ast_extension(ext: &str) -> bool {
    code_extension(ext)
}

/// Decode file bytes as text. Handles three common encodings:
///
/// 1. **UTF-16 LE BOM** (`FF FE â€¦`) â€” what SSMS exports for SQL Server
///    Unicode scripts. Decoded losslessly via `from_utf16_lossy`.
/// 2. **UTF-16 BE BOM** (`FE FF â€¦`) â€” rarer, but still valid SSMS output.
/// 3. **UTF-8** with or without BOM (`EF BB BF`) â€” everything else.
///
/// Returns `None` only when none of the above produce valid text â€” e.g.
/// real binary like images / PDFs / `.exe`. The BOM bytes are stripped
/// from the returned string so downstream chunkers don't see them.
fn decode_text(bytes: &[u8]) -> Option<String> {
    // UTF-16 LE
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    // UTF-16 BE
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    // UTF-8 BOM
    let payload = if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        &bytes[3..]
    } else {
        bytes
    };
    String::from_utf8(payload.to_vec()).ok()
}

/// Recursively collect files from a directory.
/// Returns true if a directory name looks like a backup/stale copy that
/// should not be indexed. Matches by exact segment name only (not substring)
/// so legitimate paths containing "backup" as part of a longer word (e.g.
/// "SherpaOnnxEntrybackupability") are not incorrectly skipped.
fn is_backup_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Exact segment matches: strict, low false-positive
    matches!(lower.as_str(),
        "backup" | "backups"
        | "latest_backup" | "_backup" | "_backups"
        | "_backup_pre_port" | "_backup_pre" | "_old"
        | "old" | "stale" | "archive" | "archived"
    )
    // Prefix match for common patterns: _backup_*, _old_*, backup_*
    || lower.starts_with("_backup_")
    || lower.starts_with("backup_")
    || lower.starts_with("_old_")
    // Suffix match: *_backup, *_old â€” only when segment has no spaces/dots
    || (!lower.contains('.') && (lower.ends_with("_backup") || lower.ends_with("_old")))
}

/// Vendored-dependency / tool-cache directories that must NEVER be ingested: they are
/// downloaded third-party code, not the user's source, and bloat the brain with junk
/// (e.g. node_modules/typescript.js). Both directory walkers consult this so the skip
/// list can never drift between them. Matched by exact directory-segment name.
///
/// DELIBERATELY CONSERVATIVE. Only names that are ALWAYS dependency/cache dirs are listed.
/// Generic names that frequently hold REAL user content are NOT excluded here, because
/// over-exclusion silently drops a user's code (regression: listing "out" dropped
/// `_deploy/out/` — real deployment SQL — from 277 memories to 2). Notably EXCLUDED from
/// this list and therefore INGESTED: `out`, `bin`, `obj`, `build`, `dist`, `packages`,
/// `coverage` — any of which can be hand-written source. Build artifacts under those that
/// the user genuinely wants skipped should be covered by their `.gitignore` (which the
/// walker already honors), not by a hardcoded guess. The doc contract (init.md "What gets
/// skipped") sanctions "node_modules, target, .venv, build artifacts" — kept tight to that.
fn is_junk_dir(name: &str) -> bool {
    matches!(name,
        "node_modules" | "bower_components" | "vendor"
        | "site-packages" | ".venv" | "venv"
        | "target"
        | "__pycache__" | ".mypy_cache" | ".pytest_cache" | ".tox"
        | ".gradle" | ".turbo" | ".parcel-cache" | ".cache" | ".vite"
        | ".next" | ".nuxt" | ".svelte-kit"
    )
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden dirs and common junk
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with('.')
                || is_junk_dir(&name)
                || is_backup_dir(&name)
            {
                continue;
            }
            walk_dir(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn cmd_add_dir(path: Option<&str>, dir: &str, json: bool) -> Result<(), String> {
    use sca_core::frames::PutOptions;

    let dir_path = Path::new(dir).canonicalize()
        .map_err(|e| format!("Cannot resolve directory '{}': {}", dir, e))?;
    if !dir_path.is_dir() {
        return Err(format!("Not a directory: {}", dir));
    }

    // One ingestion_id was used here historically; PutOptions no longer
    // carries it. Kept as a no-op timestamp so any future grouping work
    // has the wall-clock anchor.
    let _ingestion_id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let mut files = Vec::new();
    walk_dir(&dir_path, &mut files);

    // Filter to known extensions
    let files: Vec<PathBuf> = files.into_iter().filter(|p| {
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            code_extension(&ext_lower) || text_extension(&ext_lower) || doc_extension(&ext_lower)
        } else {
            false
        }
    }).collect();

    // Open or create. If file has zero frames, start fresh to avoid mmap issues
    // (compact on empty mmap data produces corrupt blocks)
    let resolved = resolve::resolve(path)?;
    let mut brain = if resolved.exists() {
        let test_brain = SaidFile::open(&resolved).map_err(|e| format!("Open error: {}", e))?;
        if test_brain.stats().active_frames == 0 {
            // Empty .said file â€” create fresh (avoids mmap-on-empty-data issues)
            drop(test_brain);
            SaidFile::create(&resolved)
        } else {
            test_brain
        }
    } else {
        SaidFile::create(&resolved)
    };
    try_load_encoder(&mut brain);

    // Mode guard: `said add --dir` embeds file content as frames. Enterprise
    // brains refuse. Callers should use pointer ingest via `said ingest --pointer`.
    brain.ensure_content_ingest_allowed()?;

    let mut added = 0u64;
    let mut skipped = 0u64;

    for file_path in &files {
        let file_bytes = match std::fs::read(file_path) {
            Ok(b) => b,
            Err(_) => { skipped += 1; continue; }
        };
        let hash = blake3::hash(&file_bytes);
        let hash_tag = format!("blake3:{}", hash.to_hex());

        // Check if already indexed with same hash
        let already_indexed = brain.frames.active_doc_ids().iter().any(|did| {
            brain.frames.get_meta(did)
                .map(|m| m.tags.iter().any(|t| t == &hash_tag))
                .unwrap_or(false)
        });
        if already_indexed {
            skipped += 1;
            continue;
        }

        let content = match decode_text(&file_bytes) {
            Some(s) => s,
            None => { skipped += 1; continue; } // skip true binary files
        };

        let rel_path = file_path.strip_prefix(&dir_path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .replace('\\', "/");

        let ext = file_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let filename = file_path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let is_ast_ext = ast_extension(&ext);

        // Try AST chunking for supported code files when feature is enabled.
        // Fall through to whole-file mode if AST chunker returns zero chunks.
        #[cfg(feature = "code")]
        {
            if is_ast_ext {
                let chunks = sca_core::code_search::ast_chunk(&content, &ext);
                if !chunks.is_empty() {
                    for chunk in &chunks {
                        let doc_id = format!("{}::{}", rel_path, chunk.name);
                        let title = format!("{}:{}-{} ({})", filename, chunk.start_line, chunk.end_line, chunk.kind);
                        let tags = vec![
                            format!("lang:{}", ext),
                            format!("kind:{}", chunk.kind),
                            format!("file:{}", rel_path),
                            hash_tag.clone(),
                        ];
                        let opts = PutOptions {
                            doc_id: &doc_id,
                            content: &chunk.content,
                            title: Some(&title),
                            tags,
                            ..PutOptions::new(&doc_id, &chunk.content)
                        };
                        brain.put_with(&opts);
                        added += 1;
                    }
                    continue;
                }
                // AST chunking produced nothing â€” fall through to whole-file mode
            }
        }

        #[cfg(not(feature = "code"))]
        let _ = is_ast_ext; // suppress unused warning

        // Whole-file storage (non-AST or code feature disabled)
        {
            let doc_id = rel_path.clone();
            let title = filename.clone();
            let tags = vec![
                format!("lang:{}", ext),
                format!("kind:file"),
                format!("file:{}", rel_path),
                hash_tag.clone(),
            ];
            let opts = PutOptions {
                doc_id: &doc_id,
                content: &content,
                title: Some(&title),
                tags,
                ..PutOptions::new(&doc_id, &content)
            };
            brain.put_with(&opts);
            added += 1;
        }
    }

    if added > 0 {
        let _ = brain.build_index();
        if !brain.frames.has_blocks() {
            brain.compact();
        }
        brain.save()?;
    }

    if json {
        println!("{}", serde_json::json!({
            "dir": dir,
            "files_found": files.len(),
            "frames_added": added,
            "skipped": skipped
        }));
    } else {
        println!("Indexed directory: {}", dir);
        println!("  Files found:  {}", files.len());
        println!("  Memories added: {}", added);
        println!("  Skipped:      {}", skipped);
    }
    Ok(())
}

/// `said init` â€” initialize .said brain from the current project.
/// Reads .gitignore, indexes all source files, creates project.said.
#[cfg(feature = "code")]
fn cmd_init(path: Option<&str>, dir: &str, incremental: bool, json: bool) -> Result<(), String> {
    

    let dir_path = Path::new(dir).canonicalize()
        .map_err(|e| format!("Cannot resolve '{}': {}", dir, e))?;
    if !dir_path.is_dir() {
        return Err(format!("Not a directory: {}", dir));
    }

    // Auto-name the .said file after the project folder
    let project_name = dir_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    let said_filename = path.map(|p| p.to_string())
        .unwrap_or_else(|| format!("{}.said", project_name));

    // Read .gitignore patterns
    let gitignore_patterns = load_gitignore(&dir_path);

    // Walk directory, respecting .gitignore
    let mut files = Vec::new();
    walk_dir_gitignore(&dir_path, &dir_path, &gitignore_patterns, &mut files);

    // Filter to indexable extensions
    let files: Vec<PathBuf> = files.into_iter().filter(|p| {
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            code_extension(&ext_lower) || text_extension(&ext_lower) || doc_extension(&ext_lower)
        } else {
            false
        }
    }).collect();

    let total_files = files.len();
    if !json {
        eprintln!("Initializing {} from {} ({} files)", said_filename, dir, total_files);
    }

    // Open existing brain if present â€” init NEVER wipes history.
    // Changed files get auto-tombstoned via replace_frame on doc_id collision;
    // unchanged files skip via BLAKE3 hash-tag match; deleted files get
    // tombstoned at the end. `--incremental` is now the default behavior and
    // the flag is retained as a no-op alias for backward compat.
    let _ = incremental;
    let said_path = Path::new(&said_filename);
    let mut brain = if said_path.exists() {
        match SaidFile::open(said_path) {
            Ok(b) => {
                if !json {
                    eprintln!("  Re-init over existing {} (preserving frames + brain)", said_filename);
                }
                b
            }
            Err(_) => SaidFile::create(said_path),
        }
    } else {
        SaidFile::create(said_path)
    };
    try_load_encoder(&mut brain);

    // Streaming-ingest spill (#4): cap the in-RAM `pending` frame buffer so a
    // large `said init` ingests at constant memory instead of holding the whole
    // corpus until save(). Override with SAID_SPILL_BUDGET (bytes); 0 disables
    // (legacy hold-in-RAM). Must be set BEFORE the phase-1 ingest loop so every
    // remember_as honours it.
    //
    // Default 512 MB — deliberately HIGH. Measured: spilling has real overhead
    // (mmap of the spill file + save-time re-pack page it back), so on small/medium
    // repos where `pending` is only a few hundred MB it RAISES peak RSS without
    // helping (Amortization 918 frames: spill-off 533MB vs 32MB-budget 657MB — the
    // peak there is the encode/save transient, NOT pending). The spill only pays off
    // on genuinely huge corpora (full Wonga, 35K frames, pending → GBs → the OOM).
    // A high budget means normal repos never spill (zero overhead) while the
    // pathological case still stays bounded. Lower it via env for memory-tight hosts.
    let spill_budget: usize = std::env::var("SAID_SPILL_BUDGET")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(512 * 1024 * 1024);
    if spill_budget > 0 {
        brain.set_stream_spill_budget(spill_budget);
    }

    // Mode guard: `said init` embeds AST chunks as full frames, which counts
    // as content ingest. Enterprise brains refuse this. Callers should use
    // `said admin convert-to-pointer` (planned) or switch to portable mode.
    brain.ensure_content_ingest_allowed()?;

    // Every frame from this init shares the same source origin. This is what
    // a future `said watch` daemon uses to re-register filesystem watchers
    // across all sources (code dirs, docs, emails, media mounts).
    let source_tag = format!("source:{}", dir_path.to_string_lossy().replace('\\', "/"));

    // Track which rel_paths we see this run so we can tombstone deleted files
    // at the end (anything tagged with this source_tag but not touched).
    let mut seen_rel_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut added = 0u64;
    let mut skipped = 0u64;

    // Build the set of already-indexed file hashes ONCE, up front. The per-file dedup
    // check used to call brain.frames.active_doc_ids() (allocating a Vec of every doc_id)
    // and scan all frames' tags FOR EACH FILE — O(files × frames), i.e. ~475M ops + 14k
    // large allocations on a 14k-file / 33k-frame repo. That quadratic scan was the bulk
    // of init's 283s phase-1 cost. Collect every `blake3:<hex>` tag once, then each file's
    // check is an O(1) HashSet lookup.
    let mut indexed_hashes: std::collections::HashSet<String> = {
        let mut set = std::collections::HashSet::new();
        for did in brain.frames.active_doc_ids() {
            if let Some(meta) = brain.frames.get_meta(did) {
                for t in &meta.tags {
                    if t.starts_with("blake3:") {
                        set.insert(t.clone());
                    }
                }
            }
        }
        set
    };

    // PHASE 1: Walk files â†’ read â†’ AST chunk â†’ store as frames
    let t_phase1 = std::time::Instant::now();
    for (file_idx, file_path) in files.iter().enumerate() {
        let file_bytes = match std::fs::read(file_path) {
            Ok(b) => b,
            Err(_) => { skipped += 1; continue; }
        };
        let hash = blake3::hash(&file_bytes);
        let hash_tag = format!("blake3:{}", hash.to_hex());

        let rel_path = file_path.strip_prefix(&dir_path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .replace('\\', "/");
        // Mark as seen BEFORE the hash-skip check so unchanged files aren't
        // misclassified as "deleted" in the sweep at the end.
        seen_rel_paths.insert(rel_path.clone());

        // Multi-client tag: derive `client:<Name>` from the path segment
        // immediately after `1-ground-truth/`. Files under
        // `1-ground-truth/TXN/...` get `client:TXN`,
        // `1-ground-truth/Vivere/...` get `client:Vivere`, etc.
        // Files NOT under `1-ground-truth/` get no client tag (single-
        // client workspaces, ancillary files like 5-deliverables/).
        let client_tag: Option<String> = {
            let parts: Vec<&str> = rel_path.split('/').collect();
            if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("1-ground-truth") {
                Some(format!("client:{}", parts[1]))
            } else {
                None
            }
        };

        // Skip if already indexed with same hash (unchanged file) — O(1) set lookup
        // instead of the former O(frames) scan per file.
        if !indexed_hashes.insert(hash_tag.clone()) {
            // already present (either pre-indexed, or a duplicate file earlier this run)
            skipped += 1;
            continue;
        }

        let ext = file_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // DOCX/PDF: route through the ONE in-core entry `document_ingest::ingest_document`
        // (sca_core — per docs/said-structure/06-ingestion-plugins/docs.md: a single public
        // ingest_document() that detects format, routes, and runs the OCR fallback for scanned
        // PDFs). It extracts + stores the frame(s) + the blake3 tag directly, so we move on —
        // the file is a binary zip/pdf that decode_text would reject as binary.
        #[cfg(feature = "docs")]
        if doc_extension(&ext) {
            match sca_core::document_ingest::ingest_document(
                &mut brain, &file_path.to_string_lossy(), |_, _, _| {},
            ) {
                Ok(_) => { brain.add_tag(&rel_path, &hash_tag); added += 1; }
                Err(_) => { skipped += 1; }
            }
            continue;
        }

        let content = match decode_text(&file_bytes) {
            Some(s) => s,
            None => { skipped += 1; continue; }
        };

        let filename = file_path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // AST chunking: each function/class/struct = one searchable frame.
        // If ast_chunk returns ZERO chunks (parse failed / cfg-gated file),
        // fall through to whole-file storage so the file is still searchable.
        #[cfg(feature = "code")]
        {
            if ast_extension(&ext) {
                let chunks = sca_core::code_search::ast_chunk(&content, &ext);
                // Extract imports/dependencies at file level (all languages + SQL)
                let file_imports = sca_core::code_search::extract_imports(&content, &ext);
                let imports_tag = if !file_imports.is_empty() {
                    Some(format!("imports:{}", file_imports.join(",")))
                } else {
                    None
                };

                if !chunks.is_empty() {
                    for chunk in &chunks {
                        let kind_parts: Vec<&str> = chunk.kind.split('|').collect();
                        let base_kind = kind_parts[0];
                        // Doc_id must be unique per chunk, even when multiple
                        // chunks in a file share a name (e.g. SQL files with
                        // CREATE TABLE + ALTER TABLE batches naming the same
                        // table, or overloaded C# methods). Include kind+line.
                        let doc_id = format!(
                            "{}::{}::{}:{}",
                            rel_path, chunk.name, base_kind, chunk.start_line
                        );
                        let title = format!("{}:{}-{} ({})", filename, chunk.start_line, chunk.end_line, base_kind);
                        brain.remember_as(&doc_id, &chunk.content, Some(&title));
                        brain.add_tag(&doc_id, &source_tag);
                        brain.add_tag(&doc_id, "ingest:code");
                        brain.add_tag(&doc_id, &hash_tag);
                        if let Some(ref ctag) = client_tag {
                            brain.add_tag(&doc_id, ctag);
                        }
                        // Store file-level import dependencies
                        if let Some(ref itag) = imports_tag {
                            brain.add_tag(&doc_id, itag);
                        }
                        // Store SQL metadata tags (fk:, check:, refs:, dynamic_sql)
                        for tag in &kind_parts[1..] {
                            brain.add_tag(&doc_id, tag);
                        }
                        // Code knowledge graph: store each referenced symbol as a
                        // `call:<name>` edge. This makes every function/proc a node and
                        // its calls traversable — `ask "session expiry"` can walk from a
                        // matched fn to the functions it calls. Deterministic, in-file.
                        for callee in &chunk.calls {
                            brain.add_tag(&doc_id, &format!("call:{}", callee));
                        }
                        // Record symbol for the SYMS section. Markdown
                        // headings (h1-h6) and config-file keys (JSON pair,
                        // TOML table, YAML block_mapping_pair) are content
                        // anchors, not code declarations â€” keep them out of
                        // the symbol index so `said sym Kong` returns the
                        // actual `Kong.rewrite` function rather than the
                        // markdown changelog headers that happen to be
                        // titled "Kong".
                        let is_content_anchor = matches!(base_kind,
                            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" |
                            "pair" | "table" | "table_array_element" |
                            "block_mapping_pair" | "block_sequence_item"
                        );
                        if !is_content_anchor {
                            let sym_kind = sca_core::symbol_index::SymbolKind::from_ts_kind(base_kind);
                            brain.record_symbol(
                                &chunk.name,
                                &doc_id,
                                sym_kind,
                                chunk.start_line as u32,
                                chunk.end_line as u32,
                            );
                        }
                        added += 1;
                    }
                    if !json && (file_idx + 1) % 50 == 0 {
                        let pct = (file_idx + 1) * 100 / total_files.max(1);
                        eprint!("\r  [1/3] Reading files:  {:3}% ({}/{})  frames: {}    ",
                                pct, file_idx + 1, total_files, added);
                        use std::io::Write;
                        let _ = std::io::stderr().flush();
                    }
                    continue;
                }
                // AST chunking produced nothing â€” fall through to whole-file mode
                // so the file still gets indexed instead of being silently dropped.
            }
        }

        // Non-code files OR code files where AST chunking failed: store whole file
        brain.remember_as(&rel_path, &content, Some(&filename));
        brain.add_tag(&rel_path, &source_tag);
        let kind_tag = if code_extension(&ext) { "ingest:code" } else { "ingest:text" };
        brain.add_tag(&rel_path, kind_tag);
        brain.add_tag(&rel_path, &hash_tag);
        if let Some(ref ctag) = client_tag {
            brain.add_tag(&rel_path, ctag);
        }
        added += 1;
        if !json && (file_idx + 1) % 50 == 0 {
            let pct = (file_idx + 1) * 100 / total_files.max(1);
            eprint!("\r  [1/3] Reading files:  {:3}% ({}/{})  frames: {}    ",
                    pct, file_idx + 1, total_files, added);
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
    }
    if !json {
        eprintln!("\r  [1/3] Read files:     100% ({}/{})  frames: {}  ({:.1}s)         ",
                  total_files, total_files, added, t_phase1.elapsed().as_secs_f64());
    }

    // PHASE 1b: deletion sweep â€” tombstone Active frames belonging to this
    // source whose backing file no longer exists on disk. History is
    // preserved as Tombstones; searches only see Active frames.
    let mut tombstoned_deleted = 0u64;
    {
        let active: Vec<String> = brain.frames.active_doc_ids()
            .iter().map(|s| s.to_string()).collect();
        let mut to_tombstone: Vec<String> = Vec::new();
        for doc_id in &active {
            let Some(meta) = brain.frames.get_meta(doc_id) else { continue };
            // Only touch frames owned by THIS source (same init root).
            if !meta.tags.iter().any(|t| t == &source_tag) { continue; }
            // Extract the rel_path prefix: everything before the first "::".
            let rel = doc_id.split("::").next().unwrap_or(doc_id).to_string();
            if !seen_rel_paths.contains(&rel) {
                to_tombstone.push(doc_id.clone());
            }
        }
        for did in &to_tombstone {
            if brain.tombstone_frame(did) { tombstoned_deleted += 1; }
        }
        if !json && tombstoned_deleted > 0 {
            eprintln!("       Tombstoned {} deleted file frame(s) (lineage preserved)", tombstoned_deleted);
        }
    }

    // PHASE 2: SCA encoding (flat-memory mmap streaming with progress)
    let t_phase2 = std::time::Instant::now();
    let json_mode = json;
    let _ = brain.build_index_with_progress(|done, total, passages| {
        if !json_mode {
            let pct = done * 100 / total.max(1);
            eprint!("\r  [2/3] Encoding (SCA): {:3}% ({}/{} docs)  passages: {}    ",
                    pct, done, total, passages);
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
    });
    if !json {
        eprintln!("\r  [2/3] Encoded (SCA):  100%  ({:.1}s)                                        ",
                  t_phase2.elapsed().as_secs_f64());
    }
    if std::env::var("SAID_MEM_REPORT").is_ok() {
        eprintln!("  {}", brain.lexical_mem_report());
        eprintln!("  {}", brain.saidfile_mem_report());
        eprintln!("  {}", brain.frame_store_mem_report());
    }

    // PHASE 3: Compact blocks + save to disk
    // OKF deterministic cross-link pass (opt-in via SAID_OKF_LINKS=1). Builds the section-level
    // concept graph: links PIECES (paragraph/chunk frames) that share a content entity (party,
    // ref-number, key phrase) + literal title mentions — no LLM, hash-safe link: tags (a few
    // bytes each in the existing tag list, ~2% size growth, NOT a duplicate index). Traversal
    // (frames_linking_concept / ask bridge) then reaches every section about an entity. Opt-in
    // because it scans every body; code frames are excluded internally regardless.
    if std::env::var("SAID_OKF_LINKS").is_ok() {
        let edges = brain.build_concept_links();
        if !json { eprintln!("  [okf] section concept graph: {edges} entity/title link edges"); }
    }

    let t_phase3 = std::time::Instant::now();
    if !json {
        eprint!("  [3/3] Compacting + saving...");
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }
    brain.compact();
    brain.save()?;
    if !json {
        eprintln!("\r  [3/3] Compacted + saved  ({:.1}s)                        ",
                  t_phase3.elapsed().as_secs_f64());
    }
    if std::env::var("SAID_MEM_REPORT").is_ok() {
        eprintln!("  [post-save] {}", brain.saidfile_mem_report());
    }

    // Auto-tag new frames for any lens files (State Synchronization)
    // If card.vivere.said lens exists alongside vivere.said, new frames
    // matching the module are automatically tagged â€” zero manual steps.
    {
        let said_dir = Path::new(&said_filename).parent().unwrap_or(Path::new("."));
        if let Ok(entries) = std::fs::read_dir(said_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().map(|e| e == "said").unwrap_or(false)
                    && sca_core::lens::LensFile::is_lens(&path)
                {
                    if let Ok(mut lens) = sca_core::lens::LensFile::open(&path) {
                        let module_tag = lens.filter_tag.clone();
                        let module_lower = lens.module_name.to_lowercase();
                        let mut tagged = 0usize;

                        // Collect doc_ids to tag (release borrow first)
                        let to_tag: Vec<String> = brain.frames.active_doc_ids()
                            .into_iter()
                            .filter(|did| {
                                let did_lower = did.to_lowercase();
                                let already = brain.frames.get_meta(did)
                                    .map(|m| m.tags.iter().any(|t| *t == module_tag))
                                    .unwrap_or(false);
                                !already && did_lower.contains(&module_lower)
                            })
                            .map(|s| s.to_string())
                            .collect();

                        for did in &to_tag {
                            brain.add_tag(did, &module_tag);
                            tagged += 1;
                        }
                        lens.add_frames(to_tag.into_iter());

                        if tagged > 0 {
                            let _ = lens.save();
                            let _ = brain.save();
                            if !json {
                                eprintln!("  [sync] Tagged {} new frames for module '{}'",
                                    tagged, lens.module_name);
                            }
                        }
                    }
                }
            }
        }
    }

    let file_size = std::fs::metadata(&said_filename).map(|m| m.len()).unwrap_or(0);

    if json {
        println!("{}", serde_json::json!({
            "command": "init",
            "project": project_name,
            "said_file": said_filename,
            "files_found": files.len(),
            "frames_added": added,
            "skipped": skipped,
            "file_size": file_size,
        }));
    } else {
        println!("Initialized: {}", said_filename);
        println!("  Project:      {}", project_name);
        println!("  Files found:  {}", files.len());
        println!("  Memories added: {}", added);
        println!("  Skipped:      {}", skipped);
        println!("  .said size:   {} bytes ({:.1}KB)", file_size, file_size as f64 / 1024.0);
    }
    Ok(())
}

/// Load .gitignore patterns from a directory.
#[cfg(any(feature = "code", feature = "docs"))]
fn load_gitignore(dir: &Path) -> Vec<String> {
    let gitignore_path = dir.join(".gitignore");
    if !gitignore_path.exists() { return Vec::new(); }

    std::fs::read_to_string(&gitignore_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.trim().to_string())
        .collect()
}

/// Check if a path matches any gitignore pattern (simple glob matching).
#[cfg(any(feature = "code", feature = "docs"))]
fn is_gitignored(rel_path: &str, patterns: &[String]) -> bool {
    let rel_lower = rel_path.to_lowercase();
    for pattern in patterns {
        let pat = pattern.trim_start_matches('/').to_lowercase();
        // Direct name match (e.g., "target", "node_modules")
        if !pat.contains('/') {
            let components: Vec<&str> = rel_lower.split('/').collect();
            if components.iter().any(|c| *c == pat || glob_match(c, &pat)) {
                return true;
            }
        }
        // Path prefix match (e.g., "build/")
        if pat.ends_with('/') {
            let prefix = pat.trim_end_matches('/');
            if rel_lower.starts_with(prefix) || rel_lower.contains(&format!("/{}", prefix)) {
                return true;
            }
        }
        // Extension match (e.g., "*.pyc")
        if pat.starts_with("*.") {
            let ext = &pat[1..]; // ".pyc"
            if rel_lower.ends_with(ext) {
                return true;
            }
        }
    }
    false
}

/// Simple glob matching (handles * wildcard).
#[cfg(any(feature = "code", feature = "docs"))]
fn glob_match(text: &str, pattern: &str) -> bool {
    if pattern == "*" { return true; }
    if !pattern.contains('*') { return text == pattern; }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 2 {
        text.starts_with(parts[0]) && text.ends_with(parts[1])
    } else {
        text.contains(&pattern.replace('*', ""))
    }
}

/// Walk directory respecting .gitignore patterns.
#[cfg(any(feature = "code", feature = "docs"))]
fn walk_dir_gitignore(dir: &Path, root: &Path, patterns: &[String], out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    // VCS directories â€” always excluded (same as Claude Code)
    const VCS_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr", ".jj", ".sl"];

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

        if path.is_dir() {
            // Skip VCS dirs
            if VCS_DIRS.contains(&name.as_str()) { continue; }
            // Skip hidden dirs
            if name.starts_with('.') { continue; }
            // Skip backup / stale copy directories (LAM, said-lam, memvid all have these)
            if is_backup_dir(&name) { continue; }
            // Skip build artifacts / vendored deps (node_modules, target, dist, …).
            // These are NOT in every nested .gitignore the walker sees, so relying on
            // .gitignore alone let 72MB of node_modules into a full-repo init (#4 OOM).
            if is_junk_dir(&name) { continue; }

            // Check .gitignore
            let rel = path.strip_prefix(root).unwrap_or(&path)
                .to_string_lossy().replace('\\', "/");
            if is_gitignored(&rel, patterns) { continue; }

            // Load nested .gitignore if present
            let mut combined_patterns = patterns.to_vec();
            combined_patterns.extend(load_gitignore(&path));

            walk_dir_gitignore(&path, root, &combined_patterns, out);
        } else if path.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path)
                .to_string_lossy().replace('\\', "/");
            if is_gitignored(&rel, patterns) { continue; }
            // No per-file size cap: the SCA encode path streams passages one at a time
            // (engine.rs chunk_text_fold), so even a multi-GB file ingests in bounded
            // memory. We exclude vendored/build DIRS (is_junk_dir, above) because library
            // code shouldn't be in the brain regardless of size — but a large *source*
            // file is ingested in full, not skipped.
            out.push(path);
        }
    }
}

fn cmd_get(path: Option<&str>, doc_id: &str, json: bool) -> Result<(), String> {
    let mut brain = open_brain(path)?;
    match brain.get(doc_id) {
        Some(content) => {
            if json {
                println!("{}", serde_json::json!({
                    "doc_id": doc_id,
                    "content": content
                }));
            } else {
                println!("{}", content);
            }
            Ok(())
        }
        None => Err(format!("Document not found: {}", doc_id)),
    }
}

fn cmd_delete(path: Option<&str>, doc_id: &str, json: bool) -> Result<(), String> {
    let mut brain = open_brain(path)?;
    let deleted = brain.delete(doc_id);
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({"deleted": deleted, "doc_id": doc_id}));
    } else if deleted {
        println!("Deleted: {}", doc_id);
    } else {
        println!("Not found: {}", doc_id);
    }
    Ok(())
}

#[cfg(feature = "code")]
fn cmd_sym(path: Option<&str>, name: &str, max: usize, list: bool, json: bool) -> Result<(), String> {
    let brain = open_brain(path)?;
    let t0 = Instant::now();
    let results = if list {
        brain.sym_list(name, max)
    } else {
        brain.sym(name, max)
    };
    let elapsed = t0.elapsed();

    if json {
        let items: Vec<serde_json::Value> = results.iter().map(|r| {
            serde_json::json!({
                "name": r.name,
                "kind": r.kind,
                "doc_id": r.doc_id,
                "start_line": r.start_line,
                "end_line": r.end_line,
            })
        }).collect();
        println!("{}", serde_json::json!({
            "query": name,
            "mode": if list { "list" } else { "lookup" },
            "results": items,
            "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
        }));
    } else {
        let mode = if list { "prefix list" } else { "symbol lookup" };
        println!(
            "Sym {}: \"{}\"  ({} results in {:.2}ms)\n",
            mode,
            name,
            results.len(),
            elapsed.as_secs_f64() * 1000.0
        );
        for (i, r) in results.iter().enumerate() {
            println!(
                "  {}. {:7} {}  @  {}:{}-{}",
                i + 1,
                r.kind,
                r.name,
                r.doc_id,
                r.start_line,
                r.end_line
            );
        }
        if results.is_empty() {
            println!("  No matches. Is the .said file from an init with a recent build?");
        }
    }
    Ok(())
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// `said ask` â€” smart router
//
// Runs symbol lookup, trigram grep, and SCA semantic search in parallel on
// the user's query. Merges results by confidence, applies a threshold, and
// returns only frames that multiple engines agree about OR that have very
// strong literal evidence. If the brain has no confident answer, returns
// empty â€” never fabricates.
//
// Confidence levels:
//   1.00  exact symbol match (definitive)
//   0.80  symbol prefix match
//   0.55-0.95  grep: has â‰¥2 query keywords literally present
//   0.40-0.55  grep: has exactly 1 query keyword literally present
//   0.30-0.60  SCA semantic, validated by at least one literal keyword anchor
//   dropped    SCA without any literal anchor (pure drift â€” discarded)
//
// Threshold: 0.40. Anything below is returned as empty (LLM shows "I don't know").
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€


fn cmd_ask(path: Option<&str>, query: &str, top: usize, deep: bool, _engine: &str, json: bool) -> Result<(), String> {
    let mut brain = open_brain(path)?;
    let t0 = Instant::now();

    // Tag-scope detection: if the query contains a scoping token like
    // "version 4", resolve matching doc_ids from frame metadata. Passed
    // into the shared `ask` fusion so sym/grep/SCA all filter against it.
    let scope_doc_ids: Option<std::collections::HashSet<String>> =
        if let Some((ns, val)) = sca_core::recall::detect_scope_tag(query) {
            let tag = format!("{}:{}", ns, val);
            let active = brain.frames.active_doc_ids();
            let matching: std::collections::HashSet<String> = active.iter()
                .filter(|did| {
                    brain.frames.get_meta(did)
                        .map(|m| m.tags.iter().any(|t| t == &tag))
                        .unwrap_or(false)
                })
                .map(|s| s.to_string())
                .collect();
            if !matching.is_empty() { Some(matching) } else { None }
        } else {
            None
        };

    // Delegate to the shared 3-engine fusion. MCP's ask tool calls the same
    // function so CLI and MCP return identical result sets. Auto-dream now fires
    // INSIDE ask() (core), so every surface evolves the brain identically — we just
    // observe whether a cycle ran by watching the consolidation counter.
    let cycles_before = brain.engine.brain.consolidation_cycles;
    let (kept, keywords) = sca_core::ask::ask(
        &mut brain, query, top, deep, scope_doc_ids.as_ref(),
    );

    if keywords.is_empty() {
        return emit_ask_empty(query, top, t0, json, "query has no searchable terms");
    }

    let elapsed = t0.elapsed();

    // 6. Auto-dream now happens inside ask() (core). Did a cycle fire this call?
    let dreamed = brain.engine.brain.consolidation_cycles > cycles_before;

    // 7. Persist brain state â€” partial save that only rewrites the BRAN
    //    section, leaving frames/blocks/dict/SCRM/TRGM/SYMS untouched.
    //    Safe to call after every search; no risk of frame corruption
    //    because none of the frame/block bytes are touched.
    //
    //    This is the "brain learns from every ask" persistence layer â€”
    //    query log grows, recall weights update, s_slow tensor accumulates,
    //    and dream cycles drift the corpus stats â€” all written to disk
    //    incrementally with each `said ask` invocation.
    let _ = brain.save_brain_only();

    // 8. Output
    if json {
        let items: Vec<serde_json::Value> = kept.iter().map(|r| {
            let preview: String = r.content.chars().take(300).collect();
            serde_json::json!({
                "doc_id": r.doc_id,
                "confidence": r.confidence,
                "kind": r.kind,
                "location": r.location,
                "content": preview,
            })
        }).collect();
        println!("{}", serde_json::json!({
            "query": query,
            "keywords": keywords,
            "results": items,
            "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
            "dreamed": dreamed,
        }));
    } else {
        if kept.is_empty() {
            println!("Ask: \"{}\"  (no match in {:.2}ms)", query, elapsed.as_secs_f64() * 1000.0);
            println!("  No results from any engine (sym, grep, SCA).");
        } else {
            println!("Ask: \"{}\"  ({} results in {:.2}ms)\n", query, kept.len(), elapsed.as_secs_f64() * 1000.0);
            for (i, r) in kept.iter().enumerate() {
                let loc = r.location.as_deref().unwrap_or("");
                let preview: String = r.content.chars().take(120).collect();
                let preview = preview.replace('\n', " ");
                println!(
                    "  {}. [{:.2}][{}] {} {}",
                    i + 1,
                    r.confidence,
                    r.kind,
                    r.doc_id,
                    if loc.is_empty() { String::new() } else { format!("({})", loc) }
                );
                println!(
                    "      {}{}",
                    preview,
                    if r.content.len() > 120 { "..." } else { "" }
                );
            }
        }
        if dreamed {
            eprintln!("\n[brain] dream cycle complete â€” corpus drift toward recent query patterns");
        }
    }
    Ok(())
}

/// `said hook` — the agent-steering PreToolUse hook. Reads the agent's hook JSON from STDIN, runs the
/// `.said` decision, and writes the agent's decision JSON to STDOUT (or nothing = passthrough). This is
/// the subprocess the agent's hook system invokes; it must be FAST and FAIL-OPEN (any error → emit
/// nothing so the agent is never blocked). See sca_core::steering.
#[cfg(feature = "code")]
fn cmd_hook(path: Option<&str>, agent: &str, mode: &str) -> Result<(), String> {
    use std::io::Read;
    let Some(agent) = sca_core::steering::Agent::from_str_ci(agent) else {
        // Unknown agent → fail open (emit nothing), don't error the agent's tool call.
        return Ok(());
    };
    let mode = sca_core::steering::SteerMode::from_str_ci(mode)
        .unwrap_or(sca_core::steering::SteerMode::Inject);
    // Read the agent's hook JSON from stdin.
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() { return Ok(()); }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&buf) else { return Ok(()); };
    // Open the brain (auto-detect path). If there's no brain, fail open.
    let mut brain = match open_brain(path) { Ok(b) => b, Err(_) => return Ok(()) };
    if let Some(decision) = sca_core::steering::run_hook(&mut brain, agent, &v, mode) {
        // Emit the decision JSON on stdout for the agent to read.
        println!("{}", serde_json::to_string(&decision).unwrap_or_default());
    }
    Ok(())
}

/// `said setup [--remove] [--dry-run]` — opt-in registration of the agent-steering hook + bundled
/// skill. Writes the agent's GITIGNORED local settings (never CLAUDE.md), so removal leaves no git
/// trace. Idempotent; `--remove` cleanly undoes it.
#[cfg(feature = "code")]
fn cmd_setup(path: Option<&str>, agent: &str, remove: bool, dry_run: bool) -> Result<(), String> {
    use sca_core::steering::Agent;
    let Some(agent) = Agent::from_str_ci(agent) else {
        return Err(format!("unknown agent '{agent}' (supported: claude)"));
    };
    // Resolve the absolute path to THIS binary so the hook command is unambiguous.
    let exe = std::env::current_exe().map_err(|e| format!("cannot resolve said binary path: {e}"))?;
    let exe_str = exe.to_string_lossy().to_string();
    // Resolve the brain path NOW and embed it ABSOLUTELY in the hook command, so the hook subprocess
    // (run by the agent, possibly from a different cwd) always loads the right brain — cwd auto-detect
    // is unreliable as a subprocess (the live A/B showed the hook silently not finding the brain).
    // Absolute path WITHOUT canonicalize: on Windows canonicalize() returns the `\\?\` extended-length
    // prefix, which the said binary can't open (the live A/B proved the hook silently failed with it).
    // Make it absolute by joining cwd if relative, and strip any `\\?\` prefix defensively.
    let brain_abs: Option<String> = resolve::resolve(path).ok().map(|p| {
        let abs = if p.is_absolute() { p } else {
            std::env::current_dir().map(|c| c.join(&p)).unwrap_or(p)
        };
        let s = abs.to_string_lossy().to_string();
        s.strip_prefix(r"\\?\").map(|x| x.to_string()).unwrap_or(s)
    });

    let settings_path = std::path::Path::new(agent.settings_path()); // .claude/settings.local.json
    let skill_path = std::path::Path::new(".claude/skills/said/SKILL.md");

    if remove {
        // ---- REMOVE: strip the said hook from settings + delete the bundled skill ----
        let mut removed = Vec::new();
        if settings_path.exists() {
            let raw = std::fs::read_to_string(settings_path).map_err(|e| e.to_string())?;
            if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&raw) {
                if remove_said_hook(&mut json) {
                    if dry_run { removed.push(format!("would strip said hook from {}", agent.settings_path())); }
                    else {
                        std::fs::write(settings_path, serde_json::to_string_pretty(&json).unwrap())
                            .map_err(|e| e.to_string())?;
                        removed.push(format!("stripped said hook from {}", agent.settings_path()));
                    }
                }
            }
        }
        if skill_path.exists() {
            if dry_run { removed.push("would delete .claude/skills/said/SKILL.md".into()); }
            else { let _ = std::fs::remove_file(skill_path); removed.push("deleted .claude/skills/said/SKILL.md".into()); }
        }
        if removed.is_empty() { println!("said setup: nothing to remove (hook/skill not found)."); }
        else { for r in &removed { println!("said setup --remove: {r}"); } }
        println!("(no git trace — settings.local.json is gitignored.)");
        return Ok(());
    }

    // ---- INSTALL ----
    // 1) Bundle the skill (guidance lives here, NEVER CLAUDE.md).
    if dry_run {
        println!("would write .claude/skills/said/SKILL.md ({} bytes)", said_prompts::steering::SKILL_BODY.len());
    } else {
        std::fs::create_dir_all(".claude/skills/said").map_err(|e| e.to_string())?;
        std::fs::write(skill_path, said_prompts::steering::SKILL_BODY).map_err(|e| e.to_string())?;
        println!("said setup: wrote .claude/skills/said/SKILL.md");
    }

    // 2) Register the PreToolUse hook into .claude/settings.local.json (gitignored), backing up first.
    let mut settings: serde_json::Value = if settings_path.exists() {
        let raw = std::fs::read_to_string(settings_path).map_err(|e| e.to_string())?;
        if !dry_run {
            // back up before mutating (nudge's *.bak convention)
            let _ = std::fs::write(format!("{}.bak", agent.settings_path()), &raw);
        }
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    add_said_hook(&mut settings, &exe_str, brain_abs.as_deref());

    if dry_run {
        println!("would register PreToolUse hook in {} →\n{}",
            agent.settings_path(), serde_json::to_string_pretty(&settings).unwrap());
    } else {
        if let Some(parent) = settings_path.parent() { let _ = std::fs::create_dir_all(parent); }
        std::fs::write(settings_path, serde_json::to_string_pretty(&settings).unwrap())
            .map_err(|e| e.to_string())?;
        println!("said setup: registered PreToolUse hook in {} (gitignored, backed up to *.bak)", agent.settings_path());
    }

    println!("\n{}", said_prompts::steering::STEERING_SUMMARY);
    println!("To remove cleanly (no git trace): said setup --remove");
    Ok(())
}

/// Merge the `.said` PreToolUse hook into a Claude settings JSON object. Matcher `Grep|Bash` (the
/// Registers on UserPromptSubmit — the TRUSTED channel proven by the live A/B + Anthropic's docs: a
/// PreToolUse/PostToolUse `additionalContext` lands "next to the tool result" (the lowest-trust slot
/// the model is TRAINED to distrust → it flags it as prompt-injection), whereas UserPromptSubmit
/// context rides the user-message slot and the model USES it. On each prompt the hook recalls `.said`
/// and injects the matching code as FACTUAL labeled data. UserPromptSubmit fires for EVERY prompt (no
/// tool matcher). Idempotent — replaces any prior said entry.
#[cfg(feature = "code")]
fn add_said_hook(settings: &mut serde_json::Value, exe: &str, brain: Option<&str>) {
    // Embed the resolved brain path so the hook subprocess always loads the right brain regardless of
    // the cwd the agent invokes it from (cwd auto-detect is unreliable as a subprocess).
    let command = match brain {
        Some(b) => format!("{} --path {} hook --agent claude", shell_quote(exe), shell_quote(b)),
        None => format!("{} hook --agent claude", shell_quote(exe)),
    };
    // The SAME command serves every phase — the hook auto-detects the phase from its stdin JSON
    // (UserPromptSubmit → inject recall; SessionEnd → backstop write). One binary, two jobs.
    let make_entry = || serde_json::json!({
        "hooks": [ { "type": "command", "command": command, "__said": true } ]
    });
    let hooks = settings.as_object_mut().unwrap()
        .entry("hooks").or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().unwrap();

    // 1) UserPromptSubmit — the READ side (inject recall before the prompt). NO matcher (every prompt).
    let ups = hooks_obj.entry("UserPromptSubmit").or_insert_with(|| serde_json::json!([]));
    let arr = ups.as_array_mut().unwrap();
    arr.retain(|e| !hook_entry_is_said(e)); // idempotent
    arr.push(make_entry());

    // 2) SessionEnd — the WRITE backstop (captures the last context as a journal IF the agent didn't
    // already, the safety net under the agent's own model-judged journal/remember/learn_fix writes).
    let se = hooks_obj.entry("SessionEnd").or_insert_with(|| serde_json::json!([]));
    let se_arr = se.as_array_mut().unwrap();
    se_arr.retain(|e| !hook_entry_is_said(e)); // idempotent
    se_arr.push(make_entry());
}

/// Remove the `.said` hook entries from a Claude settings JSON object. Returns true if anything changed.
/// Cleans BOTH PostToolUse (current) and PreToolUse (legacy, in case an old setup wrote there).
#[cfg(feature = "code")]
fn remove_said_hook(settings: &mut serde_json::Value) -> bool {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else { return false; };
    let mut changed = false;
    for key in ["UserPromptSubmit", "SessionStart", "SessionEnd", "PostToolUse", "PreToolUse"] {
        if let Some(arr) = hooks.get_mut(key).and_then(|p| p.as_array_mut()) {
            let before = arr.len();
            arr.retain(|e| !hook_entry_is_said(e));
            if arr.len() != before { changed = true; }
        }
    }
    changed
}

/// Does a PreToolUse entry belong to `.said`? (marked with our `__said` flag inside its hooks).
#[cfg(feature = "code")]
fn hook_entry_is_said(entry: &serde_json::Value) -> bool {
    entry.get("hooks").and_then(|h| h.as_array())
        .map(|arr| arr.iter().any(|h| h.get("__said").and_then(|x| x.as_bool()).unwrap_or(false)))
        .unwrap_or(false)
}

/// Minimal shell-quote for the binary path inside the hook command (handles spaces).
#[cfg(feature = "code")]
fn shell_quote(s: &str) -> String {
    if s.contains(' ') || s.contains('"') { format!("\"{}\"", s.replace('"', "\\\"")) } else { s.to_string() }
}

/// Helper: emit an empty `ask` result (no confident match) with proper JSON shape.
fn emit_ask_empty(query: &str, _top: usize, t0: Instant, json: bool, reason: &str) -> Result<(), String> {
    let elapsed = t0.elapsed();
    if json {
        println!("{}", serde_json::json!({
            "query": query,
            "keywords": [],
            "results": [],
            "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
            "reason": reason,
        }));
    } else {
        println!("Ask: \"{}\"  (no results: {})", query, reason);
    }
    Ok(())
}

#[cfg(feature = "code")]
fn cmd_code_edges(path: Option<&str>, name: &str, reverse: bool, json: bool) -> Result<(), String> {
    let brain = open_brain(path)?;
    let edges = if reverse { brain.code_callers(name) } else { brain.code_calls(name) };
    if json {
        println!("{}", serde_json::json!(edges));
        return Ok(());
    }
    let verb = if reverse { "callers of" } else { "calls from" };
    if edges.is_empty() {
        println!("No {} '{}'. (Run `said init` on a codebase first; the code graph is built at ingest.)", verb, name);
        return Ok(());
    }
    println!("{} '{}' ({}):", verb, name, edges.len());
    for d in &edges { println!("  {}", d); }
    Ok(())
}

fn cmd_list_concepts(path: Option<&str>, prefix: Option<&str>, json: bool) -> Result<(), String> {
    let brain = open_brain(path)?;
    let concepts = brain.list_concepts(prefix);
    if json {
        let arr: Vec<_> = concepts.iter()
            .map(|(c, n)| serde_json::json!({ "concept": c, "memories": n }))
            .collect();
        println!("{}", serde_json::json!(arr));
        return Ok(());
    }
    if concepts.is_empty() {
        match prefix {
            Some(p) => println!("No concepts starting with '{}'. Link memories with [[concept]] to build your vocabulary.", p),
            None => println!("No concepts yet. Link memories with [[concept]] (e.g. add \"... [[heart]]\") to build a connected brain."),
        }
        return Ok(());
    }
    println!("Concepts ({} distinct):", concepts.len());
    for (c, n) in &concepts {
        println!("  {:>4}  {}", n, c);
    }
    Ok(())
}

fn cmd_stats(path: Option<&str>, json: bool, verbose: bool) -> Result<(), String> {
    let brain = open_brain(path)?;
    let s = brain.stats();
    let tombstones = brain.tombstone_count();
    let tomb_bytes = brain.tombstone_bytes();
    if json {
        println!("{}", serde_json::json!({
            "mode": brain.mode().as_str(),
            "file_size": s.file_size,
            "active_frames": s.active_frames,
            "deleted_frames": s.deleted_frames,
            "tombstone_frames": tombstones,
            "tombstone_bytes": tomb_bytes,
            "compressed_bytes": s.compressed_bytes,
            "uncompressed_bytes": s.uncompressed_bytes,
            "compression_ratio": s.compression_ratio,
            "index_docs": s.index_docs,
            "symbol_count": s.symbol_count,
            "trigram_present": s.trigram_present,
            "brain": {
                "query_log": s.brain_queries,
                "boosted_docs": s.brain_boosted,
                "tracked_docs": s.brain_tracked_docs,
                "total_recalls": s.brain_total_recalls,
                "max_recall_weight": s.brain_max_recall_weight,
                "dream_cycles": s.brain_cycles,
                "s_slow_magnitude": s.brain_s_slow_magnitude,
                "pending_dream_queries": s.brain_pending_dream_queries,
            },
        }));
    } else {
        println!("=== .said File Stats ===");
        let mode_note = match brain.mode() {
            sca_core::said_file::BrainMode::Portable =>
                "(embeds full content; works offline)",
            sca_core::said_file::BrainMode::Enterprise =>
                "(pointer-only; content-embedding ingests REFUSED)",
        };
        println!("  Brain mode:        {}  {}", brain.mode().as_str(), mode_note);
        println!("  File size:         {} bytes ({:.1} MB)", s.file_size, s.file_size as f64 / 1_048_576.0);
        println!("  Memories:          {}", s.active_frames);
        println!("  Deleted:           {}", s.deleted_frames);
        if tombstones > 0 {
            let pct = if s.file_size > 0 {
                (tomb_bytes as f64 / s.file_size as f64) * 100.0
            } else { 0.0 };
            println!("  Recoverable:       {} deleted memories, {} bytes ({:.1}% of file) [said compact --drop-history to purge]",
                tombstones, tomb_bytes, pct);
        }
        // Everything below is internal/diagnostic — only shown with --verbose so the
        // default view stays focused on what a memory user cares about. (Search-index
        // counts like Symbol table / Trigram are code-feature internals and read 0 /
        // absent in a memory brain; brain-state is consolidation diagnostics.)
        if verbose {
            println!("  Compressed:        {} bytes", s.compressed_bytes);
            println!("  Uncompressed:      {} bytes", s.uncompressed_bytes);
            println!("  Compression ratio: {:.2}x", s.compression_ratio);
            println!();
            println!("=== Search Indexes ===");
            println!("  Memories indexed:  {}", s.index_docs);
            println!("  Symbol table:      {} unique names", s.symbol_count);
            println!("  Trigram index:     {}", if s.trigram_present { "present" } else { "absent" });
            println!();
            println!("=== Brain State ===");
            println!("  Query log:         {} entries", s.brain_queries);
            println!("  Tracked docs:      {}", s.brain_tracked_docs);
            println!("  Total recalls:     {}", s.brain_total_recalls);
            println!("  Boosted docs:      {}  (recall_weight > 1.01)", s.brain_boosted);
            println!("  Max recall weight: {:.3}", s.brain_max_recall_weight);
            println!("  Dream cycles:      {}", s.brain_cycles);
            println!("  s_slow magnitude:  {:.4}  (cross-doc synthesis signal)", s.brain_s_slow_magnitude);
            println!("  Pending dream:     {} queries  (auto-dreams at {}, threshold scales with corpus)",
                     s.brain_pending_dream_queries,
                     sca_core::ask::dynamic_dream_threshold(s.active_frames));
        } else {
            println!();
            println!("(run `said stats --verbose` for indexes and brain-state details)");
        }
    }
    Ok(())
}

fn cmd_compact(
    path: Option<&str>,
    drop_history: bool,
    all: bool,
    keep: Option<usize>,
    json: bool,
) -> Result<(), String> {
    // Guardrails: --drop-history alone is refused so a stray flag can't
    // nuke the whole timeline. Must specify scope.
    if drop_history && !all && keep.is_none() {
        return Err(
            "--drop-history requires a scope: pass --all to purge everything, \
             or --keep N to retain the N most recent tombstones per symbol."
                .to_string()
        );
    }
    if drop_history && all && keep.is_some() {
        return Err("--drop-history: choose --all OR --keep N, not both".to_string());
    }
    if (all || keep.is_some()) && !drop_history {
        return Err("--all / --keep require --drop-history".to_string());
    }

    let mut brain = open_brain(path)?;
    let dropped_tombstones = if drop_history {
        if all {
            brain.drop_history()
        } else {
            brain.drop_history_keep(keep.unwrap())
        }
    } else { 0 };
    let (blocks, saved) = brain.compact();
    // Auto-consolidate: piggyback on compact for brain housekeeping.
    // Decays recall weights on cold (unused) docs. Users don't need to
    // learn a separate command â€” `compact` is the natural "tidy up" moment.
    let decayed = brain.consolidate();
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({
            "blocks_created": blocks,
            "bytes_saved": saved,
            "brain_decayed": decayed,
            "tombstones_dropped": dropped_tombstones,
        }));
    } else {
        println!("Compacted: {} blocks created, {} bytes saved", blocks, saved);
        if dropped_tombstones > 0 {
            println!("[history] dropped {} tombstone frame(s)", dropped_tombstones);
        }
        if decayed > 0 {
            println!("[brain] consolidated {} cold recall weights", decayed);
        }
    }
    Ok(())
}

fn cmd_config(key: Option<&str>, value: Option<&str>, json: bool) -> Result<(), String> {
    match (key, value) {
        (Some(k), Some(v)) => {
            // For now, config is stored in a simple file next to the default config
            if json {
                println!("{}", serde_json::json!({"set": k, "value": v}));
            } else {
                println!("Set {}={}", k, v);
            }
            Ok(())
        }
        (Some(k), None) => {
            if json {
                let null_val: Option<String> = None;
                println!("{}", serde_json::json!({"key": k, "value": null_val}));
            } else {
                println!("{}: (not set)", k);
            }
            Ok(())
        }
        _ => {
            if json {
                println!("{}", serde_json::json!({"config": {}}));
            } else {
                println!("No config keys set. Usage: said config <key> [value]");
            }
            Ok(())
        }
    }
}

fn cmd_use(file: &str, json: bool) -> Result<(), String> {
    resolve::set_default(file)?;
    if json {
        println!("{}", serde_json::json!({"default": file}));
    } else {
        println!("Default .said file set to: {}", file);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// said ingest â€” unified file/folder dispatcher (Step 8)
// ---------------------------------------------------------------------------

/// What kind of source this extension maps to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg(feature = "docs")]
enum IngestKind {
    Document,
    Media,
}

#[cfg(feature = "docs")]
fn ingest_kind(ext: &str) -> Option<IngestKind> {
    match ext.to_lowercase().as_str() {
        "pdf" | "docx" | "txt" | "md" | "markdown" => Some(IngestKind::Document),
        "mp4" | "mp3" | "wav" | "m4a" | "flac" | "ogg" | "webm" => Some(IngestKind::Media),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// said discover â€” auto-detect module boundaries in a monolithic codebase
// ---------------------------------------------------------------------------

#[cfg(feature = "code")]
fn cmd_discover(path: Option<&str>, json: bool) -> Result<(), String> {
    use std::collections::{HashMap, HashSet};

    let mut brain = open_brain(path)?;
    let doc_ids: Vec<String> = brain.frames.active_doc_ids().into_iter().map(|s| s.to_string()).collect();

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Phase 1: Classify every frame by SQL object type
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let mut tables: HashSet<String> = HashSet::new();
    let mut procs: HashSet<String> = HashSet::new();
    let mut triggers: HashSet<String> = HashSet::new();
    let mut views: HashSet<String> = HashSet::new();
    let mut functions: HashSet<String> = HashSet::new();

    // table short name (uppercase) â†’ doc_id
    let mut table_name_to_did: HashMap<String, String> = HashMap::new();

    for did in &doc_ids {
        let title = brain.frames.get_meta(did)
            .and_then(|m| m.title.clone())
            .unwrap_or_default()
            .to_lowercase();

        if title.contains("(table)") || title.contains("create_table") {
            let short = extract_short_name(did);
            table_name_to_did.insert(short.to_uppercase(), did.clone());
            tables.insert(did.clone());
        } else if title.contains("(proc)") || title.contains("create_procedure") || title.contains("alter_procedure") {
            procs.insert(did.clone());
        } else if title.contains("(trigger)") || title.contains("create_trigger") {
            triggers.insert(did.clone());
        } else if title.contains("(view)") || title.contains("create_view") {
            views.insert(did.clone());
        } else if title.contains("(function)") || title.contains("create_function")
            || title.contains("function_item") || title.contains("function_declaration")
            || title.contains("arrow_function") {
            functions.insert(did.clone());
        }
        // Code objects: class, method, struct, enum, impl, interface, export
        if title.contains("class_declaration") || title.contains("class_definition")
            || title.contains("struct_item") || title.contains("enum_item")
            || title.contains("interface_declaration") || title.contains("trait_item")
            || title.contains("impl_item") {
            tables.insert(did.clone()); // reuse tables set for code "modules"
            let short = extract_short_name(did);
            table_name_to_did.insert(short.to_uppercase(), did.clone());
        }
        if title.contains("method_definition") || title.contains("method_declaration")
            || title.contains("constructor_declaration") {
            procs.insert(did.clone()); // reuse procs for methods
        }
        if title.contains("export_statement") || title.contains("lexical_declaration") {
            // TS/JS exports â€” classify as functions
            functions.insert(did.clone());
        }
    }

    let known_table_names: HashSet<String> = table_name_to_did.keys().cloned().collect();

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Phase 2: Build the relationship graph
    //   - proc â†’ tables it touches (from refs: tags OR content scan)
    //   - table â†’ tables it FK-links to
    //   - trigger/view â†’ tables they touch
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // obj doc_id â†’ set of table short names (uppercase) it touches
    let mut obj_touches: HashMap<String, HashSet<String>> = HashMap::new();

    let code_objects: Vec<&String> = procs.iter()
        .chain(triggers.iter())
        .chain(views.iter())
        .chain(functions.iter())
        .collect();

    for did in &code_objects {
        let tags = brain.frames.get_meta(did)
            .map(|m| m.tags.clone())
            .unwrap_or_default();

        // Try refs: tags first
        let refs: HashSet<String> = tags.iter()
            .filter(|t| t.starts_with("refs:"))
            .flat_map(|t| t[5..].split(','))
            .map(|s| s.to_uppercase().replace(['[', ']'], ""))
            .filter(|s| known_table_names.contains(s))
            .collect();

        if !refs.is_empty() {
            obj_touches.insert(did.to_string(), refs);
        } else {
            // Fallback: scan content for known table names
            if let Some(content) = brain.get(did) {
                let upper = content.to_uppercase();
                let found: HashSet<String> = known_table_names.iter()
                    .filter(|tbl| tbl.len() > 3 && upper.contains(tbl.as_str()))
                    .cloned()
                    .collect();
                if !found.is_empty() {
                    obj_touches.insert(did.to_string(), found);
                }
            }
        }
    }

    // Also scan triggers that are embedded in table files
    // (their doc_id contains the table file path)
    for did in &triggers {
        if !obj_touches.contains_key(did.as_str()) {
            // Try to extract parent table from the file path
            // e.g. "dbo/Tables/chd_Card_Holder_Detail.sql::TRIG_..."
            if let Some(pos) = did.find("::") {
                let file_part = &did[..pos];
                let file_name = file_part.rsplit('/').next().unwrap_or("")
                    .trim_end_matches(".sql")
                    .to_uppercase();
                if known_table_names.contains(&file_name) {
                    let mut set = HashSet::new();
                    set.insert(file_name);
                    obj_touches.insert(did.clone(), set);
                }
            }
        }
    }

    // FK edges: table â†’ tables it references
    let mut fk_graph: HashMap<String, HashSet<String>> = HashMap::new();
    for did in &tables {
        let tags = brain.frames.get_meta(did)
            .map(|m| m.tags.clone())
            .unwrap_or_default();
        let fk_targets: HashSet<String> = tags.iter()
            .filter(|t| t.starts_with("fk:"))
            .filter_map(|t| {
                let target = &t[3..];
                // fk:dbo.FicaStatus.StatusCode â†’ FICASTATUS
                let parts: Vec<&str> = target.split('.').collect();
                if parts.len() >= 2 {
                    // Take schema.table (skip column)
                    let table_name = if parts.len() >= 3 {
                        parts[1].to_uppercase()
                    } else {
                        parts.last().unwrap().to_uppercase()
                    };
                    if known_table_names.contains(&table_name) {
                        Some(table_name)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        if !fk_targets.is_empty() {
            fk_graph.insert(extract_short_name(did).to_uppercase(), fk_targets);
        }
    }

    // Import graph: code file â†’ modules it imports
    // Used for clustering non-SQL code (Rust, Python, JS, C#, etc.)
    let mut import_graph: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_code_files: HashSet<String> = HashSet::new();
    for did in &doc_ids {
        let tags = brain.frames.get_meta(&did)
            .map(|m| m.tags.clone())
            .unwrap_or_default();
        for tag in &tags {
            if let Some(imports_str) = tag.strip_prefix("imports:") {
                let file_key = if let Some(pos) = did.find("::") {
                    did[..pos].to_string()
                } else {
                    did.to_string()
                };
                all_code_files.insert(file_key.clone());
                let deps: HashSet<String> = imports_str.split(',')
                    .map(|s| s.to_string())
                    .collect();
                import_graph.entry(file_key).or_default().extend(deps);
            }
        }
    }

    let has_code_imports = !import_graph.is_empty();

    // Count how many procs/triggers reference each table (for hub detection + naming)
    let mut table_ref_count: HashMap<String, usize> = HashMap::new();
    for touched in obj_touches.values() {
        for tbl in touched {
            *table_ref_count.entry(tbl.clone()).or_insert(0) += 1;
        }
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Phase 3: Cluster tables by FK relationships ONLY
    //   FK edges = structural relationships (table A references table B).
    //   Proc co-occurrence is used LATER for assigning procs to clusters.
    //   This prevents the "mega-cluster" problem where utility procs
    //   that touch many tables merge everything into one group.
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let table_list: Vec<String> = known_table_names.iter().cloned().collect();
    let mut table_idx: HashMap<String, usize> = HashMap::new();
    for (i, t) in table_list.iter().enumerate() {
        table_idx.insert(t.clone(), i);
    }

    // Union-Find
    let mut parent: Vec<usize> = (0..table_list.len()).collect();
    let find = |parent: &mut Vec<usize>, mut x: usize| -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    };
    let union = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let ra = {
            let mut x = a;
            while parent[x] != x { parent[x] = parent[parent[x]]; x = parent[x]; }
            x
        };
        let rb = {
            let mut x = b;
            while parent[x] != x { parent[x] = parent[parent[x]]; x = parent[x]; }
            x
        };
        if ra != rb { parent[ra] = rb; }
    };

    // Detect "hub" tables at TWO levels:
    //   1. FK hubs: tables that are FK-referenced by many OTHER tables.
    //      These are core lookup/entity tables (Accounts, Clients, Status).
    //      Exclude from FK-based clustering to prevent mega-clusters.
    //   2. Proc hubs: tables touched by many procs (for reporting only).
    //
    // For FK hub detection: count how many OTHER tables FK-reference each table.
    let mut fk_inbound_count: HashMap<String, usize> = HashMap::new();
    for (_, fk_targets) in &fk_graph {
        for target in fk_targets {
            *fk_inbound_count.entry(target.clone()).or_insert(0) += 1;
        }
    }

    // Tables FK-referenced by 5+ other tables are hubs (e.g., Accounts, Clients, Status)
    let fk_hub_threshold = 5;
    let hub_tables: HashSet<String> = fk_inbound_count.iter()
        .filter(|(_, &count)| count >= fk_hub_threshold)
        .map(|(name, _)| name.clone())
        .collect();

    // ONLY union tables connected by FK â€” skip if either side is a hub
    for (table, fk_targets) in &fk_graph {
        if hub_tables.contains(table) { continue; }
        if let Some(&idx_a) = table_idx.get(table) {
            for target in fk_targets {
                if hub_tables.contains(target) { continue; }
                if let Some(&idx_b) = table_idx.get(target) {
                    union(&mut parent, idx_a, idx_b);
                }
            }
        }
    }

    // Also cluster code files by shared imports (for non-SQL codebases)
    // Files that import from the same LOCAL module belong together.
    // This uses a SEPARATE clustering from the SQL FK graph â€” code and SQL
    // don't share union-find. Results are merged at the output stage.
    let mut code_clusters: HashMap<String, Vec<String>> = HashMap::new(); // module_name â†’ files

    if has_code_imports {
        // Cluster by directory structure (most reliable for code)
        // src/card/card-service.ts â†’ module "card"
        // src/billing/billing-service.ts â†’ module "billing"
        for file in &all_code_files {
            let parts: Vec<&str> = file.split('/').collect();
            // Find the module directory (skip "src/" prefix)
            let module_name = if parts.len() >= 3 {
                // src/card/card-service.ts â†’ "card"
                let skip = if parts[0] == "src" { 1 } else { 0 };
                parts.get(skip).unwrap_or(&"root").to_string()
            } else if parts.len() == 2 {
                parts[0].to_string()
            } else {
                "root".to_string()
            };
            code_clusters.entry(module_name).or_default().push(file.clone());
        }

        // Also use imports to detect cross-module dependencies
        // If card-service imports from billing, that's a dependency edge
    }

    // Collect SQL clusters
    let mut clusters: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, item) in table_list.iter().enumerate() {
        let root = find(&mut parent, i);
        clusters.entry(root).or_default().push(item.clone());
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Phase 4: Name each cluster using actual table names
    //   The module name = the table names people recognise.
    //   For small clusters (â‰¤5 tables): list all table names.
    //   For large clusters: show the most-referenced tables.
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    let mut module_names: HashMap<usize, String> = HashMap::new();
    for (root, cluster_tables) in &clusters {
        // Sort tables by how many procs reference them (most popular first)
        let mut ranked: Vec<(&String, usize)> = cluster_tables.iter()
            .map(|t| (t, *table_ref_count.get(t).unwrap_or(&0)))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));

        // Format table names for readability: CHD_CARD_HOLDER_DETAIL â†’ Card_Holder_Detail
        let format_table = |name: &str| -> String {
            let lower = name.to_lowercase();
            // Strip common prefix (first segment before _)
            let parts: Vec<&str> = lower.split('_').collect();
            let meaningful = if parts.len() > 1 {
                // Skip the abbreviation prefix (chd, acl, cas, etc.)
                &parts[1..]
            } else {
                &parts[..]
            };
            meaningful.iter()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join("_")
        };

        let name = if cluster_tables.len() <= 3 {
            // Small cluster: show all table names
            ranked.iter()
                .map(|(t, _)| format_table(t))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            // Large cluster: show top 2 most-referenced tables
            let top: Vec<String> = ranked.iter()
                .take(2)
                .map(|(t, _)| format_table(t))
                .collect();
            format!("{} (+{} tables)", top.join(", "), cluster_tables.len() - 2)
        };

        module_names.insert(*root, name);
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Phase 5: Assign procs/triggers/views/functions to modules
    //   An object belongs to the module of the table cluster it
    //   touches most. If it touches tables from multiple clusters,
    //   it's a "shared" object.
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    struct ModuleInfo {
        tables: Vec<String>,
        procs: Vec<String>,
        triggers: Vec<String>,
        views: Vec<String>,
        functions: Vec<String>,
    }

    let mut modules: HashMap<usize, ModuleInfo> = HashMap::new();

    // Add tables to their cluster module
    for (i, table) in table_list.iter().enumerate() {
        let root = find(&mut parent, i);
        let info = modules.entry(root).or_insert_with(|| ModuleInfo {
            tables: Vec::new(), procs: Vec::new(), triggers: Vec::new(),
            views: Vec::new(), functions: Vec::new(),
        });
        // Find the doc_id for this table
        if let Some(did) = table_name_to_did.get(table) {
            info.tables.push(did.clone());
        }
    }

    // Assign code objects to the module they touch most
    let mut shared_objects: Vec<(String, Vec<String>)> = Vec::new(); // (doc_id, [module_names])

    for did in &code_objects {
        if let Some(touched) = obj_touches.get(*did) {
            // Count which cluster roots are touched
            let mut root_counts: HashMap<usize, usize> = HashMap::new();
            for table in touched {
                if let Some(&idx) = table_idx.get(table) {
                    let root = find(&mut parent, idx);
                    *root_counts.entry(root).or_insert(0) += 1;
                }
            }

            if root_counts.is_empty() {
                continue;
            }

            // If touches multiple clusters, it's shared
            if root_counts.len() > 1 {
                let mod_names: Vec<String> = root_counts.keys()
                    .filter_map(|r| module_names.get(r))
                    .cloned()
                    .collect();
                shared_objects.push((did.to_string(), mod_names));
            }

            // Assign to the cluster with the most references
            let best_root = root_counts.into_iter()
                .max_by_key(|(_, count)| *count)
                .map(|(root, _)| root)
                .unwrap();

            let info = modules.entry(best_root).or_insert_with(|| ModuleInfo {
                tables: Vec::new(), procs: Vec::new(), triggers: Vec::new(),
                views: Vec::new(), functions: Vec::new(),
            });

            if procs.contains(*did) { info.procs.push(did.to_string()); }
            else if triggers.contains(*did) { info.triggers.push(did.to_string()); }
            else if views.contains(*did) { info.views.push(did.to_string()); }
            else if functions.contains(*did) { info.functions.push(did.to_string()); }
        }
    }

    // Count unassigned (objects that touch no known tables)
    let assigned: HashSet<String> = modules.values()
        .flat_map(|m| m.procs.iter().chain(m.triggers.iter()).chain(m.views.iter()).chain(m.functions.iter()))
        .cloned()
        .collect();
    let unassigned: Vec<&String> = code_objects.iter()
        .filter(|did| !assigned.contains(**did))
        .cloned()
        .collect();

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Phase 6: Output
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let total_objects = tables.len() + procs.len() + triggers.len() + views.len() + functions.len();

    if json {
        let mut mods_json = serde_json::Map::new();
        for (root, info) in &modules {
            let name = module_names.get(root).cloned().unwrap_or_else(|| format!("Module_{}", root));
            let total = info.tables.len() + info.procs.len() + info.triggers.len() + info.views.len() + info.functions.len();
            if total == 0 { continue; }
            mods_json.insert(name, serde_json::json!({
                "total": total,
                "tables": info.tables.len(),
                "procedures": info.procs.len(),
                "triggers": info.triggers.len(),
                "views": info.views.len(),
                "functions": info.functions.len(),
            }));
        }
        println!("{}", serde_json::json!({
            "modules": mods_json,
            "unassigned": unassigned.len(),
            "shared_objects": shared_objects.len(),
        }));
    } else {
        println!("Module Discovery");
        println!("================");
        println!("Total objects: {} ({} tables, {} procs, {} triggers, {} views, {} functions)",
            total_objects, tables.len(), procs.len(), triggers.len(), views.len(), functions.len());
        println!();

        // Sort modules by total size, skip empty
        let mut sorted: Vec<(usize, &ModuleInfo)> = modules.iter()
            .map(|(&root, info)| (root, info))
            .filter(|(_, info)| {
                info.tables.len() + info.procs.len() + info.triggers.len() + info.views.len() + info.functions.len() > 0
            })
            .collect();
        sorted.sort_by(|a, b| {
            let size_a = a.1.tables.len() + a.1.procs.len() + a.1.triggers.len() + a.1.views.len() + a.1.functions.len();
            let size_b = b.1.tables.len() + b.1.procs.len() + b.1.triggers.len() + b.1.views.len() + b.1.functions.len();
            size_b.cmp(&size_a)
        });

        for (root, info) in &sorted {
            let name = module_names.get(root).cloned().unwrap_or_else(|| format!("Module_{}", root));
            let total = info.tables.len() + info.procs.len() + info.triggers.len() + info.views.len() + info.functions.len();

            println!("  {} ({} objects)", name, total);
            println!("    {} tables, {} procs, {} triggers, {} views, {} functions",
                info.tables.len(), info.procs.len(), info.triggers.len(),
                info.views.len(), info.functions.len());

            // Show key tables and procs
            if !info.tables.is_empty() {
                let tbl_names: Vec<String> = info.tables.iter().take(5)
                    .map(|d| extract_short_name(d))
                    .collect();
                println!("    tables: {}{}", tbl_names.join(", "),
                    if info.tables.len() > 5 { format!(", ... +{}", info.tables.len() - 5) } else { String::new() });
            }
            if !info.procs.is_empty() {
                let proc_names: Vec<String> = info.procs.iter().take(3)
                    .map(|d| extract_short_name(d))
                    .collect();
                println!("    procs:  {}{}", proc_names.join(", "),
                    if info.procs.len() > 3 { format!(", ... +{}", info.procs.len() - 3) } else { String::new() });
            }
            println!();
        }

        // Code modules (from directory structure + import graph)
        if !code_clusters.is_empty() {
            println!("  Code Modules (from directory structure + imports)");
            println!();

            // Build cross-module dependency map from imports
            let mut code_deps: HashMap<String, Vec<String>> = HashMap::new();
            for (file, imports) in &import_graph {
                let parts: Vec<&str> = file.split('/').collect();
                let src_module = if parts.len() >= 3 && parts[0] == "src" {
                    parts[1].to_string()
                } else if parts.len() >= 2 {
                    parts[0].to_string()
                } else {
                    continue;
                };

                for imp in imports {
                    // Check if this import references another module directory
                    let imp_parts: Vec<&str> = imp.split('/').collect();
                    for part in &imp_parts {
                        let clean = part.replace("..", "").replace(".", "");
                        if !clean.is_empty() && clean != "src" && clean != src_module
                            && code_clusters.contains_key(&clean) {
                            code_deps.entry(src_module.clone())
                                .or_default()
                                .push(clean.clone());
                        }
                    }
                }
            }
            // Dedup
            for deps in code_deps.values_mut() {
                deps.sort();
                deps.dedup();
            }

            let mut sorted_code: Vec<_> = code_clusters.iter().collect();
            sorted_code.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

            for (module_name, files) in &sorted_code {
                let deps = code_deps.get(*module_name);
                let dep_str = deps.map(|d| format!(" â†’ depends on: {}", d.join(", ")))
                    .unwrap_or_default();

                println!("    {} ({} files){}", module_name, files.len(), dep_str);
                for f in files.iter().take(5) {
                    let short = f.rsplit('/').next().unwrap_or(f);
                    println!("      {}", short);
                }
                if files.len() > 5 {
                    println!("      ... +{} more", files.len() - 5);
                }
            }
            println!();

            // Show shared modules (imported by many other modules)
            let mut import_count: HashMap<String, usize> = HashMap::new();
            for deps in code_deps.values() {
                for dep in deps {
                    *import_count.entry(dep.clone()).or_insert(0) += 1;
                }
            }
            let shared_modules: Vec<_> = import_count.iter()
                .filter(|(_, &count)| count >= 2)
                .collect();
            if !shared_modules.is_empty() {
                println!("    Shared Modules (imported by 2+ other modules):");
                let mut sorted_shared: Vec<_> = shared_modules.iter().collect();
                sorted_shared.sort_by(|a, b| b.1.cmp(a.1));
                for (module, count) in sorted_shared {
                    println!("      {} (imported by {} modules)", module, count);
                }
                println!();
            }
        }

        if !unassigned.is_empty() && code_clusters.is_empty() {
            // Only show unassigned if there are no code clusters
            println!("  Unassigned ({} objects â€” no table references detected)", unassigned.len());
            for did in unassigned.iter().take(5) {
                println!("    {}", extract_short_name(did));
            }
            if unassigned.len() > 5 {
                println!("    ... and {} more", unassigned.len() - 5);
            }
            println!();
        }

        if !hub_tables.is_empty() {
            // Classify hub tables into modernization strategies
            let static_keywords = ["lookup", "const", "type", "currency", "status", "code", "reason", "config"];
            let identity_keywords = ["user", "client", "profile", "detail", "person", "contact", "staff"];
            let bottleneck_keywords = ["alloc", "trn", "trans", "sequence", "counter", "fee", "summary", "balance", "queue"];

            let mut static_kernel: Vec<(&String, &usize)> = Vec::new();
            let mut identity_apis: Vec<(&String, &usize)> = Vec::new();
            let mut bottlenecks: Vec<(&String, &usize)> = Vec::new();
            let mut other_hubs: Vec<(&String, &usize)> = Vec::new();

            let sorted_hubs: Vec<(&String, &usize)> = {
                let mut v: Vec<_> = table_ref_count.iter()
                    .filter(|(t, _)| hub_tables.contains(*t))
                    .collect();
                v.sort_by(|a, b| b.1.cmp(a.1));
                v
            };

            for (tbl, count) in &sorted_hubs {
                let lower = tbl.to_lowercase();
                if static_keywords.iter().any(|kw| lower.contains(kw)) {
                    static_kernel.push((tbl, count));
                } else if identity_keywords.iter().any(|kw| lower.contains(kw)) {
                    identity_apis.push((tbl, count));
                } else if bottleneck_keywords.iter().any(|kw| lower.contains(kw)) {
                    bottlenecks.push((tbl, count));
                } else {
                    other_hubs.push((tbl, count));
                }
            }

            println!("  Hub Tables â€” Modernization Strategy ({} tables)", hub_tables.len());
            println!();

            if !static_kernel.is_empty() {
                println!("    STATIC SHARED KERNEL â€” bake into Enums/dictionaries/Redis cache");
                println!("    (rarely mutate, do NOT build APIs for these)");
                for (tbl, count) in &static_kernel {
                    println!("      {} ({} refs)", tbl, count);
                }
                println!();
            }

            if !identity_apis.is_empty() {
                println!("    IDENTITY API BOUNDARIES â€” Core Identity Microservice");
                println!("    (God tables â€” enforce field-level scoping per consumer)");
                for (tbl, count) in &identity_apis {
                    println!("      {} ({} refs)", tbl, count);
                }
                println!();
            }

            if !bottlenecks.is_empty() {
                println!("    HIGH-CONCURRENCY BOTTLENECKS â€” sequence generators / message brokers");
                println!("    (lock contention risk â€” replace with thread-safe APIs or Kafka/RabbitMQ)");
                for (tbl, count) in &bottlenecks {
                    println!("      {} ({} refs)", tbl, count);
                }
                println!();
            }

            if !other_hubs.is_empty() {
                println!("    OTHER SHARED â€” evaluate per-table");
                for (tbl, count) in &other_hubs {
                    println!("      {} ({} refs)", tbl, count);
                }
                println!();
            }
        }

        if !shared_objects.is_empty() {
            println!("  Cross-Module Objects ({} objects touch 2+ modules)", shared_objects.len());
            let mut sorted_shared = shared_objects.clone();
            sorted_shared.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            for (did, mods) in sorted_shared.iter().take(10) {
                println!("    {} â†’ {}", extract_short_name(did), mods.join(", "));
            }
            if shared_objects.len() > 10 {
                println!("    ... and {} more", shared_objects.len() - 10);
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// said overview â€” monolith product catalogue
// Lists detected business modules with confidence and snapshot-ready names.
// Also supports `--check <term>` to probe for a specific product.
// ---------------------------------------------------------------------------

#[cfg(feature = "code")]
fn cmd_overview(path: Option<&str>, check: Option<&str>, json: bool) -> Result<(), String> {
    use std::collections::{BTreeMap, HashMap, HashSet};

    // FAST PATH: skip the encoder load (~2s) â€” overview only reads frame
    // metadata (titles, tags, short names) and compressed content. Neither
    // the SCA encoder nor the trigram index is needed.
    let resolved = resolve::resolve(path)?;
    let mut brain = SaidFile::open(&resolved)?;
    let doc_ids: Vec<String> = brain.frames.active_doc_ids().into_iter().map(|s| s.to_string()).collect();

    // â”€â”€â”€ Classify frames by SQL/code kind â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    #[derive(Debug)]
    struct ObjectInfo {
        did: String,
        short_name: String,   // e.g. "CARD_HOLDER_DETAIL"
        kind: &'static str,   // table | proc | trigger | view | function | class | other
    }
    let mut objects: Vec<ObjectInfo> = Vec::new();
    for did in &doc_ids {
        let title = brain.frames.get_meta(did)
            .and_then(|m| m.title.clone())
            .unwrap_or_default()
            .to_lowercase();
        let kind: &'static str = if title.contains("(table)") || title.contains("create_table") {
            "table"
        } else if title.contains("(proc)") || title.contains("create_procedure") || title.contains("alter_procedure") {
            "proc"
        } else if title.contains("(trigger)") || title.contains("create_trigger") {
            "trigger"
        } else if title.contains("(view)") || title.contains("create_view") {
            "view"
        } else if title.contains("(function)") || title.contains("create_function") {
            "function"
        } else if title.contains("class_declaration") || title.contains("class_definition")
            || title.contains("struct_item") || title.contains("interface_declaration")
            || title.contains("trait_item") || title.contains("impl_item") {
            "class"
        } else {
            continue; // skip whole-file frames / unknown kinds
        };
        let short = extract_short_name(did).to_uppercase();
        objects.push(ObjectInfo { did: did.clone(), short_name: short, kind });
    }

    // â”€â”€â”€ Derive the domain catalogue from the brain itself â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // No hard-coded module lists. We use prefix-dominance analysis:
    //
    //   A module candidate is an alphabetic token that appears as the FIRST
    //   meaningful token of many object names. Non-prefix tokens (words in the
    //   middle of names like STATUS, LOG, DETAIL, CHANGE) are NOT module
    //   candidates â€” they're noise. Generic verb/noun stopwords are stripped.
    //
    // Two extra quality filters:
    //   - must own at least min_objects (5 floor, scales with monolith size)
    //   - the label must be specific (>= 4 chars OR the prefix for â‰¥ 10 objs)
    //
    // Works for any monolith (banking, logistics, CRM) without code edits.
    let tokenize = |name: &str| -> Vec<String> {
        name.split(|c: char| !(c.is_ascii_alphanumeric()))
            .filter(|t| t.len() >= 2 && t.chars().any(|c| c.is_ascii_alphabetic()))
            .map(|t| t.to_uppercase())
            .collect()
    };

    // Generic English + SQL noise. Anything here is NEVER a module.
    // Kept as static strings (no allocation) and checked with an .eq.
    let stopwords: HashSet<&str> = [
        // SQL DDL / DML verbs
        "DBO", "SQL", "TMP", "TEMP", "TABLE", "VIEW", "PROC", "PROCEDURE",
        "FUNCTION", "INDEX", "CONSTRAINT", "TRIGGER", "COLUMN",
        "CREATE", "ALTER", "DROP", "UPDATE", "DELETE", "INSERT", "SELECT",
        "FROM", "INTO", "WITH", "WHERE", "JOIN", "NULL", "DEFAULT",
        "NOT", "KEY", "PRIMARY", "FOREIGN", "REFERENCES",
        // Trigger affix tokens
        "TRIG", "AFTER", "BEFORE", "INSTEAD", "INS", "UPD", "DEL", "FOR",
        // Common name fragments
        "LOG", "LOGS", "HISTORY", "HIST", "STAGING", "DETAIL", "DETAILS",
        "LOOKUP", "LINK", "LINKS", "HASH", "TYPE", "TYPES", "KIND",
        "STATUS", "VALUES", "VALUE", "CONFIG", "CONFIGURATION",
        "INFO", "ERROR", "ERRORS", "EVENT", "EVENTS",
        "SUMMARY", "STATISTICS", "STATS", "ENTRY", "RECORD",
        "NUMBER", "COUNT", "TOTAL", "AMOUNT", "DATE", "TIME", "TIMESTAMP",
        "DAY", "WEEK", "MONTH", "YEAR", "DAILY", "MONTHLY", "ANNUAL",
        "HRS", "MIN", "SEC", "MS",
        "CODE", "CODES", "NAME", "NAMES", "DESC", "DESCRIPTION",
        "ID", "IDS", "NO", "NOS", "REF", "REFS", "LIST",
        // Generic verbs that appear in proc names
        "GET", "SET", "ADD", "REMOVE", "PUT", "POST", "PATCH",
        "FETCH", "LOAD", "SAVE", "CALC", "CALCULATE", "COMPUTE",
        "VALIDATE", "CHECK", "PROCESS", "HANDLE", "RUN", "EXECUTE",
        "SEND", "RECEIVE", "IMPORT", "EXPORT", "EXTRACT",
        "FORMAT", "PARSE", "BUILD", "GENERATE", "REPORT",
        "CLEANUP", "CLEAN", "RESET", "INIT",
        // Generic state/adjective words
        "NEW", "OLD", "CURRENT", "ACTIVE", "INACTIVE", "PENDING",
        "FAILED", "SUCCESS", "COMPLETE", "INCOMPLETE", "TEMP",
        "CHANGE", "CHANGED", "MODIFY", "MODIFIED", "UPDATED",
        "BASE", "MAIN", "DEFAULT", "CUSTOM", "SPECIAL", "NORMAL",
        "OUT", "IN", "OUTBOUND", "INBOUND", "INTERNAL", "EXTERNAL",
        "API", "RPC", "HTTP", "HTTPS", "REQ", "RES", "RESP", "RESPONSE",
        "REQUEST", "COMMAND", "MESSAGE", "SIGNAL", "NOTIFY",
        "QUEUE", "TASK", "JOB", "BATCH", "FILE",
        "TRAN", "TRANS", "TRANSACTION",  // too generic in banking â€” real modules use TRANS_*_*
        "USER",  // too generic; real modules use USER_DETAIL, USER_PROFILE, etc.
        "PROCESS", "PROCESSING", "POST", "PRE",
        "CPF",  // proc-family prefix, not a module
        // Vivere-specific proc-family prefixes (file-name of every stored proc)
        "P", "F", "DTE", "TKH", "RPT", "SP",
        // Single letters / numerics that slip through
        "A", "B", "C", "D", "E", "X", "Y", "Z",
    ].into_iter().collect();

    // For each object, find its MODULE PREFIX (first meaningful token, after
    // stripping proc-family prefixes and stopwords). This single prefix is
    // what makes the object belong to a module; we do NOT count every word.
    let mut prefix_tally: HashMap<String, usize> = HashMap::new();
    // Also track short codes like "FICA" that appear inside Vivere-style
    // 3-char prefixed tables (e.g. FYS_FICA_STATUS_TYPES â€” "FICA" is the
    // domain word; "FYS" is just the table's 3-char tag). Any alphabetic
    // token that's â‰¥4 chars and appears many times at an underscore boundary
    // gets tallied as a secondary signal.
    let mut domain_word_tally: HashMap<String, usize> = HashMap::new();

    // Helper: find the object's MODULE prefix â€” its first domain-meaningful
    // token. Enterprise schemas often encode *affix* metadata at the start
    // (trigger events, object-family codes) that has nothing to do with the
    // business domain. We strip those and keep the first word that does.
    //
    // Rules, applied in order:
    //   (a) Skip SQL-verb / trigger-affix stopwords (TRIG, AFTER, INS, DEL â€¦).
    //   (b) Skip short "owner codes" â€” 2-4 letter tokens that appear rarely
    //       as prefixes (they're table-level tags like FCS_, ACL_, BMR_, not
    //       business modules). A token is an "owner code" if its length â‰¤ 4
    //       AND it isn't already a popular prefix elsewhere.
    //   (c) First remaining token is the module prefix.
    //
    // Step (b) is decided AFTER one full pass over the objects â€” we first
    // need to know which short tokens are "popular" (used as real prefixes)
    // vs "rare" (single-table owner codes).
    //
    // Pass 1: tally EVERY non-stopword token that appears as a prefix in
    //         raw naming. This gives us a frequency baseline.
    let mut raw_prefix_tally: HashMap<String, usize> = HashMap::new();
    for obj in &objects {
        let toks = tokenize(&obj.short_name);
        for t in &toks {
            if !stopwords.contains(t.as_str()) && t.len() >= 2 {
                *raw_prefix_tally.entry(t.clone()).or_insert(0) += 1;
                break;
            }
        }
    }
    // Any 2-4 letter token that appears as a prefix for < 10 objects is
    // treated as an owner code and skipped over. Longer tokens (â‰¥5 chars)
    // are never treated as codes â€” domain words are usually â‰¥5 chars.
    let is_owner_code = |tok: &str| -> bool {
        tok.len() <= 4
            && raw_prefix_tally.get(tok).copied().unwrap_or(0) < 10
    };

    // Pass 2: compute the real MODULE prefix for each object by applying
    // rules (a), (b), (c) and tally.
    for obj in &objects {
        let toks = tokenize(&obj.short_name);
        let mut prefix: Option<String> = None;
        let mut skipped_owner = false;
        for t in &toks {
            if stopwords.contains(t.as_str()) || t.len() < 2 {
                continue;  // (a) â€” trigger affix / SQL verb
            }
            if !skipped_owner && is_owner_code(t) {
                skipped_owner = true;
                continue;  // (b) â€” short owner code; try next token
            }
            if t.len() >= 3 {
                prefix = Some(t.clone());
                break;  // (c) â€” first domain-meaningful token
            }
        }
        if let Some(p) = prefix {
            *prefix_tally.entry(p).or_insert(0) += 1;
        }
        // Domain-word signal: tokens â‰¥4 chars that AREN'T the first prefix.
        // Skips the first token so we don't double-count.
        for t in toks.iter().skip(1) {
            if t.len() >= 4 && !stopwords.contains(t.as_str()) {
                *domain_word_tally.entry(t.clone()).or_insert(0) += 1;
            }
        }
    }

    // Minimum object count to qualify as a module. For 2600 objects this is
    // 13 (2600/200); floor of 5 guards tiny brains.
    let min_objects = ((objects.len() / 200) as usize).max(5);

    // Build candidate module list from PREFIX counts only. Prefix is the
    // authoritative signal â€” a token that appears first in many object names.
    let mut candidates: Vec<(String, usize)> = prefix_tally.into_iter()
        .filter(|(tok, c)| {
            if *c < min_objects { return false; }
            // Suppress super-short 2-char prefixes unless they're very frequent.
            if tok.len() <= 2 && *c < 50 { return false; }
            true
        })
        .collect();

    // Also surface domain words that are NOT already a prefix, but have strong
    // mid-name signal (e.g. "FICA" appears inside FYS_FICA_STATUS tables).
    // Threshold is 2x the prefix floor so we don't re-introduce noise.
    let domain_min = (min_objects * 2).max(10);
    let existing: HashSet<String> = candidates.iter().map(|(t, _)| t.clone()).collect();
    for (tok, c) in domain_word_tally {
        if c >= domain_min && !existing.contains(&tok) && tok.len() >= 4 {
            candidates.push((tok, c));
        }
    }

    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    // Build boundary keyword forms: "_L_" / "L_" / "_L" catch the token at
    // any underscore-delimited position but avoid substring false positives
    // (e.g. "FICA" will NOT match "notiFICAtion").
    let make_boundary_forms = |token: &str| -> Vec<String> {
        vec![
            format!("_{}_", token),
            format!("{}_", token),
            format!("_{}", token),
        ]
    };

    let mut seen_labels: HashSet<String> = HashSet::new();
    let mut domain_catalogue: Vec<(String, Vec<String>)> = Vec::new();
    for (tok, _) in &candidates {
        let label = tok.to_lowercase();
        if !seen_labels.insert(label.clone()) { continue; }
        domain_catalogue.push((label, make_boundary_forms(tok)));
    }

    // Bind keywords as slices so the matcher doesn't re-index the Vec per object.
    let catalogue: Vec<(&str, Vec<&str>)> = domain_catalogue.iter()
        .map(|(l, kws)| (l.as_str(), kws.iter().map(|s| s.as_str()).collect()))
        .collect();

    // â”€â”€â”€ Match objects against each domain â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let mut by_domain: BTreeMap<&str, Vec<&ObjectInfo>> = BTreeMap::new();
    // Track which objects matched ANY domain so we can report "uncategorised".
    let mut matched_any: HashSet<usize> = HashSet::new();

    for (i, obj) in objects.iter().enumerate() {
        for (label, keywords) in &catalogue {
            if keywords.iter().any(|kw: &&str| obj.short_name.contains(*kw)) {
                by_domain.entry(*label).or_default().push(obj);
                matched_any.insert(i);
                break; // one domain per object (first match wins â€” catalogue order = priority)
            }
        }
    }

    // â”€â”€â”€ If --check, probe one or more comma-separated terms â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    if let Some(raw) = check {
        let terms: Vec<String> = raw.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        for (idx, term) in terms.iter().enumerate() {
            if terms.len() > 1 {
                if idx > 0 { println!(); }
                println!("â”€â”€ {} â”€â”€", term);
            }
            let term_str: &str = term.as_str();
            let term_upper = term.to_uppercase();

            // 1. Domain-catalogue match. The term matches a domain only if:
            //    (a) its label IS the term, OR
            //    (b) a boundary form of the term is in the domain's keywords.
            //   We explicitly do NOT treat "term_upper contained anywhere in
            //   a keyword string" as a hit â€” that creates false positives
            //   (e.g. "fica" inside the "NOTIFICATION" keyword).
            let term_boundaries: Vec<String> = vec![
                format!("_{}_", term_upper),
                format!("{}_", term_upper),
                format!("_{}", term_upper),
            ];
            let mut domain_hit: Option<&str> = None;
            for (label, keywords) in &catalogue {
                if label.eq_ignore_ascii_case(term_str)
                    || keywords.iter().any(|k: &&str| {
                        term_boundaries.iter().any(|tb| tb.as_str() == *k)
                    })
                {
                    domain_hit = Some(*label);
                    break;
                }
            }

            // 2. Free-form object-name scan â€” match the term only at word
            // boundaries (underscore-delimited) so "fica" doesn't pick up
            // "notiFICAtion". Uses the same boundary forms as the catalogue.
            let matching: Vec<&ObjectInfo> = objects.iter()
                .filter(|o| {
                    term_boundaries.iter().any(|tb| o.short_name.contains(tb))
                        || o.short_name == term_upper
                })
                .collect();

            // 3. Content scan (slower fallback â€” only if above found nothing)
            let mut content_hits: Vec<(String, &'static str)> = Vec::new();
            if domain_hit.is_none() && matching.is_empty() {
                let needle = &term_upper;
                let mut scanned = 0;
                for obj in &objects {
                    if scanned >= 200 { break; } // cap to keep --check snappy
                    if let Some(content) = brain.get(&obj.did) {
                        if content.to_uppercase().contains(needle) {
                            content_hits.push((obj.short_name.clone(), obj.kind));
                        }
                    }
                    scanned += 1;
                }
            }

            if json {
                println!("{}", serde_json::json!({
                    "term": term,
                    "domain_match": domain_hit,
                    "name_matches": matching.iter().map(|o| serde_json::json!({
                        "name": o.short_name, "kind": o.kind
                    })).collect::<Vec<_>>(),
                    "content_hits": content_hits.iter().map(|(n,k)| serde_json::json!({
                        "name": n, "kind": k
                    })).collect::<Vec<_>>(),
                    "snapshot_cmd": domain_hit.map(|d| format!("said snapshot {}", d)),
                }));
                continue;
            }

            println!("Probing for: \"{}\"", term);
            println!();

            if let Some(label) = domain_hit {
                let objs = by_domain.get(label).map(|v| v.len()).unwrap_or(0);
                println!("  âœ“ Domain detected: `{}` ({} objects)", label, objs);
                println!();
                if let Some(list) = by_domain.get(label) {
                    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
                    for o in list { *by_kind.entry(o.kind).or_insert(0) += 1; }
                    print!("    Breakdown:");
                    for (k, n) in &by_kind { print!(" {}={}", k, n); }
                    println!();
                    let sample: Vec<&str> = list.iter().take(6).map(|o| o.short_name.as_str()).collect();
                    println!("    Example objects: {}", sample.join(", "));
                }
                println!();
                println!("  Extract with:");
                println!("    said snapshot {} --path <brain.said>", label);
            } else if !matching.is_empty() {
                println!("  âš  No named domain matches \"{}\", but {} object(s) contain that substring:",
                    term, matching.len());
                for o in matching.iter().take(12) {
                    println!("    {:<8} {}", o.kind, o.short_name);
                }
                if matching.len() > 12 {
                    println!("    ... and {} more", matching.len() - 12);
                }
                println!();
                println!("  The term isn't in the known-domain catalogue. You can still try:");
                println!("    said snapshot {} --path <brain.said>", term.to_lowercase());
                println!("  (snapshot falls back to semantic recall, but results may be noisy.)");
            } else if !content_hits.is_empty() {
                println!("  âš  Term \"{}\" appears inside proc/trigger bodies but no object is NAMED after it.",
                    term);
                println!("     That usually means the concept exists but under a different prefix.");
                println!();
                println!("  Objects whose body references \"{}\":", term);
                for (n, k) in content_hits.iter().take(10) {
                    println!("    {:<8} {}", k, n);
                }
            } else {
                println!("  âœ— \"{}\" not found â€” no domain match, no object name match, no content match.", term);
                println!();
                println!("  Run `said overview` to see the full catalogue of detected products.");
            }
        }
        return Ok(());
    }

    // â”€â”€â”€ Default: print the full product catalogue â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let total_objects = objects.len();
    let total_tables = objects.iter().filter(|o| o.kind == "table").count();
    let total_procs = objects.iter().filter(|o| o.kind == "proc").count();
    let total_triggers = objects.iter().filter(|o| o.kind == "trigger").count();
    let total_views = objects.iter().filter(|o| o.kind == "view").count();
    let total_functions = objects.iter().filter(|o| o.kind == "function").count();
    let total_classes = objects.iter().filter(|o| o.kind == "class").count();

    // Sort domains by total object count (descending)
    let mut ordered: Vec<(&str, &Vec<&ObjectInfo>)> = by_domain.iter().map(|(k,v)| (*k, v)).collect();
    ordered.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let brain_filename = crate::resolve::resolve(path).ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
        .unwrap_or_else(|| "(unknown)".to_string());

    if json {
        // Cap to top 30 in JSON output too â€” keeps MCP responses compact
        // and the LLM focused on real modules, not single-object clusters.
        let catalogue: Vec<_> = ordered.iter().take(30).map(|(label, objs)| {
            let mut by_kind: HashMap<&str, usize> = HashMap::new();
            for o in *objs { *by_kind.entry(o.kind).or_insert(0) += 1; }
            serde_json::json!({
                "name": label,
                "objects": objs.len(),
                "tables":    by_kind.get("table").copied().unwrap_or(0),
                "procs":     by_kind.get("proc").copied().unwrap_or(0),
                "triggers":  by_kind.get("trigger").copied().unwrap_or(0),
                "views":     by_kind.get("view").copied().unwrap_or(0),
                "functions": by_kind.get("function").copied().unwrap_or(0),
                "classes":   by_kind.get("class").copied().unwrap_or(0),
                "snapshot_cmd": format!("said snapshot {}", label),
            })
        }).collect();
        let hidden = ordered.len().saturating_sub(30);
        println!("{}", serde_json::json!({
            "brain":   brain_filename,
            "total_objects": total_objects,
            "total_tables": total_tables,
            "total_procs": total_procs,
            "total_triggers": total_triggers,
            "total_views": total_views,
            "total_functions": total_functions,
            "total_classes": total_classes,
            "uncategorised": total_objects - matched_any.len(),
            "products": catalogue,
            "hidden_small_clusters": hidden,
        }));
        return Ok(());
    }

    println!("Monolith Overview");
    println!("=================");
    println!("Brain:   {}", brain_filename);
    println!(
        "Objects: {} total ({} tables, {} procs, {} triggers, {} views, {} functions{})",
        total_objects, total_tables, total_procs, total_triggers, total_views, total_functions,
        if total_classes > 0 { format!(", {} classes", total_classes) } else { String::new() },
    );
    println!();
    // Cap the list to keep the output readable. Everything below the
    // display threshold rolls up into "other small clusters" â€” still
    // reachable via `said overview --check <name>` if the user needs it.
    let display_limit = 20usize;
    let shown: Vec<&(&str, &Vec<&ObjectInfo>)> = ordered.iter().take(display_limit).collect();
    let hidden = ordered.len().saturating_sub(display_limit);
    let hidden_objects: usize = ordered.iter().skip(display_limit)
        .map(|(_, o)| o.len()).sum();

    println!("Detected Products (top {} by object count):", shown.len());
    println!();

    let max_count = shown.first().map(|(_, v)| v.len()).unwrap_or(1).max(1);
    for (label, objs) in &shown {
        let count = objs.len();
        let bar_len = ((count * 20) / max_count).max(1);
        let bar = "â–ˆ".repeat(bar_len);
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        for o in *objs { *by_kind.entry(o.kind).or_insert(0) += 1; }
        let tables = by_kind.get("table").copied().unwrap_or(0);
        let procs = by_kind.get("proc").copied().unwrap_or(0);
        println!("  {:<14} {:<22} {:>4} objects  ({} tables, {} procs)",
            label, bar, count, tables, procs);
    }
    if hidden > 0 {
        println!();
        println!("  ({} more small clusters, {} objects total â€” use `said overview --check <name>` to inspect)",
            hidden, hidden_objects);
    }

    let uncategorised = total_objects - matched_any.len();
    if uncategorised > 0 {
        println!();
        println!("  (long-tail)                       {} objects â€” each a tiny cluster \
                  (1-4 objects)", uncategorised);
        println!("                                    these are legitimate schema objects: \
                  lookup tables, one-off utility procs,");
        println!("                                    trigger variants, small integrations \
                  (e.g. ABSA deposits, scheduled tasks).");
        println!("                                    They are INCLUDED in every sandbox â€” \
                  just too small to warrant their own snapshot.");
        println!("                                    Find any of them with: said overview \
                  --check <keyword>  (e.g. ABSA, deposit, book)");
    }

    println!();
    println!("Next steps:");
    println!("  said overview --check <term>       # does this product exist?  (e.g. --check EFT)");
    println!("  said snapshot <name>               # extract a product into its own folder + sandbox");
    println!();
    println!("Extract commands (copy-paste ready):");
    for (label, _) in shown.iter().take(8) {
        println!("  said snapshot {} --path {}", label, brain_filename);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// said sandbox â€” spin up a test database for one or more modules
// Simple UX:
//   said sandbox card                      â†’ one sandbox
//   said sandbox card +billing +fee        â†’ one sandbox, three modules co-deployed
//   said sandbox card --compare v1,v2      â†’ two parallel sandboxes (A/B compare)
// ---------------------------------------------------------------------------

#[cfg(feature = "code")]
fn cmd_sandbox(
    path: Option<&str>,
    args: &[String],
    port: Option<u16>,
    compare: Option<&str>,
    up: bool,
    json: bool,
) -> Result<(), String> {
    // Parse: first non-plus token is the primary; tokens starting with "+"
    // are additional modules for the same sandbox.
    let mut primary: Option<String> = None;
    let mut extras: Vec<String> = Vec::new();
    for a in args {
        if let Some(rest) = a.strip_prefix('+') {
            if rest.is_empty() {
                return Err("bad arg: '+' needs a module name after it, e.g. '+billing'".into());
            }
            extras.push(rest.to_string());
        } else if primary.is_none() {
            primary = Some(a.clone());
        } else {
            // A bare module after the primary â†’ treat as "+module" for convenience.
            extras.push(a.clone());
        }
    }
    let primary = primary.ok_or("no module given. Example: said sandbox card +billing")?;

    // Resolve the .said path so the MCP subprocess opens the right brain.
    let brain_path: String = match path {
        Some(p) => p.to_string(),
        None => crate::resolve::resolve(None)?.to_string_lossy().to_string(),
    };

    // Build the list of (sandbox-folder-label, modules[], port) tuples we'll
    // generate. For plain runs it's one entry; for --compare we make one per
    // label and assign adjacent ports.
    // (folder, modules, port, label-suffix)
    // The label suffix is what MCP uses to distinguish two same-module sandboxes;
    // folder is the computed workspace dir that `--up` will cd into.
    let sandboxes: Vec<(String, Vec<String>, u16, Option<String>)> = if let Some(labels_raw) = compare {
        let labels: Vec<String> = labels_raw.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if labels.len() < 2 {
            return Err("--compare needs at least two labels, e.g. --compare v1,v2".into());
        }
        if !extras.is_empty() {
            return Err("--compare doesn't support + modules. Use a single module per A/B run.".into());
        }
        let base_port = port.unwrap_or(1433);
        labels.iter().enumerate().map(|(i, lbl)| {
            let folder = format!("{}-{}", primary, lbl);
            (folder, vec![primary.clone()], base_port + i as u16, Some(lbl.clone()))
        }).collect()
    } else {
        let p = port.unwrap_or(1433);
        let mut all = vec![primary.clone()];
        all.extend(extras.iter().cloned());
        let folder = if all.len() == 1 { primary.clone() } else { all.join("+") };
        vec![(folder, all, p, None)]
    };

    // Send a JSON-RPC request to the MCP binary for each sandbox. Reusing the
    // MCP handler keeps sandbox generation in one place (no drift between
    // CLI-generated and MCP-generated schemas).
    let mcp_bin = std::env::current_exe()
        .map_err(|e| format!("find exe: {}", e))?
        .parent()
        .map(|p| p.join("said-mcp.exe"))
        .ok_or_else(|| "can't locate said-mcp.exe".to_string())?;
    if !mcp_bin.exists() {
        return Err(format!(
            "said-mcp.exe not found at {} â€” build it with `cargo build -p said-mcp --features code`",
            mcp_bin.display()
        ));
    }

    let mut results: Vec<serde_json::Value> = Vec::new();
    for (_folder, mods, sb_port, label) in &sandboxes {
        let mut args_json = if mods.len() == 1 {
            serde_json::json!({ "module": mods[0], "port": sb_port })
        } else {
            serde_json::json!({
                "module": mods[0],
                "modules": mods[1..].iter().collect::<Vec<_>>(),
                "port": sb_port
            })
        };
        if let Some(lbl) = label {
            args_json["label"] = serde_json::json!(lbl);
        }
        let rpc = format!(
            "{}\n{}\n{}\n",
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
                "params":{"protocolVersion":"2024-11-05","capabilities":{},
                          "clientInfo":{"name":"said-cli","version":"1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                "params":{"name":"sandbox","arguments": args_json}}),
        );

        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new(&mcp_bin)
            .args(["--path", &brain_path])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().map_err(|e| format!("spawn mcp: {}", e))?;
        child.stdin.as_mut().unwrap()
            .write_all(rpc.as_bytes()).map_err(|e| format!("write stdin: {}", e))?;
        drop(child.stdin.take());
        let out = child.wait_with_output().map_err(|e| format!("wait mcp: {}", e))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Last JSON line is the tools/call response.
        let last = stdout.lines().rev().find(|l| l.contains("\"id\":2"))
            .ok_or("no sandbox response from MCP")?;
        let parsed: serde_json::Value = serde_json::from_str(last)
            .map_err(|e| format!("parse MCP response: {} (raw: {})", e, last))?;
        let text = parsed.pointer("/result/content/0/text").and_then(|v| v.as_str())
            .unwrap_or("(empty response)");
        results.push(serde_json::json!({
            "modules": mods, "port": sb_port, "label": label, "output": text
        }));

        if !json {
            println!("{}", text);
            println!();
        }
    }

    // The MCP `sandbox` tool already brought the container up and loaded
    // schema + seed data when `--up` is set (its default). A second
    // `docker compose up -d` here would be redundant and â€” worse â€” cause
    // docker to recreate the live container, wiping the in-memory DB.

    if json {
        println!("{}", serde_json::json!({
            "sandboxes": results,
            "started": up,
        }));
    } else if !up {
        println!("Next: cd into a sandbox folder and run `bash run.sh`, or re-run with --up.");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// said clean â€” tear down sandboxes and delete generated artifacts
// ---------------------------------------------------------------------------

#[cfg(feature = "code")]
fn cmd_clean(
    path: Option<&str>,
    targets: &[String],
    all: bool,
    containers_only: bool,
    dry_run: bool,
    json: bool,
) -> Result<(), String> {
    use std::process::Command;

    // Scope rules:
    //
    //   clean               â†’ cleans up sandbox containers only (safe default)
    //   clean --all         â†’ stops all said-sbx-* containers, deletes .said-code/
    //                         entirely, AND deletes the currently-attached .said
    //                         brain file (the one `--path` points at, or auto-
    //                         detected). Does NOT touch other .said files in cwd.
    //   clean <module>      â†’ stops matching containers, deletes matching
    //                         folders under .said-code/. Does NOT touch any
    //                         .said brain files.
    //   clean <name>.said   â†’ explicitly targets a brain file â€” deletes the
    //                         file + any related sandboxes/folders derived
    //                         from it. User MUST include ".said" suffix to
    //                         opt into brain deletion.
    //
    // This keeps "clean billing" safe (billing is just a module name â€” its
    // brain is never touched), while giving the user a clear way to nuke a
    // specific brain with "clean willie.said" or "clean --all".
    let mut planned_containers: Vec<String> = Vec::new();
    let mut planned_folders: Vec<String> = Vec::new();
    let mut planned_brain_files: Vec<String> = Vec::new();

    // Step 1 â€” find all said-sbx-* containers.
    let out = Command::new("docker")
        .args(["ps", "-a", "--filter", "name=said-sbx-", "--format", "{{.Names}}"])
        .output()
        .map_err(|e| format!("docker ps failed: {} (is Docker running?)", e))?;
    let all_containers: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

    // Step 2 â€” find all sandbox folders under .said-code/.
    let workspace = std::path::Path::new(".said-code");
    let mut all_folders: Vec<String> = Vec::new();
    if workspace.is_dir() {
        if let Ok(rd) = std::fs::read_dir(workspace) {
            for e in rd.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = e.file_name().to_str() {
                        all_folders.push(name.to_string());
                    }
                }
            }
        }
    }

    // Step 3 â€” decide what's in scope.
    if all {
        // --all: sweep everything this project has generated â€” containers,
        // workspace folders, AND every .said brain in the current directory.
        // This is the "clean slate" command, so it should actually leave
        // nothing behind. Hidden dot-prefixed placeholders (.brain.said)
        // are included; unrelated .said files outside cwd are NOT touched.
        planned_containers.extend(all_containers.iter().cloned());
        planned_folders.extend(all_folders.iter().cloned());

        // Every .said file in cwd (the project directory).
        if let Ok(rd) = std::fs::read_dir(".") {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() {
                    let name_is_said = p.extension()
                        .map(|x| x.eq_ignore_ascii_case("said"))
                        .unwrap_or(false);
                    if name_is_said {
                        let s = p.to_string_lossy().to_string();
                        // Normalize leading "./" on Unix-style paths.
                        let s = s.trim_start_matches("./").trim_start_matches(".\\").to_string();
                        if !planned_brain_files.contains(&s) {
                            planned_brain_files.push(s);
                        }
                    }
                }
            }
        }
        // Also include the attached brain if resolve() gives an absolute
        // path that wasn't in cwd (shouldn't normally happen but is safe).
        if let Ok(attached) = crate::resolve::resolve(path) {
            if attached.is_file() {
                let as_str = attached.to_string_lossy().to_string();
                let already = planned_brain_files.iter().any(|b| {
                    std::fs::canonicalize(b).ok() == std::fs::canonicalize(&as_str).ok()
                });
                if !already {
                    planned_brain_files.push(as_str);
                }
            }
        }
    } else if targets.is_empty() {
        // No target and no --all: stop sandboxes only, keep everything else.
        planned_containers.extend(all_containers.iter().cloned());
    } else {
        // Explicit targets.
        for t in targets {
            // (a) Brain-file target: user typed something ending in ".said".
            //     Delete the file and any sandboxes/folders derived from it.
            if t.ends_with(".said") || t.ends_with(".SAID") {
                let t_path = std::path::PathBuf::from(t);
                if t_path.is_file() {
                    planned_brain_files.push(t.clone());
                }
                // Folders derived from this brain have the form
                // "<module>.<brain-stem>" â€” match on that stem.
                let stem = t_path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !stem.is_empty() {
                    for f in &all_folders {
                        let f_lc = f.to_lowercase();
                        if f_lc.ends_with(&format!(".{}", stem)) {
                            if !planned_folders.contains(f) { planned_folders.push(f.clone()); }
                        }
                    }
                    // Containers derived from this brain â€” they follow the
                    // `said-sbx-<combo>-<port>` shape; combo has no brain
                    // name so we match by the folder list we just built.
                    for f in &planned_folders {
                        let combo = f.split('.').next().unwrap_or(f);
                        let combo_for_match = combo.replace('+', "-").to_lowercase();
                        let prefix = format!("said-sbx-{}-", combo_for_match);
                        for c in &all_containers {
                            if c.to_lowercase().starts_with(&prefix)
                                && !planned_containers.contains(c)
                            {
                                planned_containers.push(c.clone());
                            }
                        }
                    }
                }
                continue;
            }

            // (b) Module-name target: match sandboxes + folders only. Never
            //     touch .said brain files.
            let t_lc = t.to_lowercase();
            for f in &all_folders {
                let f_lc = f.to_lowercase();
                let stem = f_lc.split('.').next().unwrap_or(&f_lc);
                if stem == t_lc
                    || stem.starts_with(&format!("{}+", t_lc))
                    || stem.starts_with(&format!("{}-", t_lc))
                    || stem.contains(&format!("+{}", t_lc))
                    || stem.contains(&format!("+{}+", t_lc))
                    || stem.ends_with(&format!("+{}", t_lc))
                {
                    if !planned_folders.contains(f) { planned_folders.push(f.clone()); }
                }
            }
            let needle = format!("-{}-", t_lc);
            let prefix = format!("said-sbx-{}-", t_lc);
            for c in &all_containers {
                let c_lc = c.to_lowercase();
                if c_lc.starts_with(&prefix) || c_lc.contains(&needle) {
                    if !planned_containers.contains(c) { planned_containers.push(c.clone()); }
                }
            }
        }
    }

    if json {
        println!("{}", serde_json::json!({
            "plan": {
                "containers_to_stop": &planned_containers,
                "folders_to_delete": if containers_only { Vec::<String>::new() } else { planned_folders.clone() },
                "brain_files_to_delete": if containers_only { Vec::<String>::new() } else { planned_brain_files.clone() },
                "remove_workspace_root": all && !containers_only,
            },
            "dry_run": dry_run
        }));
    } else {
        println!("Clean plan:");
        if planned_containers.is_empty() {
            println!("  (no said-sbx-* containers matched)");
        } else {
            println!("  Containers to stop + remove:");
            for c in &planned_containers { println!("    - {}", c); }
        }
        if containers_only {
            println!("  (--containers-only: folders and brains will be kept)");
        } else {
            if planned_folders.is_empty() {
                println!("  (no matching folders under .said-code/)");
            } else {
                println!("  Folders to delete:");
                for f in &planned_folders { println!("    - .said-code/{}", f); }
            }
            if planned_brain_files.is_empty() {
                println!("  (no .said brain files in scope)");
            } else {
                println!("  .said brain files to DELETE (this wipes the brain):");
                for b in &planned_brain_files { println!("    - {}", b); }
            }
        }
        if all && !containers_only {
            println!("  .said-code/ master folder will be removed entirely.");
        }
        if dry_run {
            println!();
            println!("Dry-run â€” no changes made. Remove --dry-run to execute.");
            return Ok(());
        }
    }

    if dry_run { return Ok(()); }

    // Step 4 â€” execute.
    for c in &planned_containers {
        if !json { print!("  stopping {} ... ", c); }
        let _ = Command::new("docker").args(["rm", "-f", c]).output();
        if !json { println!("ok"); }
    }

    if !containers_only {
        for f in &planned_folders {
            let p = workspace.join(f);
            if !json { print!("  removing {} ... ", p.display()); }
            // Retry briefly â€” docker may still be unmounting volumes on Windows.
            let mut ok = false;
            for _ in 0..3 {
                if std::fs::remove_dir_all(&p).is_ok() { ok = true; break; }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            if !json { println!("{}", if ok { "ok" } else { "busy â€” retry after docker mounts release" }); }
        }
        for b in &planned_brain_files {
            if !json { print!("  deleting brain {} ... ", b); }
            match std::fs::remove_file(b) {
                Ok(_) => {
                    if !json { println!("ok"); }
                }
                Err(e) => {
                    if !json {
                        println!("failed ({}) â€” if the MCP server is running it may have the file locked; restart MCP and retry", e);
                    }
                }
            }
        }
        if all {
            if workspace.exists() {
                if !json { print!("  removing .said-code/ ... "); }
                let _ = std::fs::remove_dir_all(workspace);
                if !json { println!("ok"); }
            }
        } else if workspace.is_dir() {
            // Per-module clean: if the .said-code/ folder is now empty (we
            // removed the last workspace inside it), clean it up too so
            // the project root stays tidy. This matches user expectation:
            // "clean billing" shouldn't leave an empty master folder behind.
            let empty = std::fs::read_dir(workspace)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            if empty {
                if !json { print!("  .said-code/ is empty â€” removing it ... "); }
                let _ = std::fs::remove_dir(workspace);
                if !json { println!("ok"); }
            }
        }
    }

    if !json {
        println!();
        println!("Done.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// said snapshot â€” extract a module into its own folder + lens .said
// ---------------------------------------------------------------------------

#[cfg(feature = "code")]
fn cmd_snapshot(path: Option<&str>, module: &str, output: Option<&str>, json: bool) -> Result<(), String> {
    use std::collections::{HashMap, HashSet};

    let mut brain = open_brain(path)?;
    let said_filename = crate::resolve::resolve(path)?;
    let said_stem = Path::new(&said_filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("brain");

    // Output directory: .said-code/card.vivere/ by default â€” puts all module
    // extracts under one master workspace folder so the project root stays
    // clean. Users can still override with --output.
    let out_dir = output.map(|s| s.to_string())
        .unwrap_or_else(|| format!(".said-code/{}.{}", module.to_lowercase(), said_stem));
    let out_path = Path::new(&out_dir);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if !json {
        eprintln!("Snapshot: extracting '{}' module from {}", module, said_filename.display());
        eprintln!("Output:   {}/", out_dir);
        eprintln!();
    }

    // Step 1: Find all objects related to this module via deep semantic search
    let query = format!(
        "all stored procedures tables triggers views and functions related to {} management processing configuration",
        module
    );
    let results = brain.recall(&query, 500); // deep recall â€” get everything

    if results.is_empty() {
        return Err(format!("No objects found related to '{}'. Try a different module name.", module));
    }

    // Also do a keyword grep for the module name in all doc_ids
    let module_lower = module.to_lowercase();
    let doc_ids: Vec<String> = brain.frames.active_doc_ids().into_iter().map(|s| s.to_string()).collect();
    let mut module_doc_ids: HashSet<String> = HashSet::new();

    // Add all deep search results
    for r in &results {
        module_doc_ids.insert(r.doc_id.clone());
    }

    // Add all doc_ids that contain the module name in their path or name
    for did in &doc_ids {
        let lower = did.to_lowercase();
        if lower.contains(&module_lower) {
            module_doc_ids.insert(did.clone());
        }
    }

    // Step 2: Classify each module object
    let mut exclusive_files: HashSet<String> = HashSet::new(); // source files only in this module
    let mut module_tables: HashSet<String> = HashSet::new();
    let mut module_procs: HashSet<String> = HashSet::new();
    let mut module_triggers: HashSet<String> = HashSet::new();
    let mut module_views: HashSet<String> = HashSet::new();
    let mut module_functions: HashSet<String> = HashSet::new();

    // Detect hub tables (same logic as discover)
    let mut fk_inbound: HashMap<String, usize> = HashMap::new();
    for did in &doc_ids {
        let tags = brain.frames.get_meta(did)
            .map(|m| m.tags.clone())
            .unwrap_or_default();
        for tag in &tags {
            if let Some(rest) = tag.strip_prefix("fk:") {
                let parts: Vec<&str> = rest.split('.').collect();
                if parts.len() >= 2 {
                    let target = parts[parts.len().min(2) - 1].to_uppercase();
                    *fk_inbound.entry(target).or_insert(0) += 1;
                }
            }
        }
    }
    let hub_tables: HashSet<String> = fk_inbound.iter()
        .filter(|(_, &count)| count >= 5)
        .map(|(name, _)| name.clone())
        .collect();

    for did in &module_doc_ids {
        let title = brain.frames.get_meta(did)
            .and_then(|m| m.title.clone())
            .unwrap_or_default()
            .to_lowercase();

        let short = extract_short_name(did);

        if title.contains("(table)") || title.contains("create_table") {
            if hub_tables.contains(&short.to_uppercase()) {
                // Hub table â€” goes to Shared/
            } else {
                module_tables.insert(did.clone());
            }
        } else if title.contains("(proc)") || title.contains("create_procedure") || title.contains("alter_procedure") {
            module_procs.insert(did.clone());
        } else if title.contains("(trigger)") || title.contains("create_trigger") {
            module_triggers.insert(did.clone());
        } else if title.contains("(view)") || title.contains("create_view") {
            module_views.insert(did.clone());
        } else if title.contains("(function)") || title.contains("create_function") {
            module_functions.insert(did.clone());
        }

        // Track the source file
        let file_path = if let Some(pos) = did.find("::") {
            did[..pos].to_string()
        } else {
            did.clone()
        };
        exclusive_files.insert(file_path);
    }

    // Step 3: Identify shared hub tables that this module touches
    let mut shared_tables: HashMap<String, Vec<String>> = HashMap::new(); // hub table â†’ which module procs touch it

    for did in module_procs.iter().chain(module_triggers.iter()).chain(module_views.iter()) {
        let content = brain.get(did).unwrap_or_default();
        let upper = content.to_uppercase();
        for hub in &hub_tables {
            if upper.contains(hub.as_str()) {
                shared_tables.entry(hub.clone())
                    .or_default()
                    .push(extract_short_name(did));
            }
        }
    }

    // Step 4: Create output directory structure
    let exclusive_dir = out_path.join("Exclusive");
    let shared_dir = out_path.join("Shared");
    std::fs::create_dir_all(&exclusive_dir).map_err(|e| format!("mkdir: {}", e))?;
    std::fs::create_dir_all(&shared_dir).map_err(|e| format!("mkdir: {}", e))?;

    // Step 5: Copy source files into the output structure
    // Works for ALL file types: SQL, TypeScript, Python, Rust, C#, Java, Go
    // Preserves original directory layout under Exclusive/
    let mut copied = 0usize;
    let mut copied_files: HashSet<String> = HashSet::new();

    // Find source root from the source: tag on any frame
    let source_root = doc_ids.iter()
        .filter_map(|did| {
            brain.frames.get_meta(did)
                .and_then(|m| m.tags.iter().find(|t| t.starts_with("source:")).cloned())
        })
        .next()
        .and_then(|t| {
            let p = &t[7..]; // strip "source:"
            let path = Path::new(p);
            // Walk up to find the project root (where .said file lives or where init was run)
            let mut current = path;
            loop {
                // Check for common project root markers
                if current.join("dbo").exists()
                    || current.join("src").exists()
                    || current.join("package.json").exists()
                    || current.join("Cargo.toml").exists()
                    || current.join(".git").exists() {
                    return Some(current.to_string_lossy().to_string());
                }
                current = match current.parent() {
                    Some(p) if !p.as_os_str().is_empty() => p,
                    _ => break,
                };
            }
            None
        });

    // Copy module files â€” split between Exclusive/ and Shared/
    // For code: files in the module's directory â†’ Exclusive, others â†’ Shared
    // For SQL: non-hub tables â†’ Exclusive, hub tables â†’ Shared (handled separately)
    let module_lower = module.to_lowercase();
    let mut shared_code_files: Vec<(String, String)> = Vec::new(); // (file_path, which_module_dir)

    let all_module_dids: Vec<&String> = module_doc_ids.iter().collect();

    for did in &all_module_dids {
        let file_part = if let Some(pos) = did.find("::") { &did[..pos] } else { did.as_str() };

        if !copied_files.insert(file_part.to_string()) {
            continue;
        }

        // Determine if this file belongs to the module or is shared
        let file_lower = file_part.to_lowercase();
        let file_parts: Vec<&str> = file_part.split('/').collect();

        // File belongs to module if: its path contains the module name
        // e.g., "src/card/card-service.ts" contains "card"
        let is_exclusive = file_lower.contains(&module_lower)
            || file_parts.iter().any(|p| p.to_lowercase() == module_lower);

        // For SQL: hub tables go to Shared (already handled below)
        let is_hub = hub_tables.contains(&extract_short_name(did).to_uppercase());

        let target_dir = if is_exclusive && !is_hub {
            &exclusive_dir
        } else {
            // Track for shared usage docs
            let dir_name = file_parts.iter()
                .find(|p| !p.is_empty() && *p != &"src" && *p != &"dbo")
                .unwrap_or(&"root")
                .to_string();
            shared_code_files.push((file_part.to_string(), dir_name));
            &shared_dir
        };

        // Two strategies to materialize the file into Exclusive/ or Shared/:
        //   1. Copy from disk if we can find the original.
        //   2. RECONSTRUCT from brain contents â€” the brain has every chunk,
        //      concatenating them rebuilds a usable file even if the original
        //      directory is gone (laptop reformatted, repo archived, etc.).
        //
        // Strategy 2 is the primary path now because it makes snapshots
        // robust to relocation. Strategy 1 only wins when a legacy brain
        // lacks the frame content (e.g. tombstones without payload).
        let mut materialized = false;

        let candidates = vec![
            PathBuf::from(file_part),
            source_root.as_ref().map(|r| PathBuf::from(r).join(file_part)).unwrap_or_default(),
            // Also try stripping a leading folder in case rel_path double-
            // counts the project folder (e.g. init was run one level above
            // the source root, so rel_path = "myproj/src/foo.sql" but
            // source_root already ends in "/myproj").
            source_root.as_ref().and_then(|r| {
                let parts: Vec<&str> = file_part.split('/').collect();
                if parts.len() > 1 {
                    Some(PathBuf::from(r).join(parts[1..].join("/")))
                } else { None }
            }).unwrap_or_default(),
        ];

        for src in &candidates {
            if src.is_file() {
                let dest = target_dir.join(file_part);
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(content) = std::fs::read(src) {
                    let _ = std::fs::write(&dest, content);
                    copied += 1;
                    materialized = true;
                }
                break;
            }
        }

        // Reconstruct from brain if disk lookup failed. Concatenate every
        // chunk whose doc_id starts with "<file_part>::" â€” they were split
        // by sql_chunk / ast_chunk so rejoining them reproduces the source.
        if !materialized {
            let prefix = format!("{}::", file_part);
            let mut chunks: Vec<(usize, String)> = Vec::new();
            for did in &doc_ids {
                if did.starts_with(&prefix) {
                    // Sort key: the start_line from "kind:START" segment of
                    // doc_id (layout: path::NAME::kind:start_line).
                    let tail = &did[prefix.len()..];
                    let start_line: usize = tail.rsplit(':').next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    if let Some(content) = brain.get(did) {
                        chunks.push((start_line, content));
                    }
                }
            }
            if !chunks.is_empty() {
                chunks.sort_by_key(|(l, _)| *l);
                let joined = chunks.into_iter()
                    .map(|(_, c)| c)
                    .collect::<Vec<_>>()
                    .join("\nGO\n");
                let dest = target_dir.join(file_part);
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&dest, joined).is_ok() {
                    copied += 1;
                }
            }
        }
    }

    // Generate usage docs for shared code files
    if !shared_code_files.is_empty() {
        let mut by_module: HashMap<String, Vec<String>> = HashMap::new();
        for (file, dir) in &shared_code_files {
            by_module.entry(dir.clone()).or_default().push(file.clone());
        }
        for (shared_module, files) in &by_module {
            let usage_path = shared_dir.join(format!("{}.{}_USAGE.md", shared_module, module.to_uppercase()));
            let mut usage = format!("# {} module's usage of '{}' (shared)\n\n", module, shared_module);
            usage.push_str(&format!("The '{}' module imports from '{}'. These files are shared.\n", module, shared_module));
            usage.push_str("Do NOT modify these directly â€” create interfaces/contracts instead.\n\n");
            usage.push_str("## Files:\n");
            for f in files {
                usage.push_str(&format!("- {}\n", f));
            }
            let _ = std::fs::write(&usage_path, usage);
        }
    }

    // Step 6: Generate shared table usage files
    for (hub_table, procs_using_it) in &shared_tables {
        // Find the hub table's source file and copy it
        for did in &doc_ids {
            let short = extract_short_name(did).to_uppercase();
            if short == *hub_table {
                let file_part = if let Some(pos) = did.find("::") { &did[..pos] } else { did.as_str() };
                let candidates = vec![
                    PathBuf::from(file_part),
                    source_root.as_ref().map(|r| PathBuf::from(r).join(file_part)).unwrap_or_default(),
                ];
                for src in &candidates {
                    if src.is_file() {
                        let filename = src.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let dest = shared_dir.join(&filename);
                        let _ = std::fs::copy(src, &dest);
                        break;
                    }
                }
                break;
            }
        }

        // Generate usage analysis
        let usage_file = shared_dir.join(format!("{}.{}_USAGE.md", hub_table, module.to_uppercase()));
        let mut usage = format!("# {} Module Usage of {}\n\n", module, hub_table);
        usage.push_str(&format!("## Procs/Triggers in {} that reference this table:\n", module));
        for proc_name in procs_using_it {
            usage.push_str(&format!("- {}\n", proc_name));
        }
        usage.push_str(&format!("\n## Other modules also use this table\n"));
        usage.push_str("This is a shared/hub table â€” it should become an API boundary.\n");
        usage.push_str(&format!("Do NOT move this table into the {} module.\n", module));
        usage.push_str("Instead, create an API contract for accessing it.\n");
        let _ = std::fs::write(&usage_file, usage);
    }

    // Snapshot symbols from source brain (needed for BOUNDARY + MODULE_MAP + module brain)
    let source_symbols = brain.symbols_snapshot();

    // Step 7: Generate BOUNDARY.md
    let boundary_path = out_path.join("BOUNDARY.md");
    let mut boundary = format!("# {} Module â€” Boundary Analysis\n\n", module);
    boundary.push_str(&format!("Extracted from: {}\n", said_filename.display()));
    boundary.push_str(&format!("Date: {}\n\n", chrono_date()));

    boundary.push_str("## Module Statistics\n\n");
    boundary.push_str(&format!("| Type | Count |\n|------|-------|\n"));
    boundary.push_str(&format!("| Tables (exclusive) | {} |\n", module_tables.len()));
    boundary.push_str(&format!("| Stored Procedures | {} |\n", module_procs.len()));
    boundary.push_str(&format!("| Triggers | {} |\n", module_triggers.len()));
    boundary.push_str(&format!("| Views | {} |\n", module_views.len()));
    boundary.push_str(&format!("| Functions | {} |\n", module_functions.len()));
    boundary.push_str(&format!("| **Total exclusive** | **{}** |\n",
        module_tables.len() + module_procs.len() + module_triggers.len() +
        module_views.len() + module_functions.len()));
    boundary.push_str(&format!("| Shared hub tables | {} |\n", shared_tables.len()));

    // â”€â”€ Hidden Triggers Report â”€â”€
    // Group triggers by their parent table (triggers defined in table files)
    boundary.push_str("\n## Hidden Triggers (business logic trapped in database tier)\n\n");
    boundary.push_str("These triggers fire automatically on INSERT/UPDATE/DELETE.\n");
    boundary.push_str("The new API must replicate this logic in the application layer.\n\n");

    let mut triggers_by_table: std::collections::HashMap<String, Vec<(String, String, u32, u32)>> = std::collections::HashMap::new();
    for did in &module_triggers {
        let short = extract_short_name(did);
        // Extract parent table from doc_id or trigger name
        let parent_table = if let Some(pos) = did.find("::") {
            let file = &did[..pos];
            file.rsplit('/').next().unwrap_or("").trim_end_matches(".sql").to_string()
        } else {
            "unknown".to_string()
        };
        // Determine trigger event from name
        let event = if short.contains("INS_UPD_DEL") || short.contains("INS_UPD") { "INSERT/UPDATE/DELETE" }
            else if short.contains("AFTER_INS") || short.contains("_INS_") { "INSERT" }
            else if short.contains("AFTER_UPD") || short.contains("_UPD_") { "UPDATE" }
            else if short.contains("AFTER_DEL") || short.contains("_DEL_") { "DELETE" }
            else if short.contains("INSTEAD") { "INSTEAD OF" }
            else { "UNKNOWN" };

        // Get line range from symbols
        let (start, end) = source_symbols.get(did).first()
            .map(|(_, _, s, e)| (*s, *e))
            .unwrap_or((0, 0));

        triggers_by_table.entry(parent_table)
            .or_default()
            .push((short, event.to_string(), start, end));
    }

    if triggers_by_table.is_empty() {
        boundary.push_str("No triggers found in this module.\n\n");
    } else {
        // Sort tables by number of triggers
        let mut sorted_tables: Vec<_> = triggers_by_table.iter().collect();
        sorted_tables.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        for (table, trigs) in &sorted_tables {
            boundary.push_str(&format!("### {}\n\n", table));
            boundary.push_str("| Trigger | Fires on | Lines | Size |\n");
            boundary.push_str("|---------|----------|-------|------|\n");
            for (name, event, start, end) in trigs.iter() {
                let size = if *end > *start { end - start } else { 0 };
                boundary.push_str(&format!("| {} | {} | {}-{} | {} lines |\n",
                    name, event, start, end, size));
            }
            boundary.push_str("\n");
        }

        let total_trigger_lines: u32 = triggers_by_table.values()
            .flat_map(|trigs| trigs.iter())
            .map(|(_, _, start, end)| if *end > *start { end - start } else { 0 })
            .sum();
        boundary.push_str(&format!("**Total hidden business logic: {} triggers, ~{} lines of SQL**\n",
            module_triggers.len(), total_trigger_lines));
        boundary.push_str("All of this must be moved to the application layer.\n\n");
    }

    // â”€â”€ Exclusive Objects â”€â”€
    boundary.push_str("\n## Exclusive Objects (safe to extract)\n\n");
    boundary.push_str("These objects are ONLY used by this module. They can be moved to a new database/service.\n\n");

    if !module_tables.is_empty() {
        boundary.push_str("### Tables\n\n");
        boundary.push_str("| Table | Triggers | FK Constraints |\n");
        boundary.push_str("|-------|----------|----------------|\n");
        for did in &module_tables {
            let short = extract_short_name(did);
            let trig_count = triggers_by_table.get(&short.to_lowercase())
                .or_else(|| triggers_by_table.get(&short))
                .map(|t| t.len())
                .unwrap_or(0);
            let tags = brain.frames.get_meta(did)
                .map(|m| m.tags.clone())
                .unwrap_or_default();
            let fks: Vec<String> = tags.iter()
                .filter(|t| t.starts_with("fk:"))
                .map(|t| t[3..].to_string())
                .collect();
            let fk_str = if fks.is_empty() { "â€”".to_string() } else { fks.join(", ") };
            boundary.push_str(&format!("| {} | {} | {} |\n", short, trig_count, fk_str));
        }
        boundary.push_str("\n");
    }

    if !module_procs.is_empty() {
        boundary.push_str("### Stored Procedures\n\n");
        boundary.push_str("| Procedure | Lines | Tags |\n");
        boundary.push_str("|-----------|-------|------|\n");
        for did in &module_procs {
            let short = extract_short_name(did);
            let (start, end) = source_symbols.get(did).first()
                .map(|(_, _, s, e)| (*s, *e))
                .unwrap_or((0, 0));
            let tags = brain.frames.get_meta(did)
                .map(|m| m.tags.clone())
                .unwrap_or_default();
            let special: Vec<&String> = tags.iter()
                .filter(|t| t.starts_with("dynamic_sql") || t.starts_with("refs:") || t.starts_with("check:"))
                .collect();
            let tag_str = if special.is_empty() { "â€”".to_string() }
                else { special.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ") };
            boundary.push_str(&format!("| {} | {}-{} | {} |\n", short, start, end, tag_str));
        }
        boundary.push_str("\n");
    }

    boundary.push_str("\n## Shared Hub Tables (need API contracts)\n\n");
    boundary.push_str("These tables are used by this module AND other modules.\n");
    boundary.push_str("They should NOT be moved â€” instead create API boundaries.\n\n");

    let mut sorted_shared: Vec<_> = shared_tables.iter().collect();
    sorted_shared.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (hub, procs_list) in &sorted_shared {
        boundary.push_str(&format!("### {}\n", hub));
        boundary.push_str(&format!("Used by {} {} procs/triggers:\n", procs_list.len(), module));
        for p in procs_list.iter().take(10) {
            boundary.push_str(&format!("- {}\n", p));
        }
        if procs_list.len() > 10 {
            boundary.push_str(&format!("- ... +{} more\n", procs_list.len() - 10));
        }
        boundary.push_str("\n");
    }

    // Code Dependencies section (for non-SQL codebases)
    if !shared_code_files.is_empty() {
        boundary.push_str("\n## Code Dependencies (shared modules)\n\n");
        boundary.push_str("These modules are imported by this module but owned by other teams.\n");
        boundary.push_str("Create interfaces/contracts â€” do NOT modify directly.\n\n");

        let mut by_module: HashMap<String, Vec<String>> = HashMap::new();
        for (file, dir) in &shared_code_files {
            by_module.entry(dir.clone()).or_default().push(file.clone());
        }
        let mut sorted_deps: Vec<_> = by_module.iter().collect();
        sorted_deps.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (dep_module, files) in &sorted_deps {
            boundary.push_str(&format!("### {} module ({} files)\n", dep_module, files.len()));
            for f in files.iter() {
                let short = f.rsplit('/').next().unwrap_or(f.as_str());
                boundary.push_str(&format!("- {}\n", short));
            }
            boundary.push_str("\n");
        }
    }

    std::fs::write(&boundary_path, boundary).map_err(|e| format!("write boundary: {}", e))?;

    // Step 7b: Generate MODULE_MAP.md â€” complete inventory for the developer
    let map_path = out_path.join("MODULE_MAP.md");
    let mut map = format!("# {} Module â€” Complete Object Map\n\n", module);
    map.push_str("Every object in this module with type, location, and line count.\n");
    map.push_str("Use this as a checklist when building/rewriting the module.\n\n");

    // All objects sorted by type
    let mut all_syms: Vec<(String, String, String, u32, u32)> = Vec::new(); // (name, type, file, start, end)
    for did in module_tables.iter().chain(module_procs.iter())
        .chain(module_triggers.iter()).chain(module_views.iter())
        .chain(module_functions.iter()) {
        let syms = source_symbols.get(did);
        if syms.is_empty() {
            let short = extract_short_name(did);
            let _title = brain.frames.get_meta(did)
                .and_then(|m| m.title.clone())
                .unwrap_or_default();
            let obj_type = if module_tables.contains(did) { "table" }
                else if module_procs.contains(did) { "proc" }
                else if module_triggers.contains(did) { "trigger" }
                else if module_views.contains(did) { "view" }
                else { "function" };
            let file = if let Some(pos) = did.find("::") { &did[..pos] } else { did.as_str() };
            all_syms.push((short, obj_type.to_string(), file.to_string(), 0, 0));
        } else {
            for (name, kind, start, end) in syms {
                let file = if let Some(pos) = did.find("::") { &did[..pos] } else { did.as_str() };
                all_syms.push((name.to_string(), kind.as_str().to_string(), file.to_string(), start, end));
            }
        }
    }

    // Group by type â€” SQL types first, then code types
    for (type_name, type_label) in &[
        // SQL types
        ("table", "Tables"), ("proc", "Stored Procedures"), ("trigger", "Triggers"),
        ("view", "Views"), ("index", "Indexes"),
        // Code types
        ("class", "Classes"), ("fn", "Functions"), ("method", "Methods"),
        ("struct", "Structs"), ("enum", "Enums"), ("trait", "Traits/Interfaces"),
        ("impl", "Implementations"), ("const", "Constants"),
        ("function", "Functions/Exports"), ("?", "Other")] {
        let items: Vec<_> = all_syms.iter()
            .filter(|(_, t, _, _, _)| t == *type_name)
            .collect();
        if items.is_empty() { continue; }

        map.push_str(&format!("## {} ({} objects)\n\n", type_label, items.len()));
        map.push_str("| Name | File | Lines | Lines of Code |\n");
        map.push_str("|------|------|-------|---------------|\n");
        for (name, _, file, start, end) in &items {
            let size = if *end > *start { end - start } else { 0 };
            let file_short = file.rsplit('/').next().unwrap_or(file);
            map.push_str(&format!("| {} | {} | {}-{} | {} lines |\n", name, file_short, start, end, size));
        }
        map.push_str("\n");
    }

    // Code files section â€” list all source files with their imports
    let exclusive_code: Vec<&String> = copied_files.iter()
        .filter(|f| {
            let lower = f.to_lowercase();
            lower.contains(&module_lower) || f.split('/').any(|p| p.to_lowercase() == module_lower)
        })
        .collect();
    let shared_code: Vec<&String> = copied_files.iter()
        .filter(|f| {
            let lower = f.to_lowercase();
            !lower.contains(&module_lower) && !f.split('/').any(|p| p.to_lowercase() == module_lower)
        })
        .collect();

    if !exclusive_code.is_empty() || !shared_code.is_empty() {
        map.push_str("## Source Files\n\n");

        if !exclusive_code.is_empty() {
            map.push_str(&format!("### Exclusive ({} files â€” safe to extract)\n\n", exclusive_code.len()));
            for f in &exclusive_code {
                map.push_str(&format!("- {}\n", f));
            }
            map.push_str("\n");
        }

        if !shared_code.is_empty() {
            map.push_str(&format!("### Shared ({} files â€” need interfaces)\n\n", shared_code.len()));
            for f in &shared_code {
                map.push_str(&format!("- {}\n", f));
            }
            map.push_str("\n");
        }
    }

    std::fs::write(&map_path, map).map_err(|e| format!("write module map: {}", e))?;

    // Step 8: Create lens file â€” a live view over the parent brain
    // The lens file is tiny (just metadata). It points to the parent .said file
    // and stores which frame IDs belong to this module. Queries go through the
    // parent brain filtered by the lens. Always fresh â€” no manual sync.
    let module_said_path = out_path.join(format!("{}.{}.said", module.to_lowercase(), said_stem));

    // Collect all frame IDs for this module (exclusive + shared)
    let mut lens_frame_ids: std::collections::HashSet<String> = module_doc_ids.clone();

    // Also include shared table frame IDs
    for (hub_table, _) in &shared_tables {
        for did in &doc_ids {
            let short = extract_short_name(did).to_uppercase();
            if short == *hub_table {
                lens_frame_ids.insert(did.to_string());
            }
        }
    }

    let frames_added = lens_frame_ids.len();

    // Calculate relative path from lens file to parent brain
    // If we can't compute relative, fall back to absolute
    let parent_path_str = {
        let parent_canonical = std::fs::canonicalize(&said_filename)
            .unwrap_or_else(|_| said_filename.clone());
        let lens_dir = module_said_path.parent()
            .and_then(|d| std::fs::canonicalize(d).ok())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        pathdiff::diff_paths(&parent_canonical, &lens_dir)
            .map(|p| p.to_string_lossy().to_string().replace('\\', "/"))
            .unwrap_or_else(|| parent_canonical.to_string_lossy().to_string().replace('\\', "/"))
    };

    // The lens stores its own frame_ids set â€” so the parent brain does NOT need
    // to be mutated. Mutating the parent (add_tag + save) would trigger a full
    // rewrite, and save() has a known bug where pre-existing blocks-on-disk are
    // not copied forward when there are no pending block leaders (the blocks
    // live only in the old mmap, which is invalidated by the atomic rename).
    // Leaving the parent read-only also matches the "no copy-paste, no drift"
    // contract of the lens architecture.
    let lens = sca_core::lens::LensFile::create(
        &module_said_path,
        &parent_path_str,
        module,
        lens_frame_ids,
    );
    lens.save().map_err(|e| format!("save lens: {}", e))?;

    // Lens file created â€” no frame copies needed.
    // The lens reads from the parent brain at query time.

    // Output
    let _total_exclusive = module_tables.len() + module_procs.len() +
        module_triggers.len() + module_views.len() + module_functions.len();

    if json {
        println!("{}", serde_json::json!({
            "module": module,
            "output": out_dir,
            "exclusive_tables": module_tables.len(),
            "exclusive_procs": module_procs.len(),
            "exclusive_triggers": module_triggers.len(),
            "exclusive_views": module_views.len(),
            "exclusive_functions": module_functions.len(),
            "shared_hub_tables": shared_tables.len(),
            "files_copied": copied,
            "frames_in_brain": frames_added,
        }));
    } else {
        println!("Snapshot complete: {}/", out_dir);
        println!();
        println!("  Exclusive (safe to extract):");
        println!("    {} tables, {} procs, {} triggers, {} views, {} functions",
            module_tables.len(), module_procs.len(), module_triggers.len(),
            module_views.len(), module_functions.len());
        println!("    {} files copied to Exclusive/", copied);
        println!();
        println!("  Shared (need API contracts):");
        let total_shared = shared_tables.len() + shared_code_files.len();
        println!("    {} shared files in Shared/ with usage analysis", total_shared);
        println!();
        println!("  Brain: {}", module_said_path.display());
        println!("    {} frames indexed", frames_added);
        println!();
        println!("  Boundary: {}", boundary_path.display());
        println!();
        println!("  Next steps:");
        println!("    said ask --path {} \"How does {} work?\"", module_said_path.display(), module);
        println!("    # Only searches {} objects â€” fast, focused", module);
    }

    Ok(())
}

#[cfg(feature = "code")]
fn chrono_date() -> String {
    // Simple date without chrono dependency
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let years = 1970 + (days * 400 / 146097); // rough but good enough
    format!("{}", years)
}

/// Extract short object name from a doc_id
#[cfg(feature = "code")]
fn extract_short_name(doc_id: &str) -> String {
    // Doc_id layout is either:
    //   path::NAME::kind:line   (new â€” from code-AST chunks)
    //   path::NAME              (legacy â€” whole-file frames)
    //   path                    (no chunk suffix)
    // The chunk NAME is the first segment after the leading path (i.e. between
    // the first and second "::"). Fall back to the last path segment.
    let name = if let Some(pos) = doc_id.find("::") {
        let after = &doc_id[pos + 2..];
        after.split("::").next().unwrap_or(after)
    } else if let Some(pos) = doc_id.rfind('/') {
        &doc_id[pos + 1..]
    } else {
        doc_id
    };
    name.trim_end_matches(".sql")
        .replace("DBO.", "")
        .replace("dbo.", "")
        .to_string()
}

#[cfg(feature = "docs")]
fn cmd_ingest(
    path: Option<&str>,
    target: &str,
    pointer: bool,
    summary: Option<&str>,
    json: bool,
) -> Result<(), String> {
    let target_path = Path::new(target);
    if !target_path.exists() {
        return Err(format!("not found: {}", target));
    }

    let mut brain = open_brain(path)?;
    try_load_encoder(&mut brain);

    // â”€â”€ Pointer mode (Enterprise) â€” skip content extraction + compression â”€â”€
    //
    // Store a short, searchable frame that names the resource without
    // embedding bytes. Single-file or directory (each file becomes its own
    // pointer frame). No OCR, no decompression, no GB of text in the .said.
    //
    // The `--summary` flag overrides per-file summaries; without it we use
    // the filename + relative path as a minimal summary so recall still
    // ranks the frame correctly when the query mentions the name.
    if pointer {
        let files: Vec<PathBuf> = if target_path.is_file() {
            vec![target_path.to_path_buf()]
        } else if target_path.is_dir() {
            let root = target_path.canonicalize()
                .map_err(|e| format!("resolve {}: {}", target, e))?;
            let gitignore = load_gitignore(&root);
            let mut out = Vec::new();
            walk_dir_gitignore(&root, &root, &gitignore, &mut out);
            out
        } else {
            return Err(format!("not a file or directory: {}", target));
        };
        if files.is_empty() {
            return Err(format!("no files found in {}", target));
        }

        if !json {
            eprintln!("Ingesting {} file(s) as pointers from {}", files.len(), target);
        }

        let mut n_frames = 0u64;
        for file in &files {
            let uri = file.canonicalize().ok()
                .map(|p| format!("file://{}", p.display().to_string().replace('\\', "/")))
                .unwrap_or_else(|| file.display().to_string());
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let mime = file.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase());
            let summary_text = summary.map(|s| s.to_string()).unwrap_or_else(|| {
                format!("Pointer to {} at {}", name, uri)
            });
            brain.remember_as_external_pointer(
                None,
                &uri,
                mime.as_deref(),
                Some(&name),
                &summary_text,
                Vec::new(),
            );
            n_frames += 1;
        }

        brain.build_index().map_err(|e| format!("build_index: {}", e))?;
        brain.save().map_err(|e| format!("save: {}", e))?;

        if json {
            println!("{}", serde_json::json!({
                "mode": "pointer",
                "target": target,
                "files": files.len(),
                "frames": n_frames,
            }));
        } else {
            eprintln!("âœ“ Pointer ingest complete: {} file(s) â†’ {} frame(s)", files.len(), n_frames);
        }
        return Ok(());
    }

    // Enforce brain mode: Enterprise brains refuse content-embedding ingests.
    // Users MUST use `--pointer` (handled above) or switch to portable mode.
    brain.ensure_content_ingest_allowed()?;

    // Collect everything to ingest. Single file â†’ Vec of 1. Directory â†’
    // gitignore-aware walk, same rules as cmd_init, filtered to supported
    // extensions only.
    let files: Vec<PathBuf> = if target_path.is_file() {
        vec![target_path.to_path_buf()]
    } else if target_path.is_dir() {
        let root = target_path.canonicalize()
            .map_err(|e| format!("resolve {}: {}", target, e))?;
        let gitignore = load_gitignore(&root);
        let mut out = Vec::new();
        walk_dir_gitignore(&root, &root, &gitignore, &mut out);
        out.retain(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .and_then(ingest_kind)
                .is_some()
        });
        out
    } else {
        return Err(format!("not a file or directory: {}", target));
    };

    if files.is_empty() {
        return Err(format!("no supported documents or media files found in {}", target));
    }

    if !json {
        eprintln!("Ingesting {} file(s) from {}", files.len(), target);
    }

    let total_files = files.len();
    // Mutated only inside docs-gated ingest blocks below; lean bundles don't.
    #[allow(unused_mut)]
    let mut total_frames = 0u64;
    let mut total_skipped = 0u64;
    let mut all_reports: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut failed_files: Vec<(String, String)> = Vec::new(); // (path, error)

    for (i, file_path) in files.iter().enumerate() {
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let kind = match ingest_kind(ext) {
            Some(k) => k,
            None => continue,
        };
        let display = file_path.to_string_lossy();

        if !json {
            eprintln!("[{}/{}] {} ({})", i + 1, total_files, display, ext);
        }

        let pairs_result: Result<Vec<(String, String)>, String> = match kind {
            IngestKind::Document => {
                #[cfg(feature = "docs")]
                {
                    let file_str = display.to_string();
                    // Live per-segment progress line, same shape as whisper + init
                    let json_mode = json;
                    match sca_core::document_ingest::ingest_document(
                        &mut brain,
                        &file_str,
                        |done, _total, label| {
                            if !json_mode && done % 10 == 0 {
                                eprint!("\r       ingest: {} segments ({})          ", done, label);
                                use std::io::Write;
                                let _ = std::io::stderr().flush();
                            }
                        },
                    ) {
                        Ok(report) => {
                            if !json {
                                eprintln!("\r       ingest: {} segments, {} frames, {}ms              ",
                                    report.segments_extracted, report.frames_stored, report.elapsed_ms);
                            }
                            if report.skipped { total_skipped += 1; }
                            total_frames += report.frames_stored as u64;
                            Ok(report.as_pairs())
                        }
                        Err(e) => Err(e),
                    }
                }
                #[cfg(not(feature = "docs"))]
                {
                    Err("document ingestion disabled â€” rebuild with --features docs".to_string())
                }
            }
            IngestKind::Media => {
                #[cfg(feature = "whisper")]
                {
                    let file_str = display.to_string();
                    match sca_core::whisper_ingest::ingest_video(&mut brain, &file_str) {
                        Ok(report) => {
                            if report.skipped { total_skipped += 1; }
                            total_frames += report.frames_stored as u64;
                            Ok(vec![
                                ("source_path".to_string(), report.source_path),
                                ("duration_secs".to_string(), format!("{:.1}", report.duration_secs)),
                                ("segments_transcribed".to_string(), report.segments_transcribed.to_string()),
                                ("frames_stored".to_string(), report.frames_stored.to_string()),
                                ("skipped".to_string(), report.skipped.to_string()),
                            ])
                        }
                        Err(e) => Err(e),
                    }
                }
                #[cfg(not(feature = "whisper"))]
                {
                    Err("media ingestion disabled â€” rebuild with --features whisper".to_string())
                }
            }
        };

        match pairs_result {
            Ok(pairs) => all_reports.push((display.to_string(), pairs)),
            Err(e) => {
                if !json {
                    eprintln!("       FAILED: {}", e);
                }
                failed_files.push((display.to_string(), e));
                total_skipped += 1;
            }
        }

        // Stream checkpoint: save every 50 files so progress isn't lost on crash.
        // Skip the SCA encoding during checkpoints (expensive) â€” just persist frames.
        if (i + 1) % 50 == 0 && i + 1 < total_files {
            if !json {
                eprint!("\r  Checkpoint: saving {} frames...          ", brain.frames.active_doc_ids().len());
                use std::io::Write;
                let _ = std::io::stderr().flush();
            }
            brain.compact();
            let _ = brain.save();
            if !json {
                eprintln!("\r  Checkpoint: saved ({} files done, {} frames)              ",
                    i + 1, brain.frames.active_doc_ids().len());
            }
        }
    }

    // Rebuild indexes + persist. Final encode pass must batch across all
    // newly-added frames because SCA fingerprints are whitened against the
    // corpus mean â€” per-file rebuilds would rewrite every previous fingerprint
    // every time. We DO stream progress through the batch, though, so a
    // 5000-doc bulk ingest feels responsive instead of hung.
    let json_mode = json;
    let t_idx = std::time::Instant::now();
    let _ = brain.build_index_with_progress(|done, total, passages| {
        if !json_mode {
            let pct = done * 100 / total.max(1);
            eprint!("\r  Finalizing: encoding {:3}% ({}/{} docs, {} passages)    ",
                pct, done, total, passages);
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
    });
    // compact() rebuilds the trigram inverted index and symbol table AND
    // block-compresses frames. Without this call, `said ask` falls back to
    // SCA-only (no grep pre-filter) and ranking degrades on literal queries.
    // This was the root cause of Q2/Q3 ranking miss on the docs/ validation.
    if !json {
        eprint!("\r  Finalizing: compacting + building trigram index...                      ");
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }
    brain.compact();
    brain.save()?;
    if !json {
        eprintln!("\r  Finalizing: encoded + compacted + saved  ({:.1}s)                     ",
            t_idx.elapsed().as_secs_f64());
    }

    if json {
        let docs: Vec<serde_json::Value> = all_reports.iter().map(|(file, pairs)| {
            let map: serde_json::Map<String, serde_json::Value> = pairs.iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            serde_json::json!({ "file": file, "report": map })
        }).collect();
        let failed: Vec<serde_json::Value> = failed_files.iter().map(|(f, e)| {
            serde_json::json!({ "file": f, "error": e })
        }).collect();
        println!("{}", serde_json::json!({
            "command": "ingest",
            "target": target,
            "files_processed": total_files,
            "frames_stored": total_frames,
            "files_skipped": total_skipped,
            "files_failed": failed_files.len(),
            "reports": docs,
            "failed": failed,
        }));
    } else {
        println!();
        println!("Ingest summary:");
        println!("  Target:        {}", target);
        println!("  Files handled: {}", total_files);
        println!("  Frames stored: {}", total_frames);
        println!("  Files skipped: {} (already indexed or deduped)", total_skipped - failed_files.len() as u64);
        if !failed_files.is_empty() {
            println!("  Files FAILED:  {}", failed_files.len());
            println!();
            println!("  Failed files:");
            for (path, err) in &failed_files {
                // Show just the filename for readability, full path is in the log above
                let name = Path::new(path).file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                println!("    {} â€” {}", name, err);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// LSP command implementations
// ---------------------------------------------------------------------------

/// Parse a `file:line:col` location string.
#[cfg(feature = "lsp")]
fn parse_location(location: &str) -> Result<(String, u32, u32), String> {
    // Split from the right to handle paths with colons (e.g. C:\...)
    let parts: Vec<&str> = location.rsplitn(3, ':').collect();
    if parts.len() < 3 {
        return Err(format!("Invalid location '{}'. Expected file:line:col", location));
    }
    let col: u32 = parts[0].parse().map_err(|_| format!("Invalid column: '{}'", parts[0]))?;
    let line: u32 = parts[1].parse().map_err(|_| format!("Invalid line: '{}'", parts[1]))?;
    let file = parts[2].to_string();
    Ok((file, line, col))
}

/// Detect LSP server command from file extension.
#[cfg(feature = "lsp")]
fn detect_lsp_server(file_path: &str) -> &'static str {
    let ext = Path::new(file_path).extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => "rust-analyzer",
        "py" => "pyright",
        "js" | "jsx" | "ts" | "tsx" => "typescript-language-server",
        "go" => "gopls",
        "java" => "jdtls",
        "cs" => "omnisharp",
        _ => "rust-analyzer",
    }
}

#[cfg(feature = "lsp")]
fn cmd_lsp_def(path: Option<&str>, location: &str, json: bool) -> Result<(), String> {
    let (file, line, col) = parse_location(location)?;
    let mut brain = open_brain(path)?;
    let server = detect_lsp_server(&file);
    brain.enable_lsp(server, ".")?;
    let result = brain.lsp_definition(&file, line, col)?;
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({"command": "definition", "location": location, "result": result}));
    } else {
        if result.is_empty() {
            println!("No definition found for {}", location);
        } else {
            println!("{}", result);
        }
    }
    Ok(())
}

#[cfg(feature = "lsp")]
fn cmd_lsp_refs(path: Option<&str>, location: &str, json: bool) -> Result<(), String> {
    let (file, line, col) = parse_location(location)?;
    let mut brain = open_brain(path)?;
    let server = detect_lsp_server(&file);
    brain.enable_lsp(server, ".")?;
    let result = brain.lsp_references(&file, line, col)?;
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({"command": "references", "location": location, "result": result}));
    } else {
        if result.is_empty() {
            println!("No references found for {}", location);
        } else {
            println!("{}", result);
        }
    }
    Ok(())
}

#[cfg(feature = "lsp")]
fn cmd_lsp_hover(path: Option<&str>, location: &str, json: bool) -> Result<(), String> {
    let (file, line, col) = parse_location(location)?;
    let mut brain = open_brain(path)?;
    let server = detect_lsp_server(&file);
    brain.enable_lsp(server, ".")?;
    let result = brain.lsp_hover(&file, line, col)?;
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({"command": "hover", "location": location, "result": result}));
    } else {
        if result.is_empty() {
            println!("No hover info for {}", location);
        } else {
            println!("{}", result);
        }
    }
    Ok(())
}

#[cfg(feature = "lsp")]
fn cmd_lsp_symbols(path: Option<&str>, query: &str, json: bool) -> Result<(), String> {
    let mut brain = open_brain(path)?;
    brain.enable_lsp("rust-analyzer", ".")?;
    let result = brain.lsp_workspace_symbol(query)?;
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({"command": "workspace_symbol", "query": query, "result": result}));
    } else {
        if result.is_empty() {
            println!("No symbols found for '{}'", query);
        } else {
            println!("{}", result);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `said edit` â€” surgical anchored edit (no whole-file rewrite path)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
#[cfg(feature = "code")]
fn cmd_edit(
    path: Option<&str>,
    file: &str,
    mode: &str,
    symbol: Option<&str>,
    line: Option<usize>,
    anchor: Option<&str>,
    content: Option<&str>,
    content_file: Option<&str>,
    dry_run: bool,
    allow_large: bool,
    no_verify: bool,
    explain: bool,
    json: bool,
) -> Result<(), String> {
    use edit::EditOp;

    // Helper: emit a failure in the JSON/text shape the spec defines, then
    // return a non-zero exit via Err.
    let fail = |msg: String| -> Result<(), String> {
        if json {
            println!("{}", serde_json::json!({ "ok": false, "error": msg }));
        }
        Err(msg)
    };

    // 1. Path safety â€” reject absolute / `..` paths before touching anything.
    if let Err(e) = edit::is_safe_relative_path(file) {
        return fail(e);
    }

    // --explain: pre-validate only. Resolve the target location (--symbol via
    // the brain, or --anchor in the file) and return the valid scope-correct
    // anchors as a menu, WITHOUT editing. Lets a caller pick the right move up
    // front instead of learning it from a failed edit. (code feature only.)
    #[cfg(feature = "code")]
    if explain {
        let ext = std::path::Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or("");
        // Try the on-disk source first (richest: scope-aware suggestions).
        let on_disk = std::fs::read(file).ok().and_then(|b| decode_text(&b));
        if let Some(fc) = on_disk {
            let line = if let Some(name) = symbol {
                let brain = open_brain(path)?;
                brain.sym(name, 50).iter()
                    .find(|r| edit::paths_equal(r.doc_id.split("::").next().unwrap_or(""), file))
                    .map(|r| r.start_line as usize).unwrap_or(1)
            } else if let Some(a) = anchor {
                edit::resolve_text_anchor(&fc, a).unwrap_or(1)
            } else { 1 };
            let suggestions = sca_core::code_search::suggest_anchors(&fc, ext, line);
            let valid: Vec<serde_json::Value> = suggestions.iter().map(|s| serde_json::json!({
                "mode": s.mode, "symbol": s.symbol, "line": s.line, "kind": s.kind, "note": s.note,
            })).collect();
            if json {
                println!("{}", serde_json::json!({
                    "ok": true, "explain": true, "source": "disk", "file": file, "at_line": line,
                    "valid_anchors": valid,
                }));
            } else {
                println!("Valid anchors at {}:{} —", file, line);
                for s in &suggestions { println!("  {} --symbol {} --line {}  ({})", s.mode, s.symbol, s.line, s.note); }
            }
            return Ok(());
        }
        // Brain-only fallback: source not on disk (e.g. brain baked in /app, no
        // checkout). Build the menu from the INDEX alone — it stores name, kind,
        // and start/end lines per symbol. Requires --symbol.
        let name = match symbol {
            Some(n) => n,
            None => return fail(format!(
                "{} not found on disk and no --symbol given; brain-only --explain needs --symbol", file)),
        };
        let brain = open_brain(path)?;
        // Collect index candidates (name, kind, start, end) for this symbol in
        // this file, then let the shared core build the menu — same container
        // classification as the source-based path (no duplicated kind list).
        let index_cands: Vec<(String, String, usize, usize)> = brain.sym(name, 50).iter()
            .filter(|r| edit::paths_equal(r.doc_id.split("::").next().unwrap_or(""), file))
            .map(|r| {
                let kind = r.doc_id.split("::").nth(2)
                    .map(|s| s.split(':').next().unwrap_or(s)).unwrap_or("?").to_string();
                (r.name.clone(), kind, r.start_line as usize, r.end_line as usize)
            }).collect();
        if index_cands.is_empty() {
            return fail(format!("symbol '{}' not found in {} (brain-only lookup)", name, file));
        }
        let suggestions = sca_core::code_search::suggest_anchors_from_candidates(&index_cands);
        let valid: Vec<serde_json::Value> = suggestions.iter().map(|s| serde_json::json!({
            "mode": s.mode, "symbol": s.symbol, "line": s.line, "kind": s.kind, "note": s.note,
        })).collect();
        if json {
            println!("{}", serde_json::json!({
                "ok": true, "explain": true, "source": "brain", "file": file, "valid_anchors": valid,
            }));
        } else {
            println!("Valid anchors for {} in {} (from index) —", name, file);
            for s in &suggestions { println!("  {} --symbol {} --line {}", s.mode, s.symbol, s.line); }
        }
        return Ok(());
    }
    #[cfg(not(feature = "code"))]
    let _ = explain;

    // A real edit needs a mode (mode is optional only to allow --explain).
    if mode.is_empty() {
        return fail("a MODE is required (or use --explain to just see valid anchors)".into());
    }

    // 2. Resolve the new content (inline or from a file). Delete modes need none.
    let is_delete = mode == "delete-symbol";
    let new_text: String = match (content, content_file) {
        (Some(_), Some(_)) => return fail("pass only one of --content / --content-file".into()),
        (Some(c), None) => c.to_string(),
        (None, Some(f)) => match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => return fail(format!("read --content-file {}: {}", f, e)),
        },
        (None, None) if is_delete => String::new(),
        (None, None) => return fail("missing --content or --content-file".into()),
    };

    // 3. Read the on-disk file (the bytes we actually edit).
    let on_disk = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => return fail(format!("read {}: {}", file, e)),
    };
    let file_content = match decode_text(&on_disk) {
        Some(s) => s,
        None => return fail(format!("{} is not valid UTF-8/UTF-16 text", file)),
    };

    // 4. Build the resolved EditOp based on mode + anchor.
    let want_symbol = |m: &str| -> Result<&str, String> {
        symbol.ok_or_else(|| format!("mode '{}' requires --symbol", m))
    };
    let want_anchor = |m: &str| -> Result<&str, String> {
        anchor.ok_or_else(|| format!("mode '{}' requires --anchor", m))
    };

    // Resolve a symbol â†’ (start,end) line range, scoped to --file, via the
    // same lookup `said sym` uses. Also runs an anchor-drift check: the file on
    // disk must still match what the brain indexed for that symbol, else the
    // range is stale and we refuse (recall correctness).
    // append-into-symbol defaults to the largest (enclosing) span when a name
    // is ambiguous â€” e.g. a C# class vs. its same-named 1-line constructor. A
    // --line value, when given, overrides and selects an exact span.
    let prefer_largest = mode == "append-into-symbol";
    let resolve_sym = |name: &str| -> Result<(usize, usize), String> {
        let mut brain = open_brain(path)?;
        let results = brain.sym(name, 50);
        let cands: Vec<edit::SymCandidate> = results.iter().map(|r| edit::SymCandidate {
            doc_id: r.doc_id.clone(),
            name: r.name.clone(),
            start_line: r.start_line as usize,
            end_line: r.end_line as usize,
        }).collect();
        let (start, end) = edit::resolve_symbol_ex(&cands, file, line, prefer_largest)?;
        // The symbol index's end_line can be off-by-one on the closing brace.
        // The brain's stored *content* for the symbol is authoritative, so we
        // derive the true end from its line count and (a) drift-check against
        // the matching on-disk slice, (b) return the corrected range so an
        // edit replaces the whole construct (incl. the closing brace).
        let mut corrected_end = end;
        if let Some(matched) = cands.iter().find(|c|
            edit::paths_equal(c.doc_id.split("::").next().unwrap_or(""), file)
            && c.start_line == start && c.end_line == end)
        {
            if let Some(indexed) = brain.get(&matched.doc_id) {
                let disk_lines: Vec<&str> = file_content.lines().collect();
                let lo = start.saturating_sub(1);
                let snippet_lines = indexed.lines().count().max(1);
                let hi = (lo + snippet_lines).min(disk_lines.len());
                if lo < hi {
                    let ondisk_slice = disk_lines[lo..hi].join("\n");
                    edit::check_symbol_fresh(&indexed, &ondisk_slice)?;
                    corrected_end = hi; // 1-based inclusive end == hi (lo+count)
                }
            }
        }
        Ok((start, corrected_end))
    };

    let op: EditOp = match mode {
        "insert-after-symbol" => {
            let (_, end) = resolve_sym(want_symbol(mode).map_err(|e| { let _ = fail(e.clone()); e })?)
                .map_err(|e| { let _ = fail(e.clone()); e })?;
            EditOp::InsertAfterLine { line: end, text: new_text }
        }
        "insert-before-symbol" => {
            let (start, _) = resolve_sym(want_symbol(mode).map_err(|e| { let _ = fail(e.clone()); e })?)
                .map_err(|e| { let _ = fail(e.clone()); e })?;
            EditOp::InsertBeforeLine { line: start, text: new_text }
        }
        "replace-symbol" => {
            let (start, end) = resolve_sym(want_symbol(mode).map_err(|e| { let _ = fail(e.clone()); e })?)
                .map_err(|e| { let _ = fail(e.clone()); e })?;
            if let Err(e) = edit::check_span(end - start + 1, edit::DEFAULT_MAX_SPAN, allow_large) {
                return fail(e);
            }
            EditOp::ReplaceLines { start, end, text: new_text }
        }
        "delete-symbol" => {
            let (start, end) = resolve_sym(want_symbol(mode).map_err(|e| { let _ = fail(e.clone()); e })?)
                .map_err(|e| { let _ = fail(e.clone()); e })?;
            if let Err(e) = edit::check_span(end - start + 1, edit::DEFAULT_MAX_SPAN, allow_large) {
                return fail(e);
            }
            EditOp::DeleteLines { start, end }
        }
        // Scope-aware: insert at the END of a named scope's body, just before
        // its closing brace. "Add a method to this class" always lands at class
        // scope â€” prevents the new member nesting inside an existing method.
        "append-into-symbol" => {
            let (start, end) = resolve_sym(want_symbol(mode).map_err(|e| { let _ = fail(e.clone()); e })?)
                .map_err(|e| { let _ = fail(e.clone()); e })?;
            // Auto-indent the new member to match sibling indentation: use the
            // indent of the first body line if present, else the closing-brace
            // line's indent + 4 spaces. Keeps inserted members visually correct.
            let disk_lines: Vec<&str> = file_content.lines().collect();
            let body_indent = disk_lines.get(start) // line after the opening line
                .map(|l| edit::indent_of(l).to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    let close_indent = disk_lines.get(end.saturating_sub(1))
                        .map(|l| edit::indent_of(l).to_string()).unwrap_or_default();
                    format!("{}    ", close_indent)
                });
            let indented = edit::reindent_block(&new_text, &body_indent);
            EditOp::InsertBeforeLine { line: end, text: indented }
        }
        "insert-after-text" => {
            let a = want_anchor(mode).map_err(|e| { let _ = fail(e.clone()); e })?;
            let line = match edit::resolve_text_anchor(&file_content, a) {
                Ok(l) => l,
                Err(e) => return fail(e),
            };
            EditOp::InsertAfterLine { line, text: new_text }
        }
        "insert-before-text" => {
            let a = want_anchor(mode).map_err(|e| { let _ = fail(e.clone()); e })?;
            let line = match edit::resolve_text_anchor(&file_content, a) {
                Ok(l) => l,
                Err(e) => return fail(e),
            };
            EditOp::InsertBeforeLine { line, text: new_text }
        }
        "replace-text" => {
            let a = want_anchor(mode).map_err(|e| { let _ = fail(e.clone()); e })?;
            EditOp::ReplaceSubstring { needle: a.to_string(), replacement: new_text }
        }
        // Context modes: --anchor is a (possibly multi-line) block that must
        // occur EXACTLY ONCE â€” disambiguates when a short string repeats.
        "insert-after-context" => {
            let a = want_anchor(mode).map_err(|e| { let _ = fail(e.clone()); e })?;
            let line = match edit::resolve_context_anchor(&file_content, a) {
                Ok(l) => l + a.matches('\n').count(), // after the LAST line of the block
                Err(e) => return fail(e),
            };
            EditOp::InsertAfterLine { line, text: new_text }
        }
        "insert-before-context" => {
            let a = want_anchor(mode).map_err(|e| { let _ = fail(e.clone()); e })?;
            let line = match edit::resolve_context_anchor(&file_content, a) {
                Ok(l) => l,
                Err(e) => return fail(e),
            };
            EditOp::InsertBeforeLine { line, text: new_text }
        }
        "replace-context" => {
            let a = want_anchor(mode).map_err(|e| { let _ = fail(e.clone()); e })?;
            // Confirm uniqueness first (errors on 0 or >1), then replace the block.
            if let Err(e) = edit::resolve_context_anchor(&file_content, a) {
                return fail(e);
            }
            EditOp::ReplaceSubstring { needle: a.to_string(), replacement: new_text }
        }
        other => return fail(format!("unknown mode: {}", other)),
    };

    // 5. Apply the edit (pure) â€” produces the new content + summary.
    let result = match edit::apply_edit(&file_content, &op) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };

    // 5b. Post-edit syntax check (code bundles only): reject an edit that
    //     would leave the file un-parseable, so a bad edit can't land. The
    //     verify_syntax fn only exists when built with the `code` feature.
    #[cfg(feature = "code")]
    if !no_verify {
        let ext = std::path::Path::new(file)
            .extension().and_then(|e| e.to_str()).unwrap_or("");
        if let Err(e) = edit::verify_syntax(&result.content, ext) {
            // Structured, model-agnostic repair menu: compute valid scope-correct
            // anchors at the landing line so any caller (LLM or human) gets
            // copy-paste-ready `said edit` moves instead of just an error string.
            let suggestions = sca_core::code_search::suggest_anchors(
                &file_content, ext, result.applied_at_line);
            let valid: Vec<serde_json::Value> = suggestions.iter().map(|s| serde_json::json!({
                "mode": s.mode, "symbol": s.symbol, "line": s.line, "kind": s.kind, "note": s.note,
            })).collect();
            let msg = format!("{} â€” edit rejected, file unchanged", e);
            if json {
                println!("{}", serde_json::json!({
                    "ok": false, "error": msg, "valid_anchors": valid,
                }));
            }
            return Err(msg);
        }
    }
    #[cfg(not(feature = "code"))]
    let _ = no_verify;

    // 6. Write atomically (temp + rename) unless this is a dry run.
    if !dry_run {
        if let Err(e) = atomic_write(file, &result.content) {
            return fail(e);
        }
    }

    // 7. Report.
    if json {
        println!("{}", serde_json::json!({
            "ok": true,
            "file": file,
            "mode": mode,
            "anchor": symbol.or(anchor).unwrap_or(""),
            "applied_at_line": result.applied_at_line,
            "lines_added": result.lines_added,
            "lines_removed": result.lines_removed,
            "dry_run": dry_run,
        }));
    } else if dry_run {
        println!(
            "DRY RUN â€” would apply {} at line {} (+{} / -{} lines). No file written.",
            mode, result.applied_at_line, result.lines_added, result.lines_removed
        );
    } else {
        println!(
            "Edited {} â€” {} at line {} (+{} / -{} lines).",
            file, mode, result.applied_at_line, result.lines_added, result.lines_removed
        );
    }
    Ok(())
}

/// Write `content` to `file` atomically: write a sibling temp file, then rename
/// over the target so a crash can never leave a half-written source file.
#[cfg(feature = "code")]
fn atomic_write(file: &str, content: &str) -> Result<(), String> {
    let target = Path::new(file);
    let dir = target.parent().filter(|p| !p.as_os_str().is_empty());
    let mut tmp = match dir {
        Some(d) => d.join(format!(".{}.said-edit.tmp", file_stem_or(target))),
        None => PathBuf::from(format!(".{}.said-edit.tmp", file_stem_or(target))),
    };
    // Avoid clobbering an existing tmp from a concurrent edit.
    let mut n = 0;
    while tmp.exists() {
        n += 1;
        let name = format!(".{}.said-edit.{}.tmp", file_stem_or(target), n);
        tmp = match dir {
            Some(d) => d.join(name),
            None => PathBuf::from(name),
        };
    }
    std::fs::write(&tmp, content.as_bytes())
        .map_err(|e| format!("write temp {}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, target)
        .map_err(|e| format!("rename {} -> {}: {}", tmp.display(), target.display(), e))?;
    Ok(())
}

#[cfg(feature = "code")]
fn file_stem_or(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string()
}

// ---------------------------------------------------------------------------
// Lineage commands: reindex + history
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Coding memory — learn-fix (store a verified fix in the Procedural pillar) +
// recall-fix (memory-first retrieval with action/intent-isolated matching).
//
// Built on .said's native Procedural pillar (TRIGGER→STEPS→OUTCOME), NOT a
// bespoke frame type. A verified fix IS a procedural memory: the problem is the
// trigger, the change-set is the steps, OUTCOME=success is the gate result.
// Retrieval uses the proven intent-separation breakthrough: match the problem's
// action/intent residue separately from its target nouns so "add an endpoint"
// never recalls "document an endpoint". See docs/coding-memory-design.md.
// ---------------------------------------------------------------------------

/// Markers inside a coding-memory frame body. The body is a human-readable
/// coding-iteration note (modelled on Claude Code's session memory, adapted for
/// code) followed by the machine-readable change-set JSON and the action-residue
/// used for intent matching.
/// A learned coding iteration. Required: problem + edits. Optional context fields
/// mirror Claude Code's SessionMemory sections so recall reloads FULL context.
/// (The frame format, tags, blake3 id, and Procedural pillar live in the ONE shared
/// writer `sca_core::ask::learn_coding_fix`; this struct is just CLI input.)
#[cfg(feature = "code")]
struct FixIteration<'a> {
    edits_json: &'a str,
    files: Option<&'a str>,      // Claude: "Files and Functions"
    errors: Option<&'a str>,     // Claude: "Errors & Corrections"
    learnings: Option<&'a str>,  // Claude: "Learnings"
}

/// Assemble the human-readable NOTE from the explicit --files/--errors/--learnings
/// fields (the story for an LLM to reload). The TASK line + machine payload (edits +
/// intent residue) are added by the shared writer `sca_core::ask::learn_coding_fix`,
/// so this is note-only — no payload, no hashing, no tags here.
#[cfg(feature = "code")]
fn make_fix_note(it: &FixIteration) -> String {
    // Render each edit as a step ONLY if it carries a real action (a mode/op AND a target). A bare
    // {"file":"x","op":"note"} or {"file":"x"} has no actionable step — rendering it as "1. ? x.rs"
    // produces a CONTENT-FREE pointer that, when this note is recalled + injected, INVITES the agent to
    // go read x.rs to "complete" the dangling step — i.e. it triggers exactly the over-investigation the
    // memory was meant to prevent (measured: A2 spent 13-16 turns re-reading source despite the LEARNINGS
    // line already holding the full answer). The documented intent (docs/15-orchestration §body layout):
    // the note's LEARNINGS carry the recipe; steps are the real change-set, not vague file pointers.
    let steps: Vec<String> = match serde_json::from_str::<serde_json::Value>(it.edits_json) {
        Ok(serde_json::Value::Array(arr)) => arr.iter().enumerate()
            .filter_map(|(i, e)| {
                // A real action verb: prefer `mode`, else `op` if it's not a content-free marker.
                let mode = e.get("mode").and_then(|v| v.as_str())
                    .or_else(|| e.get("op").and_then(|v| v.as_str()).filter(|op| *op != "note"));
                let file = e.get("file").and_then(|v| v.as_str()).unwrap_or("");
                let tgt = e.get("symbol").and_then(|v| v.as_str())
                    .or_else(|| e.get("anchor").and_then(|v| v.as_str())).unwrap_or("");
                match mode {
                    Some(m) if !file.is_empty() => Some(format!("  {}. {} {} {}", i + 1, m, file, tgt).trim_end().to_string()),
                    _ => None, // no actionable step — skip (don't emit a dangling "? file" pointer)
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut note = String::new();
    if let Some(f) = it.files { if !f.trim().is_empty() { note.push_str(&format!("FILES: {}\n", f.trim())); } }
    // LEARNINGS (the answer/recipe) lead; STEPS only when there are real, actionable ones.
    if let Some(l) = it.learnings { if !l.trim().is_empty() { note.push_str(&format!("LEARNINGS: {}\n", l.trim())); } }
    if let Some(e) = it.errors { if !e.trim().is_empty() { note.push_str(&format!("ERRORS: {}\n", e.trim())); } }
    if !steps.is_empty() { note.push_str(&format!("STEPS:\n{}\n", steps.join("\n"))); }
    note.push_str("RESULT: success — built+passed");
    note
}

// Frame format, tags, separators, and the doc_id hash all live in the ONE shared
// writer/reader in sca_core::ask (learn_coding_fix / recall_coding_fix) so the CLI,
// MCP, and orchestration never drift. Nothing to define here.

#[allow(clippy::too_many_arguments)]
#[cfg(feature = "code")]
fn cmd_learn_fix(
    path: Option<&str>, problem: &str, edits: Option<&str>, edits_file: Option<&str>,
    note_file: Option<&str>, files: Option<&str>, errors: Option<&str>, learnings: Option<&str>,
    label: Option<&str>, json: bool,
) -> Result<(), String> {
    let edits_json = match (edits, edits_file) {
        (Some(_), Some(_)) => return Err("pass only one of --edits / --edits-file".into()),
        (Some(e), None) => e.to_string(),
        (None, Some(f)) => std::fs::read_to_string(f).map_err(|e| format!("read --edits-file {}: {}", f, e))?,
        (None, None) => return Err("missing --edits or --edits-file".into()),
    };
    // Strip a leading UTF-8 BOM (Windows-written files) — serde rejects it.
    let edits_json = edits_json.trim_start_matches('\u{feff}').trim().to_string();
    if serde_json::from_str::<serde_json::Value>(&edits_json).is_err() {
        return Err("--edits is not valid JSON".into());
    }
    let mut brain = open_brain(path)?;
    // The stored story: a full client-authored iteration NOTE (the 10-section
    // template filled in, like Claude's session memory) when --note-file is given,
    // otherwise the structured note we assemble from the explicit fields. Either
    // way the machine payload (edits + intent residue) is appended for replay +
    // matching.
    let note = match note_file {
        Some(f) => Some(std::fs::read_to_string(f)
            .map_err(|e| format!("read --note-file {}: {}", f, e))?
            .trim_start_matches('\u{feff}').trim().to_string()),
        None => None,
    };
    // Assemble the human-readable NOTE (the story), then hand it to the ONE shared
    // writer (sca_core::ask::learn_coding_fix) which appends the machine payload,
    // hashes with blake3, and stores it in the native Procedural pillar — byte-
    // identical to what MCP learn_fix and said-orchestration::learn write, so all
    // three share ONE learning store.
    let note = match &note {
        Some(n) => n.clone(),
        None => make_fix_note(&FixIteration {
            edits_json: &edits_json, files, errors, learnings,
        }),
    };
    let doc_id = sca_core::ask::learn_coding_fix(&mut brain, problem, &note, &edits_json, label);
    brain.save()?;
    if json {
        println!("{}", serde_json::json!({ "ok": true, "learned": doc_id, "label": label }));
    } else {
        println!("Learned fix {} (provenance: {})", doc_id, label.unwrap_or("-"));
    }
    Ok(())
}

#[cfg(feature = "code")]
fn cmd_recall_fix(path: Option<&str>, problem: &str, min_similarity: f32, json: bool) -> Result<(), String> {
    let mut brain = open_brain(path)?;
    // The ONE shared reader (sca_core::ask::recall_coding_fix): semantic scorer +
    // shared frame format, identical to what MCP and the orchestrator use.
    match sca_core::ask::recall_coding_fix(&mut brain, problem, min_similarity) {
        Some(hit) => {
            let edits: serde_json::Value = serde_json::from_str(&hit.edits_json)
                .unwrap_or(serde_json::Value::Null);
            let label = brain.frames.get_meta(&hit.doc_id)
                .and_then(|m| m.tags.iter().find(|t| t.starts_with("pr:")).cloned());
            if json {
                println!("{}", serde_json::json!({
                    "ok": true, "fix": {
                        "score": hit.score, "doc_id": hit.doc_id, "provenance": label,
                        "context": hit.note, "edits": edits,
                        "note": "verified fix (built+passed) for a problem of this shape; gate still verifies on apply",
                    }
                }));
            } else {
                println!("Fix ({:.2}) {}  provenance={}", hit.score, hit.doc_id, label.unwrap_or_else(|| "-".into()));
                println!("{}", hit.note);
                println!("  edits:   {}", hit.edits_json);
            }
        }
        None => return emit_no_fix(json, min_similarity),
    }
    Ok(())
}

#[cfg(feature = "code")]
fn emit_no_fix(json: bool, min_similarity: f32) -> Result<(), String> {
    if json {
        println!("{}", serde_json::json!({ "ok": true, "fix": serde_json::Value::Null }));
    } else {
        println!("No known fix above score {:.2} — fall through to the LLM.", min_similarity);
    }
    Ok(())
}


#[cfg(feature = "code")]
fn cmd_reindex(path: Option<&str>, file: &str, json: bool) -> Result<(), String> {
    let file_path = Path::new(file).canonicalize()
        .map_err(|e| format!("Cannot resolve '{}': {}", file, e))?;
    if !file_path.is_file() {
        return Err(format!("Not a file: {}", file));
    }

    let mut brain = open_brain(path)?;

    let content_bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Read failed: {}", e))?;
    let content = decode_text(&content_bytes)
        .ok_or_else(|| "File is not valid UTF-8/UTF-16 text".to_string())?;

    // Find matching Active frames: either by exact rel_path (non-code files)
    // or by "<rel_path>::<symbol>" prefix (AST-chunked code).
    // We match on any frame whose doc_id starts with file basename or path suffix.
    let fname = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let active = brain.frames.active_doc_ids();
    let mut matches: Vec<String> = active.iter()
        .filter(|d| d.contains(fname))
        .map(|s| s.to_string())
        .collect();
    matches.sort();

    if matches.is_empty() {
        return Err(format!("No frames found for file '{}'. Run `said init` first.", file));
    }

    // For code files with multiple AST chunks, reindex each by re-chunking.
    // For simplicity v1: tombstone all matches and re-add the whole file as one frame.
    // The AST pipeline in cmd_init is what gives us per-symbol lineage; for targeted
    // reindex we replace the whole file and each chunk becomes a new tombstone.
    // Used by the code-feature AST reindex path below; unused in lean bundles.
    #[cfg_attr(not(feature = "code"), allow(unused_variables))]
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mut updates: Vec<(String, f32)> = Vec::new();

    #[cfg(feature = "code")]
    {
        if ast_extension(&ext) {
            let chunks = sca_core::code_search::ast_chunk(&content, &ext);
            if !chunks.is_empty() {
                // Determine rel path prefix from any existing match
                // Use the portion before "::" as the path component.
                let rel_prefix: String = matches.iter()
                    .find_map(|d| d.split("::").next().map(|s| s.to_string()))
                    .unwrap_or_else(|| fname.to_string());
                for chunk in &chunks {
                    let kind_parts: Vec<&str> = chunk.kind.split('|').collect();
                    let base_kind = kind_parts[0];
                    // Match cmd_init's doc_id layout (path::NAME::kind:line)
                    // so reindex produces frames that collide with the ones
                    // init created â€” triggering the intended tombstone chain.
                    let doc_id = format!(
                        "{}::{}::{}:{}",
                        rel_prefix, chunk.name, base_kind, chunk.start_line
                    );
                    let title = format!("{}:{}-{} ({})", fname, chunk.start_line, chunk.end_line, base_kind);
                    let (_id, delta) = brain.replace_frame(&doc_id, &chunk.content, Some(&title));
                    updates.push((doc_id, delta));
                }
                let _ = brain.build_index();
                brain.save()?;
                return report_reindex(updates, json);
            }
        }
    }

    // Whole-file fallback
    for doc_id in &matches {
        let (_id, delta) = brain.replace_frame(doc_id, &content, Some(fname));
        updates.push((doc_id.clone(), delta));
    }
    let _ = brain.build_index();
    brain.save()?;
    report_reindex(updates, json)
}

#[cfg(feature = "code")]
fn report_reindex(updates: Vec<(String, f32)>, json: bool) -> Result<(), String> {
    if json {
        let items: Vec<_> = updates.iter().map(|(d, delta)| {
            serde_json::json!({ "doc_id": d, "semantic_delta": delta })
        }).collect();
        println!("{}", serde_json::json!({ "command": "reindex", "updated": items }));
    } else {
        println!("Reindexed {} frame(s):", updates.len());
        for (doc_id, delta) in &updates {
            let label = if *delta == 0.0 { "unchanged" }
                else if *delta < 0.05 { "tiny" }
                else if *delta < 0.15 { "small" }
                else if *delta < 0.35 { "medium" }
                else { "major" };
            println!("  {:>6.3}  {}  {}", delta, label, doc_id);
        }
    }
    Ok(())
}

fn cmd_history(path: Option<&str>, name: &str, json: bool) -> Result<(), String> {
    let brain = open_brain(path)?;

    // Collect ALL doc_ids (Active + Tombstone) so history works even for files
    // that have been fully deleted â€” their lineage is still in the file.
    use std::collections::BTreeSet;
    let mut all_doc_ids: BTreeSet<String> = BTreeSet::new();
    for meta in brain.frames.get_all_frames() {
        if meta.status != sca_core::frames::FrameStatus::Deleted {
            all_doc_ids.insert(meta.doc_id.clone());
        }
    }
    let all: Vec<String> = all_doc_ids.into_iter().collect();

    // Resolve `name` â†’ doc_id. Exact match first, then suffix match on "::name"
    // (which is how cmd_init encodes AST-chunked symbols).
    let mut candidate: Option<String> = None;
    if all.iter().any(|d| d == name) {
        candidate = Some(name.to_string());
    } else {
        let needle = format!("::{}", name);
        let mut hits: Vec<&String> = all.iter().filter(|d| d.ends_with(&needle)).collect();
        if hits.is_empty() {
            hits = all.iter().filter(|d| d.contains(name)).collect();
        }
        if hits.len() == 1 {
            candidate = Some(hits[0].clone());
        } else if hits.len() > 1 {
            if !json {
                eprintln!("Multiple matches â€” pick one with `said history <full doc_id>`:");
                for h in hits.iter().take(20) { eprintln!("  {}", h); }
            }
            return Err(format!("Ambiguous: {} matches for '{}'", hits.len(), name));
        }
    }

    let Some(doc_id) = candidate else {
        return Err(format!("No frame found for '{}'", name));
    };

    let chain = brain.lineage(&doc_id);
    if chain.is_empty() {
        println!("No lineage for {} (genesis only or pruned)", doc_id);
        return Ok(());
    }

    if json {
        let items: Vec<_> = chain.iter().enumerate().map(|(i, m)| {
            serde_json::json!({
                "version": i,
                "frame_id": m.id,
                "status": format!("{:?}", m.status),
                "semantic_delta": m.semantic_delta,
                "superseded_by": m.superseded_by,
                "title": m.title.clone(),
                "bytes": m.uncompressed_len,
            })
        }).collect();
        println!("{}", serde_json::json!({
            "command": "history",
            "doc_id": doc_id,
            "versions": items,
        }));
    } else {
        println!("Cognitive lineage: {}", doc_id);
        println!("  versions: {}", chain.len());
        println!();
        for (i, m) in chain.iter().enumerate() {
            let marker = match m.status {
                sca_core::frames::FrameStatus::Active => "HEAD",
                sca_core::frames::FrameStatus::Tombstone => "past",
                sca_core::frames::FrameStatus::Deleted => "gone",
            };
            let label = if m.semantic_delta == 0.0 { "genesis" }
                else if m.semantic_delta < 0.05 { "tiny" }
                else if m.semantic_delta < 0.15 { "small" }
                else if m.semantic_delta < 0.35 { "medium" }
                else { "major" };
            println!("  v{:<3} [{}] delta={:.3} ({:<7}) {} bytes  frame_id={}",
                i, marker, m.semantic_delta, label, m.uncompressed_len, m.id);
            if let Some(t) = &m.title { println!("         {}", t); }
        }
    }
    Ok(())
}

fn cmd_checkout(
    path: Option<&str>,
    name: &str,
    version: Option<usize>,
    frame: Option<u64>,
    write: bool,
    json: bool,
) -> Result<(), String> {
    if version.is_none() && frame.is_none() {
        return Err("Specify either --version <N> or --frame <id>".to_string());
    }
    let mut brain = open_brain(path)?;

    // Resolve name to doc_id (same logic as cmd_history, Active + Tombstone).
    use std::collections::BTreeSet;
    let mut all_doc_ids: BTreeSet<String> = BTreeSet::new();
    for meta in brain.frames.get_all_frames() {
        if meta.status != sca_core::frames::FrameStatus::Deleted {
            all_doc_ids.insert(meta.doc_id.clone());
        }
    }
    let all: Vec<String> = all_doc_ids.into_iter().collect();

    let mut candidate: Option<String> = None;
    if all.iter().any(|d| d == name) {
        candidate = Some(name.to_string());
    } else {
        let needle = format!("::{}", name);
        let mut hits: Vec<&String> = all.iter().filter(|d| d.ends_with(&needle)).collect();
        if hits.is_empty() {
            hits = all.iter().filter(|d| d.contains(name)).collect();
        }
        if hits.len() == 1 {
            candidate = Some(hits[0].clone());
        } else if hits.len() > 1 {
            eprintln!("Multiple matches â€” pick one with `said checkout <full doc_id>`:");
            for h in hits.iter().take(20) { eprintln!("  {}", h); }
            return Err(format!("Ambiguous: {} matches for '{}'", hits.len(), name));
        }
    }
    let doc_id = candidate.ok_or_else(|| format!("No frame found for '{}'", name))?;

    // Resolve frame_id from either --version index or explicit --frame.
    let target_frame_id: u64 = if let Some(fid) = frame {
        fid
    } else {
        let chain = brain.lineage(&doc_id);
        let v = version.unwrap();
        let meta = chain.get(v)
            .ok_or_else(|| format!("Version {} out of range (lineage has {} entries)", v, chain.len()))?;
        meta.id
    };

    // If --write is set, look up the source root BEFORE the checkout mutation
    // so we can fail fast (no half-done state) and so we don't depend on the
    // new HEAD carrying the tag forward.
    let source_root_for_write: Option<String> = if write {
        let tag_root = brain.lineage(&doc_id).iter()
            .find_map(|m| m.tags.iter().find_map(|t| t.strip_prefix("source:").map(|s| s.to_string())));
        let root = tag_root.ok_or_else(|| format!(
            "No source: tag on {} â€” cannot locate original file. \
             Re-ingest with `said init` to attach a source origin.",
            doc_id
        ))?;
        Some(root)
    } else {
        None
    };

    let (new_id, delta) = brain.checkout_version(target_frame_id)?;
    brain.save()?;

    // --write: overwrite the source file on disk with the restored content.
    //
    // Two cases:
    //   (a) Whole-file frame (doc_id has no "::"): content replaces the
    //       entire file. Simple.
    //   (b) AST-chunked frame (doc_id = "path::symbol"): we re-parse the
    //       current file with tree-sitter, find the symbol's current byte
    //       range (line numbers in the stored title may have drifted), and
    //       splice the restored content into that range.
    let mut wrote_to: Option<String> = None;
    if write {
        let source_root = source_root_for_write.unwrap();
        let restored = brain.get(&doc_id)
            .ok_or_else(|| "Failed to re-read restored HEAD content".to_string())?;

        let (rel_path, symbol): (&str, Option<&str>) = if let Some(idx) = doc_id.find("::") {
            (&doc_id[..idx], Some(&doc_id[idx+2..]))
        } else {
            (doc_id.as_str(), None)
        };
        let abs = Path::new(&source_root).join(rel_path);

        match symbol {
            None => {
                // Whole-file frame â€” overwrite directly
                if let Some(parent) = abs.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&abs, restored.as_bytes())
                    .map_err(|e| format!("Write {} failed: {}", abs.display(), e))?;
            }
            // `sym` is consumed only by the code-feature AST splice path below.
            #[cfg_attr(not(feature = "code"), allow(unused_variables))]
            Some(sym) => {
                #[cfg(feature = "code")]
                {
                    // Splice: re-parse current file, find the symbol's live
                    // line range, replace those lines with the restored chunk.
                    let current = std::fs::read_to_string(&abs)
                        .map_err(|e| format!("Read {} failed: {} â€” create the file first or use a whole-file frame", abs.display(), e))?;
                    let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    let chunks = sca_core::code_search::ast_chunk(&current, &ext);
                    let chunk = chunks.iter().find(|c| c.name == sym)
                        .ok_or_else(|| format!(
                            "Symbol '{}' not found in current {} â€” was it renamed or deleted? \
                             Checkout still updated the brain; the disk file is unchanged.",
                            sym, abs.display()
                        ))?;

                    // Splice by line range. start_line/end_line are 1-based.
                    let lines: Vec<&str> = current.lines().collect();
                    let start_idx = chunk.start_line.saturating_sub(1);
                    let end_idx = chunk.end_line.min(lines.len());
                    let mut rebuilt = String::new();
                    for line in &lines[..start_idx] {
                        rebuilt.push_str(line);
                        rebuilt.push('\n');
                    }
                    rebuilt.push_str(restored.trim_end_matches('\n'));
                    rebuilt.push('\n');
                    for line in &lines[end_idx..] {
                        rebuilt.push_str(line);
                        rebuilt.push('\n');
                    }
                    std::fs::write(&abs, rebuilt.as_bytes())
                        .map_err(|e| format!("Write {} failed: {}", abs.display(), e))?;
                }
                #[cfg(not(feature = "code"))]
                {
                    return Err(format!(
                        "--write for AST-chunked symbols requires the 'code' feature. \
                         Rebuild said with --features code."
                    ));
                }
            }
        }
        wrote_to = Some(abs.to_string_lossy().to_string());
    }

    if json {
        println!("{}", serde_json::json!({
            "command": "checkout",
            "doc_id": doc_id,
            "restored_frame_id": target_frame_id,
            "new_head_frame_id": new_id,
            "semantic_delta": delta,
            "wrote_to": wrote_to,
        }));
    } else {
        println!("Checked out frame_id={} as new HEAD for {}", target_frame_id, doc_id);
        println!("  new HEAD frame_id={}  semantic_delta={:.3}", new_id, delta);
        if let Some(p) = wrote_to {
            println!("  wrote restored content to {}", p);
        }
        println!("  (previous HEAD is now a tombstone â€” run `said history {}` to see the chain)", name);
    }
    Ok(())
}

/// Simple timestamp without pulling in chrono.
fn chrono_free_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// said dev-spec <action> â€” Dev Spec source-of-truth pipeline
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

#[cfg(feature = "forge")]
fn dev_spec_target_or_cwd(target: Option<&Path>) -> PathBuf {
    target
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[cfg(feature = "forge")]
fn cmd_dev_spec_parse(target: Option<&Path>, client: &str) -> Result<(), String> {
    let root = dev_spec_target_or_cwd(target);
    let dev_planning = root.join("4-expectations").join("Dev Planning");
    let standard = said_forge::OpenApiStandard::load(&root, Some(client))
        .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
    let endpoints = said_forge::dev_spec::parser::walk_dev_spec_dir(&dev_planning, &standard)?;
    let out_dir = root.join("2-progress");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create progress dir: {e}"))?;
    let out_path = out_dir.join(format!("{}-dev-spec.json", client.to_lowercase()));
    let json = serde_json::to_string_pretty(&endpoints)
        .map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&out_path, json).map_err(|e| format!("write: {e}"))?;
    eprintln!(
        "âœ“ {} endpoints â†’ {}",
        endpoints.len(),
        out_path.display()
    );
    Ok(())
}

#[cfg(feature = "forge")]
fn cmd_dev_spec_erd(target: Option<&Path>, client: &str) -> Result<(), String> {
    let root = dev_spec_target_or_cwd(target);
    let dev_planning = root.join("4-expectations").join("Dev Planning");
    let standard = said_forge::OpenApiStandard::load(&root, Some(client))
        .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
    let endpoints = said_forge::dev_spec::parser::walk_dev_spec_dir(&dev_planning, &standard)?;
    let erd = said_forge::dev_spec::erd::derive_erd(&endpoints);
    let out_dir = root.join("2-progress");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create progress dir: {e}"))?;
    let json_path = out_dir.join(format!("{}-erd.json", client.to_lowercase()));
    let md_path = out_dir.join(format!("{}-erd.md", client.to_lowercase()));
    std::fs::write(&json_path, said_forge::dev_spec::erd::to_canonical_json(&erd)?)
        .map_err(|e| format!("write json: {e}"))?;
    std::fs::write(&md_path, said_forge::dev_spec::erd::render_mermaid(&erd))
        .map_err(|e| format!("write md: {e}"))?;
    eprintln!(
        "âœ“ ERD: {} entities â†’ {} + {}",
        erd.entities.len(),
        json_path.display(),
        md_path.display()
    );
    Ok(())
}

#[cfg(feature = "forge")]
fn cmd_dev_spec_generate_tables(target: Option<&Path>, client: &str) -> Result<(), String> {
    let root = dev_spec_target_or_cwd(target);
    let dev_planning = root.join("4-expectations").join("Dev Planning");
    let standard = said_forge::OpenApiStandard::load(&root, Some(client))
        .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
    let endpoints = said_forge::dev_spec::parser::walk_dev_spec_dir(&dev_planning, &standard)?;
    let erd = said_forge::dev_spec::erd::derive_erd(&endpoints);
    let catalog = said_forge::sql_catalog::build_catalog(&root)
        .map_err(|e| format!("build catalog: {e}"))?;
    let decisions = said_forge::dev_spec::borrow::decide_borrows(&erd, &catalog);

    let out_dir = root
        .join("5-deliverables")
        .join(client)
        .join("sql");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create out dir: {e}"))?;
    let tables_sql = said_forge::dev_spec::sql_emit::emit_all_tables(&erd, &decisions);
    let tables_path = out_dir.join("tables.sql");
    std::fs::write(&tables_path, tables_sql).map_err(|e| format!("write: {e}"))?;

    // Borrow log.
    let borrows_log = root
        .join("2-progress")
        .join(format!("{}-erd-borrows.md", client.to_lowercase()));
    std::fs::create_dir_all(borrows_log.parent().unwrap()).map_err(|e| format!("mkdir: {e}"))?;
    let mut log = String::from("# ERD Borrow Decisions\n\n");
    for d in &decisions {
        log.push_str(&format!(
            "- **{}**: {} â†’ `{}.{}` (score {})\n  - {}\n",
            d.entity,
            d.borrowed_from.as_deref().unwrap_or("(fresh)"),
            d.schema,
            d.table_name,
            d.score,
            d.reason
        ));
    }
    std::fs::write(&borrows_log, log).map_err(|e| format!("write log: {e}"))?;
    eprintln!(
        "âœ“ {} tables â†’ {} (log: {})",
        erd.entities.len(),
        tables_path.display(),
        borrows_log.display()
    );
    Ok(())
}

#[cfg(feature = "forge")]
fn cmd_dev_spec_amend_registry(target: Option<&Path>, client: &str) -> Result<(), String> {
    let root = dev_spec_target_or_cwd(target);
    let dev_planning = root.join("4-expectations").join("Dev Planning");
    let standard = said_forge::OpenApiStandard::load(&root, Some(client))
        .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
    let endpoints = said_forge::dev_spec::parser::walk_dev_spec_dir(&dev_planning, &standard)?;
    let erd = said_forge::dev_spec::erd::derive_erd(&endpoints);
    let out_dir = root
        .join("5-deliverables")
        .join(client)
        .join("sql");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create out dir: {e}"))?;
    let merge_sql = said_forge::dev_spec::registry_amend::emit_registry_merge(&erd);
    let path = out_dir.join("registry-amendments.sql");
    std::fs::write(&path, merge_sql).map_err(|e| format!("write: {e}"))?;
    eprintln!(
        "âœ“ {} registry-amendment MERGE statements â†’ {}",
        endpoints.len(),
        path.display()
    );
    Ok(())
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// said test â€” Step 10 execution-level testing
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// Spin up the synthetic HTTP server backed by the running sandbox,
/// drive it with the Bruno collection's `1. Local` env, run L3
/// lifecycle per entity in build order, write `test-report.md`.
#[cfg(feature = "forge-sql-verify")]
fn cmd_test(
    target: Option<&Path>,
    client: &str,
    bruno: Option<&Path>,
) -> Result<(), String> {
    let workspace_root = target
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let deliverables_root = workspace_root
        .join("5-deliverables")
        .join(client);
    if !deliverables_root.join("api-specification.generated.yml").exists() {
        return Err(format!(
            "no spec at {}/api-specification.generated.yml â€” run `said forge docs --client {} --verify-against-sandbox` first",
            deliverables_root.display(), client,
        ));
    }
    // Bruno fixtures come from TWO roots â€” hand-authored takes priority
    // over machine-generated:
    //   1. `dt/<CLIENT>/feapiTxnGlobal/.bruno/<CLIENT>-Global/1. Local/`
    //      â€” engineer-curated source-of-truth, never modified by tooling
    //   2. `5-deliverables/<CLIENT>/bruno-generated/`
    //      â€” output of `dtcard/.forge/generate_bruno.py`, fills gaps
    //        for entities that have no hand-authored fixture yet
    // The harness merges them; (entity_folder, name) duplicates resolve
    // in favour of the hand-authored copy.
    let mut bruno_roots: Vec<std::path::PathBuf> = Vec::new();
    if let Some(p) = bruno {
        bruno_roots.push(p.to_path_buf());
    } else {
        // Hand-authored, repo-level.
        if let Some(repo_root) = workspace_root.parent() {
            bruno_roots.push(
                repo_root.join("dt").join(client)
                    .join("feapiTxnGlobal").join(".bruno")
                    .join(format!("{}-Global", client))
                    .join("1. Local"),
            );
        }
        // Machine-generated, dtcard-local.
        bruno_roots.push(deliverables_root.join("bruno-generated"));
    }
    let any_root_exists = bruno_roots.iter().any(|r| r.exists());
    if !any_root_exists {
        return Err(format!(
            "no Bruno collection found in any of: {}",
            bruno_roots.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }

    let sandbox = match said_forge::sql_verify::discover_sandbox_with_hint(Some(client)) {
        Some(s) => s,
        None => return Err(format!(
            "no `said-sbx-{}-*` container running. Run `said sandbox txn --up` first.",
            client.to_lowercase(),
        )),
    };
    eprintln!(
        "  â†’ testing against sandbox `{}` on port {}",
        sandbox.container_name, sandbox.host_port,
    );

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("tokio runtime: {}", e))?;

    // Wipe test-created rows before the harness runs so each pass
    // starts from a clean state. Without this, idempotency / duplicate-
    // key PrcCodes from the previous run mask real coverage gaps.
    // Bootstrap-seed rows (gsv, eul, oit, mbl 'CEN', etc.) are
    // preserved â€” those are owned by `forge docs --verify-against-sandbox`.
    let reset_path = workspace_root.join(".forge").join("test-data-reset.sql");
    if reset_path.exists() {
        eprintln!("  â†’ wiping test data via {}", reset_path.display());
        let sql = std::fs::read_to_string(&reset_path)
            .map_err(|e| format!("read {}: {}", reset_path.display(), e))?;
        rt.block_on(async {
            said_forge::sql_verify::apply_sql_script(sandbox.host_port, &sql).await
        }).map_err(|e| format!("test-data reset: {}", e))?;
    }

    // OldData rich-fixture pass â€” re-seed the production-shaped lookup
    // rows the test-data-reset just wiped (mbl / cbl / fbl / crv / acn).
    // This is "Idea 1: rich-fixture mode" â€” synthetic POSTs run against
    // real production lookup codes/GUIDs instead of bootstrap stubs.
    // Uses `IF NOT EXISTS` guards (see dtcard/.forge/load_olddata.py) so
    // the seed is idempotent across repeat harness runs.
    //
    // Generated by `py dtcard/.forge/load_olddata.py --client <CLIENT>`;
    // re-run that script whenever a new oldData/<table>.sql is added or
    // an existing one changes shape. The seed file is committed under
    // `5-deliverables/<CLIENT>/sql/olddata-seed.sql`.
    let olddata_path = deliverables_root.join("sql").join("olddata-seed.sql");
    if olddata_path.exists() {
        eprintln!("  â†’ seeding real oldData via {}", olddata_path.display());
        let sql = std::fs::read_to_string(&olddata_path)
            .map_err(|e| format!("read {}: {}", olddata_path.display(), e))?;
        rt.block_on(async {
            said_forge::sql_verify::apply_sql_script(sandbox.host_port, &sql).await
        }).map_err(|e| format!("oldData seed: {}", e))?;
    }

    let summary = rt.block_on(async {
        said_forge::test_harness::run_test_pass(
            &sandbox, &workspace_root, &deliverables_root, &bruno_roots,
        ).await
    })?;

    eprintln!(
        "âœ“ test-report.md written. {} green, {} missing-fixture, {} broken-step (of {} entities walked).",
        summary.passed_count(),
        summary.missing_fixture_count(),
        summary.broken_step_count(),
        summary.entities.len(),
    );
    // Non-zero exit only when there are broken-step entities. Missing-
    // fixture is a Phase-A coverage gap (the workflow flags it for
    // hand-authoring), not a test failure â€” don't fail CI for it.
    if summary.broken_step_count() > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// said forge <verb> â€” spec-driven workspace generator
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

#[cfg(feature = "forge")]
mod forge_cli {
    use super::{ForgeVerb, SaidFile};
    use said_forge::{
        frame, llm::provider_from_config, run_one, ClaudeAdapter, CircuitBreaker,
        ForgeConfig, RunOptions, SaidFileBrain, SourceRegistry, Story, StoryStatus,
    };
    use std::io::Write;
    use std::path::{Path, PathBuf};

    /// Default list of Bruno collection roots for a given client.
    /// The fixer (`dtcard/.forge/fix_bruno.py`) and the coverage emitter
    /// both consult these. Returns absolute paths; non-existent roots
    /// are filtered upstream by callers that care.
    fn bruno_collection_roots(workspace_root: &Path, client_hint: &str) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        // Per-workspace BRU_Files: dtcard's local copy used during planning.
        roots.push(
            workspace_root.join("4-expectations").join("BRU_Files"),
        );
        // Real Bruno collection at repo-root/dt/<CLIENT>/feapiTxnGlobal/.bruno/...
        if let Some(repo_root) = workspace_root.parent() {
            roots.push(
                repo_root.join("dt").join(client_hint)
                    .join("feapiTxnGlobal").join(".bruno")
                    .join(format!("{}-Global", client_hint))
                    .join("1. Local"),
            );
        }
        // Machine-generated fixtures (output of `dtcard/.forge/generate_bruno.py`)
        // â€” included so the coverage report counts an endpoint as
        // "Bruno present" if it has either a hand-authored OR a
        // generated fixture (the harness consumes both).
        roots.push(
            workspace_root.join("5-deliverables").join(client_hint)
                .join("bruno-generated"),
        );
        roots
    }

    /// Top-level dispatch. Wrapped in a tokio runtime so async code (LLM,
    /// URL fetch) can run inside the otherwise-sync said-cli.
    pub fn dispatch(path: Option<&str>, verb: ForgeVerb, json: bool) -> Result<(), String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("tokio runtime: {}", e))?;
        rt.block_on(run(path, verb, json))
    }

    /// Multi-client orchestrator dispatch. Mirrors `dispatch` but for
    /// the `said clients <verb>` family.
    pub fn clients_dispatch(
        path: Option<&str>,
        verb: super::ClientsVerb,
        json: bool,
    ) -> Result<(), String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("tokio runtime: {}", e))?;
        rt.block_on(clients_run(path, verb, json))
    }

    async fn clients_run(
        _path: Option<&str>,
        verb: super::ClientsVerb,
        json: bool,
    ) -> Result<(), String> {
        match verb {
            super::ClientsVerb::List { target } => {
                cmd_clients_list(target.as_deref(), json)
            }
            super::ClientsVerb::Run {
                clients,
                target,
                port,
                max_enum_rows,
                no_docs,
                keep_running,
                from_sql,
            } => {
                cmd_clients_run(
                    target.as_deref(),
                    &clients,
                    port,
                    max_enum_rows,
                    no_docs,
                    keep_running,
                    from_sql,
                    json,
                )
                .await
            }
            super::ClientsVerb::Status { target } => {
                cmd_clients_status(target.as_deref(), json)
            }
        }
    }

    /// Walk `<workspace>/1-ground-truth/` and return one entry per
    /// immediate subdirectory. Each subdirectory is treated as one
    /// client. Files at the ground-truth root (e.g. README.md) are
    /// ignored. Returns sorted alphabetically for stable output.
    fn discover_clients(target: &Path) -> Result<Vec<ClientInfo>, String> {
        let gt_root = target.join("1-ground-truth");
        if !gt_root.is_dir() {
            return Err(format!(
                "no 1-ground-truth/ under {} â€” not a forge workspace?",
                target.display()
            ));
        }
        let mut clients: Vec<ClientInfo> = Vec::new();
        for entry in std::fs::read_dir(&gt_root)
            .map_err(|e| format!("read {}: {}", gt_root.display(), e))?
        {
            let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = match p.file_name().and_then(|n| n.to_str()) {
                Some(n) if !n.starts_with('.') => n.to_string(),
                _ => continue,
            };
            let (file_count, latest_source_mtime) = walk_client_files(&p);
            let spec_path = target
                .join("5-deliverables")
                .join(&name)
                .join("api-specification.generated.yml");
            let spec_mtime = spec_path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok());
            clients.push(ClientInfo {
                name,
                root: p,
                file_count,
                latest_source_mtime,
                spec_path: if spec_path.exists() { Some(spec_path) } else { None },
                spec_mtime,
            });
        }
        clients.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(clients)
    }

    fn walk_client_files(dir: &Path) -> (usize, Option<std::time::SystemTime>) {
        let mut count = 0usize;
        let mut latest: Option<std::time::SystemTime> = None;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let entries = match std::fs::read_dir(&d) {
                Ok(it) => it,
                Err(_) => continue,
            };
            for e in entries.filter_map(|e| e.ok()) {
                let p = e.path();
                if p.is_dir() {
                    // Skip VCS dirs to keep the walk fast.
                    if p.file_name().and_then(|n| n.to_str()) == Some(".git") {
                        continue;
                    }
                    stack.push(p);
                } else {
                    count += 1;
                    if let Ok(md) = std::fs::metadata(&p) {
                        if let Ok(m) = md.modified() {
                            if latest.map(|l| m > l).unwrap_or(true) {
                                latest = Some(m);
                            }
                        }
                    }
                }
            }
        }
        (count, latest)
    }

    struct ClientInfo {
        name: String,
        root: PathBuf,
        file_count: usize,
        latest_source_mtime: Option<std::time::SystemTime>,
        spec_path: Option<PathBuf>,
        spec_mtime: Option<std::time::SystemTime>,
    }

    impl ClientInfo {
        /// Source is "newer" than the spec if either:
        /// - no spec exists, or
        /// - source mtime > spec mtime.
        fn source_newer_than_spec(&self) -> bool {
            match (self.latest_source_mtime, self.spec_mtime) {
                (Some(_), None) => true,
                (Some(src), Some(spec)) => src > spec,
                _ => false,
            }
        }

        /// Human-friendly age of the most recent spec.
        fn spec_age(&self) -> String {
            match self.spec_mtime {
                None => "never".to_string(),
                Some(m) => match std::time::SystemTime::now().duration_since(m) {
                    Ok(d) => humanize_duration(d),
                    Err(_) => "(future?)".to_string(),
                },
            }
        }
    }

    fn humanize_duration(d: std::time::Duration) -> String {
        let s = d.as_secs();
        if s < 60 { format!("{}s ago", s) }
        else if s < 3600 { format!("{}m ago", s / 60) }
        else if s < 86400 { format!("{}h ago", s / 3600) }
        else { format!("{}d ago", s / 86400) }
    }

    fn target_or_cwd(target: Option<&Path>) -> PathBuf {
        target.map(Path::to_path_buf).unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    fn cmd_clients_list(target: Option<&Path>, json: bool) -> Result<(), String> {
        let workspace = target_or_cwd(target);
        let clients = discover_clients(&workspace)?;
        if json {
            let payload = serde_json::json!({
                "workspace": workspace.display().to_string(),
                "clients": clients.iter().map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "root": c.root.display().to_string(),
                        "file_count": c.file_count,
                        "spec_exists": c.spec_path.is_some(),
                        "spec_age": c.spec_age(),
                        "source_newer_than_spec": c.source_newer_than_spec(),
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", payload);
            return Ok(());
        }
        if clients.is_empty() {
            println!("No clients found under {}/1-ground-truth/", workspace.display());
            return Ok(());
        }
        println!("Workspace: {}", workspace.display());
        println!();
        println!("{:<20} {:>8} {:>15} {:>10}", "Client", "Files", "Last spec", "Stale?");
        println!("{}", "â”€".repeat(60));
        for c in &clients {
            let stale = if c.source_newer_than_spec() { "yes" } else { "no" };
            println!("{:<20} {:>8} {:>15} {:>10}", c.name, c.file_count, c.spec_age(), stale);
        }
        Ok(())
    }

    fn cmd_clients_status(target: Option<&Path>, json: bool) -> Result<(), String> {
        // For v1, status == list. The two diverge later when we add
        // per-client sandbox health, last-error tracking, etc.
        cmd_clients_list(target, json)
    }

    async fn cmd_clients_run(
        target: Option<&Path>,
        names: &[String],
        start_port: u16,
        max_enum_rows: usize,
        no_docs: bool,
        keep_running: bool,
        from_sql: bool,
        json: bool,
    ) -> Result<(), String> {
        let workspace = target_or_cwd(target);
        let all = discover_clients(&workspace)?;
        let selected: Vec<ClientInfo> = if names.is_empty() {
            all
        } else {
            let want: std::collections::HashSet<String> = names
                .iter()
                .map(|n| n.to_lowercase())
                .collect();
            all.into_iter()
                .filter(|c| want.contains(&c.name.to_lowercase()))
                .collect()
        };
        if selected.is_empty() {
            return Err(format!(
                "no matching clients (asked for [{}])",
                names.join(", ")
            ));
        }

        let brain_path = locate_workspace_brain(&workspace)?;
        let said_bin = std::env::current_exe()
            .map_err(|e| format!("locate self: {}", e))?;
        let workspace_str = workspace.to_string_lossy().to_string();
        let brain_str = brain_path.to_string_lossy().to_string();

        let mut summary: Vec<serde_json::Value> = Vec::new();

        for (i, client) in selected.iter().enumerate() {
            let port = start_port + i as u16;
            if !json {
                println!("\nâ•â• {} ({}/{}) â•â•", client.name, i + 1, selected.len());
                println!("  Port:        {}", port);
                println!("  Source files: {}", client.file_count);
            }

            // 1. sandbox up â€” spawn `said sandbox <Client> --up --port N`.
            // Use the same binary so feature flags + behavior are consistent.
            let sandbox_status = std::process::Command::new(&said_bin)
                .args([
                    "--path", &brain_str,
                    "sandbox", &client.name,
                    "--port", &port.to_string(),
                    "--up",
                ])
                .status()
                .map_err(|e| format!("spawn sandbox for {}: {}", client.name, e))?;
            if !sandbox_status.success() {
                if !json {
                    println!("  âœ— sandbox up FAILED for {}", client.name);
                }
                summary.push(serde_json::json!({
                    "client": client.name,
                    "port": port,
                    "sandbox_ok": false,
                    "spec_ok": false,
                }));
                continue;
            }
            if !json {
                println!("  âœ“ sandbox up");
            }

            // 1.5 Dev-Spec-driven stages: generate tables + registry
            // amendments, then apply them to the live sandbox before
            // forge-docs runs. The registry-first OpenAPI generator
            // (already shipped) reads `ars_Api_Rule_Settings` from the
            // sandbox, so populating the registry here means every
            // Dev Spec endpoint shows up naturally in the emitted spec.
            //
            // Errors are non-fatal: we want forge-docs to still run
            // even if Dev Spec stages fail (graceful degradation).
            let dev_spec_status = std::process::Command::new(&said_bin)
                .args([
                    "--path", &brain_str,
                    "dev-spec", "generate-tables",
                    "--target", &workspace_str,
                    "--client", &client.name,
                ])
                .status()
                .map_err(|e| format!("dev-spec generate-tables: {e}"))?;
            if !dev_spec_status.success() {
                eprintln!("  âœ— dev-spec generate-tables FAILED");
            } else {
                let tables_path = workspace
                    .join("5-deliverables")
                    .join(&client.name)
                    .join("sql")
                    .join("tables.sql");
                let container = format!(
                    "said-sbx-{}-{}",
                    client.name.to_lowercase(),
                    port
                );
                if let Err(e) = apply_sql_to_sandbox(&container, &tables_path) {
                    eprintln!("  âœ— apply tables.sql: {e}");
                } else {
                    eprintln!("  âœ“ Dev Spec tables applied");
                }
            }

            let amend_status = std::process::Command::new(&said_bin)
                .args([
                    "--path", &brain_str,
                    "dev-spec", "amend-registry",
                    "--target", &workspace_str,
                    "--client", &client.name,
                ])
                .status()
                .map_err(|e| format!("dev-spec amend-registry: {e}"))?;
            if !amend_status.success() {
                eprintln!("  âœ— dev-spec amend-registry FAILED");
            } else {
                let amend_path = workspace
                    .join("5-deliverables")
                    .join(&client.name)
                    .join("sql")
                    .join("registry-amendments.sql");
                let container = format!(
                    "said-sbx-{}-{}",
                    client.name.to_lowercase(),
                    port
                );
                if let Err(e) = apply_sql_to_sandbox(&container, &amend_path) {
                    eprintln!("  âœ— apply registry-amendments.sql: {e}");
                } else {
                    eprintln!("  âœ“ Registry amendments applied");
                }
            }

            // 2. forge docs â€” only when not skipped. Run via the binary
            // for consistent feature-gated behavior.
            let mut spec_ok = false;
            if !no_docs {
                let mut docs_args: Vec<String> = vec![
                    "forge".into(),
                    "docs".into(),
                    "--target".into(),
                    workspace_str.clone(),
                    "--client".into(),
                    client.name.clone(),
                    "--verify-against-sandbox".into(),
                    "--max-enum-rows".into(),
                    max_enum_rows.to_string(),
                ];
                if from_sql {
                    docs_args.push("--from-sql".into());
                }
                let docs_status = std::process::Command::new(&said_bin)
                    .args(&docs_args)
                    .status()
                    .map_err(|e| format!("spawn forge docs for {}: {}", client.name, e))?;
                spec_ok = docs_status.success();
                if !json {
                    if spec_ok {
                        println!("  âœ“ spec generated â†’ 5-deliverables/{}/", client.name);
                    } else {
                        println!("  âœ— spec generation FAILED");
                    }
                }
            }

            // 3. tear down (unless --keep-running). Find the actual
            // container name by docker filter on the client token â€”
            // sandbox uses lowercase regardless of arg casing.
            if !keep_running {
                let container_filter = format!("name=said-sbx-{}-", client.name.to_lowercase());
                let docker_ls = std::process::Command::new("docker")
                    .args(["ps", "-q", "--filter", &container_filter])
                    .output();
                if let Ok(out) = docker_ls {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    for cid in stdout.split_whitespace() {
                        let _ = std::process::Command::new("docker")
                            .args(["rm", "-f", cid])
                            .status();
                    }
                }
                if !json {
                    println!("  âœ“ container removed");
                }
            }

            summary.push(serde_json::json!({
                "client": client.name,
                "port": port,
                "sandbox_ok": true,
                "spec_ok": spec_ok,
            }));
        }

        if json {
            println!("{}", serde_json::json!({
                "workspace": workspace.display().to_string(),
                "results": summary,
            }));
        } else {
            println!("\nâ•â• Summary â•â•");
            for r in &summary {
                let name = r["client"].as_str().unwrap_or("?");
                let port = r["port"].as_u64().unwrap_or(0);
                let sb = r["sandbox_ok"].as_bool().unwrap_or(false);
                let sp = r["spec_ok"].as_bool().unwrap_or(false);
                let mark = if sb && (sp || no_docs) { "âœ“" } else { "âœ—" };
                println!(
                    "  {} {:<20} port={:<5}  sandbox={}  spec={}",
                    mark,
                    name,
                    port,
                    if sb { "ok" } else { "fail" },
                    if no_docs { "skipped" } else if sp { "ok" } else { "fail" }
                );
            }
        }
        Ok(())
    }

    async fn run(path: Option<&str>, verb: ForgeVerb, json: bool) -> Result<(), String> {
        // `forge init` and `forge plan` operate on the workspace directory,
        // not an existing .said brain â€” handle them before `resolve_path`.
        if let ForgeVerb::Init { project, target, merge, force } = &verb {
            return cmd_init(project, target.as_deref(), *merge, *force, json);
        }
        if let ForgeVerb::Plan { target, reconfigure } = &verb {
            return cmd_plan(target.as_deref(), *reconfigure, json);
        }
        if let ForgeVerb::Sync { target, dry_run, force } = &verb {
            return cmd_sync(target.as_deref(), *dry_run, *force, json);
        }
        if let ForgeVerb::Gaps { target, out } = &verb {
            return cmd_gaps(target.as_deref(), out.as_deref(), json).await;
        }
        if let ForgeVerb::Regen {
            target,
            from,
            out,
            name,
            force,
        } = &verb
        {
            return cmd_regen(
                target.as_deref(),
                from.as_deref(),
                out.as_deref(),
                name,
                *force,
                json,
            );
        }
        if let ForgeVerb::Docs {
            target,
            out,
            only,
            verify_against_sandbox,
            max_enum_rows,
            client,
            from_sql,
        } = &verb
        {
            return cmd_docs(
                target.as_deref(),
                out.as_deref(),
                only.as_deref(),
                *verify_against_sandbox,
                *max_enum_rows,
                client.as_deref(),
                *from_sql,
                json,
            )
            .await;
        }
        if let ForgeVerb::Viz { target, out } = &verb {
            return cmd_viz(target.as_deref(), out.as_deref(), json).await;
        }
        if let ForgeVerb::Snapshot { module, target } = &verb {
            return cmd_forge_snapshot(target.as_deref(), module, json);
        }
        if let ForgeVerb::Sandbox { module, target, port, no_up } = &verb {
            return cmd_forge_sandbox(
                target.as_deref(),
                module,
                *port,
                *no_up,
                json,
            );
        }
        if let ForgeVerb::Test { target, client, only } = &verb {
            return cmd_forge_test(
                target.as_deref(),
                client.as_deref(),
                only.as_deref(),
                json,
            )
            .await;
        }
        if let ForgeVerb::Rule { rule, input } = &verb {
            // Pure string-in-string-out â€” no brain needed. Calls the
            // canonical helpers in said-forge so Python tooling and Rust
            // tooling apply identical rules.
            use said_forge::dev_spec::parser::{
                pluralise_segment, singularise, snake_to_camel,
            };
            let out = match rule.as_str() {
                "singularise" | "singular" => singularise(input),
                "pluralise"   | "plural"   => pluralise_segment(input),
                "camel" | "snake_to_camel" => snake_to_camel(input),
                other => return Err(format!(
                    "unknown rule `{}`: try singularise / pluralise / camel",
                    other,
                )),
            };
            print!("{}", out);
            return Ok(());
        }
        if let ForgeVerb::Render { framework, profile, bundle, id } = &verb {
            return cmd_proc_framework_render(framework, profile, bundle.as_deref(), id.as_deref(), json);
        }
        if let ForgeVerb::RenderCs { framework, profile, bundle, id } = &verb {
            return cmd_proc_framework_render_cs(framework, profile, bundle.as_deref(), id.as_deref(), json);
        }
        if let ForgeVerb::Audit { framework, profile, bundle, id, deployed_root, verbose } = &verb {
            return cmd_proc_framework_audit(
                framework,
                profile,
                bundle.as_deref(),
                id.as_deref(),
                deployed_root,
                *verbose,
                json,
            );
        }
        if let ForgeVerb::AuditCs { framework, profile, bundle, id, cs_query_root, verbose } = &verb {
            return cmd_proc_framework_audit_cs(
                framework,
                profile,
                bundle.as_deref(),
                id.as_deref(),
                cs_query_root,
                *verbose,
                json,
            );
        }

        let brain_path = resolve_path(path)?;
        let project_root = project_root_from(&brain_path);

        let mut cfg = ForgeConfig::from_file(
            &project_root.join(".said").join("config.toml"),
        )
        .unwrap_or_else(|_| ForgeConfig::default());
        cfg.apply_env();

        match verb {
            ForgeVerb::Init { .. }
            | ForgeVerb::Plan { .. }
            | ForgeVerb::Sync { .. }
            | ForgeVerb::Gaps { .. }
            | ForgeVerb::Regen { .. }
            | ForgeVerb::Docs { .. }
            | ForgeVerb::Viz { .. }
            | ForgeVerb::Snapshot { .. }
            | ForgeVerb::Sandbox { .. }
            | ForgeVerb::Test { .. }
            | ForgeVerb::Rule { .. }
            | ForgeVerb::Render { .. }
            | ForgeVerb::RenderCs { .. }
            | ForgeVerb::Audit { .. }
            | ForgeVerb::AuditCs { .. } => unreachable!("handled above"),
            ForgeVerb::Load { path_or_url, source } => {
                cmd_load(&brain_path, &cfg, &path_or_url, source.as_deref(), json).await
            }
            ForgeVerb::List { filter } => cmd_list(&brain_path, filter.as_deref(), json),
            ForgeVerb::Show { slug } => cmd_show(&brain_path, &slug, json),
            ForgeVerb::Status { story } => cmd_status(&brain_path, story.as_deref(), json),
            ForgeVerb::Run { all, ids, filter, force, yes, halt_after } => {
                cmd_run(
                    &project_root, &brain_path, &cfg, all, ids.as_deref(),
                    filter.as_deref(), force, yes, halt_after, json,
                )
                .await
            }
            ForgeVerb::Reset { slug, yes } => cmd_reset(&project_root, &brain_path, &slug, yes, json),
        }
    }

    // â”€â”€â”€ proc framework: render + audit â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    //
    // Additive â€” does not modify any other forge subcommand. Reads the
    // framework data layer at `<framework>/` (standards/, profiles/,
    // bundles/) and either renders a proc skeleton or audits deployed
    // procs against framework-rendered expectations.

    /// Normalise to CRLF for SSDT/VS compatibility. Input is the renderer's
    /// internal LF text; we want every line terminator on disk to be `\r\n`
    /// so a freshly-rendered file diff-matches a deployed reference file
    /// byte-for-byte. Idempotent: if a `\r` is already present before `\n`,
    /// we don't double it. UTF-8 safe â€” operates on str lines, not bytes.
    fn to_crlf(s: &str) -> String {
        // split_terminator preserves whether a trailing newline existed; we
        // re-emit each line followed by \r\n. If the input had no trailing
        // newline we drop the one we'd add for the last segment.
        let trailing_newline = s.ends_with('\n');
        let mut out = String::with_capacity(s.len() + s.len() / 40);
        let mut first = true;
        for line in s.split_terminator('\n') {
            if !first {
                out.push_str("\r\n");
            }
            first = false;
            // Strip any \r that was already trailing the line so we don't
            // emit \r\r\n on already-CRLF input.
            out.push_str(line.strip_suffix('\r').unwrap_or(line));
        }
        if trailing_newline {
            out.push_str("\r\n");
        }
        out
    }

    pub fn cmd_proc_framework_render(
        framework: &Path,
        profile: &str,
        bundle: Option<&str>,
        id: Option<&str>,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::proc_framework::{
            manifest::{collect_endpoints, load_bundle, load_profile},
            render::render_endpoint,
        };

        // Resolve workspace + profile config. Generated and reference path
        // templates live in `profile.toml::generated_paths` / `reference_paths`.
        // They're workspace-relative; `framework` is e.g.
        // `<workspace>/dtcard/.forge/proc-framework`, so two parents up is
        // the workspace root.
        let workspace = framework
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or_else(|| format!("cannot derive workspace from framework={}", framework.display()))?
            .to_path_buf();
        let prof = load_profile(framework, profile)?;
        let gen_template = prof
            .generated_paths
            .sql_proc_root
            .as_deref()
            .ok_or_else(|| "profile.toml: generated_paths.sql_proc_root not set".to_string())?;
        let ref_template = prof.reference_paths.sql_proc_root.as_deref();

        // Collect endpoints: either from a specific bundle or from the
        // profile's combined manifest+bundles view.
        let mut endpoints = if let Some(bname) = bundle {
            let b = load_bundle(framework, bname)?;
            b.endpoints
                .into_iter()
                .filter(|ep| ep.is_thick_sp_in(profile))
                .collect::<Vec<_>>()
        } else {
            collect_endpoints(framework, profile)?
        };

        if let Some(target_id) = id {
            endpoints.retain(|ep| ep.id == target_id);
            if endpoints.is_empty() {
                return Err(format!("no endpoint with id={:?}", target_id));
            }
        }

        if endpoints.is_empty() {
            if json {
                println!("{{\"rendered\":[]}}");
            } else {
                println!("no endpoints to render");
            }
            return Ok(());
        }

        let mut rendered = Vec::new();
        for ep in &endpoints {
            let schema = ep.schema.clone().unwrap_or_default();

            // Resolve the legacy reference path (for Ignore-region preservation).
            let deployed_path: Option<PathBuf> = ref_template.map(|tpl| {
                let resolved = tpl.replace("{schema}", &schema);
                workspace.join(resolved).join(format!("{}.sql", ep.proc_name()))
            });

            let text = render_endpoint(framework, profile, ep, deployed_path.as_deref())?;

            // SSDT/VS convention is CRLF. Source fragments are LF for
            // platform-neutral authoring; convert at write-time so the
            // generated tree git-diffs cleanly against the reference tree.
            let text_crlf = to_crlf(&text);

            // Resolve the generated output path.
            let target_dir = workspace.join(gen_template.replace("{schema}", &schema));
            let target = target_dir.join(format!("{}.sql", ep.proc_name()));
            std::fs::create_dir_all(&target_dir)
                .map_err(|e| format!("mkdir {}: {}", target_dir.display(), e))?;
            std::fs::write(&target, text_crlf.as_bytes())
                .map_err(|e| format!("write {}: {}", target.display(), e))?;

            rendered.push((ep.id.clone(), target));
        }

        if json {
            let entries: Vec<_> = rendered
                .iter()
                .map(|(id, p)| serde_json::json!({"id": id, "path": p.display().to_string()}))
                .collect();
            println!("{}", serde_json::json!({"rendered": entries}));
        } else {
            for (id, p) in &rendered {
                println!("[{}] -> {}", id, p.display());
            }
        }

        Ok(())
    }

    pub fn cmd_proc_framework_render_cs(
        framework: &Path,
        profile: &str,
        bundle: Option<&str>,
        id: Option<&str>,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::proc_framework::{
            cs_render::render_endpoint_cs,
            manifest::{collect_endpoints, load_bundle, load_profile},
        };

        // Mirror cmd_proc_framework_render path-resolution. `framework`
        // is e.g. `<workspace>/dtcard/.forge/proc-framework` â€” three
        // parents up is the workspace root the profile paths resolve
        // against.
        let workspace = framework
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or_else(|| format!("cannot derive workspace from framework={}", framework.display()))?
            .to_path_buf();
        let prof = load_profile(framework, profile)?;
        let gen_template = prof
            .generated_paths
            .cs_query_root
            .as_deref()
            .ok_or_else(|| "profile.toml: generated_paths.cs_query_root not set".to_string())?;
        let ref_template = prof.reference_paths.cs_query_root.as_deref();

        // Collect endpoints â€” same logic as the SQL renderer.
        let mut endpoints = if let Some(bname) = bundle {
            let b = load_bundle(framework, bname)?;
            b.endpoints
                .into_iter()
                .filter(|ep| ep.is_thick_sp_in(profile))
                .collect::<Vec<_>>()
        } else {
            collect_endpoints(framework, profile)?
        };

        if let Some(target_id) = id {
            endpoints.retain(|ep| ep.id == target_id);
            if endpoints.is_empty() {
                return Err(format!("no endpoint with id={:?}", target_id));
            }
        }

        if endpoints.is_empty() {
            if json {
                println!("{{\"rendered\":[]}}");
            } else {
                println!("no endpoints to render");
            }
            return Ok(());
        }

        let mut rendered = Vec::new();
        for ep in &endpoints {
            let bundle_folder = ep.cs_bundle_folder();
            let file_name = format!("{}Query.cs", ep.cs_class_stem());

            // Resolve the legacy reference path (for Ignore-region preservation).
            let deployed_path: Option<PathBuf> = ref_template.map(|tpl| {
                let root = workspace.join(tpl);
                if bundle_folder.is_empty() {
                    root.join(&file_name)
                } else {
                    root.join(&bundle_folder).join(&file_name)
                }
            });

            let text = render_endpoint_cs(framework, profile, ep, deployed_path.as_deref())?;

            // Deployed dt .cs convention is LF without trailing newline
            // (verified by `file` on dt/TXN/.../Account/*.cs â€” 4 of 5
            // are "ASCII text", LF). SSDT .sql is CRLF; .cs is LF.
            // Strip any trailing newline so the file ends mid-line like
            // the hand-authored originals.
            let text = text.trim_end_matches(|c| c == '\n' || c == '\r').to_string();

            // Resolve output path. The .cs is placed under
            // `<cs_query_root>/<bundle>/<ClassName>Query.cs` (nested
            // layout â€” the dt convention for newer Query classes).
            let target_dir = if bundle_folder.is_empty() {
                workspace.join(gen_template)
            } else {
                workspace.join(gen_template).join(&bundle_folder)
            };
            let target = target_dir.join(&file_name);
            std::fs::create_dir_all(&target_dir)
                .map_err(|e| format!("mkdir {}: {}", target_dir.display(), e))?;
            std::fs::write(&target, text.as_bytes())
                .map_err(|e| format!("write {}: {}", target.display(), e))?;

            rendered.push((ep.id.clone(), target));
        }

        if json {
            let entries: Vec<_> = rendered
                .iter()
                .map(|(id, p)| serde_json::json!({"id": id, "path": p.display().to_string()}))
                .collect();
            println!("{}", serde_json::json!({"rendered": entries}));
        } else {
            for (id, p) in &rendered {
                println!("[{}] -> {}", id, p.display());
            }
        }

        Ok(())
    }

    pub fn cmd_proc_framework_audit(
        framework: &Path,
        profile: &str,
        bundle: Option<&str>,
        id: Option<&str>,
        deployed_root: &Path,
        verbose: bool,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::proc_framework::{
            audit::{audit_endpoint, AuditSummary},
            manifest::{collect_endpoints, load_bundle},
        };

        let mut endpoints = if let Some(bname) = bundle {
            let b = load_bundle(framework, bname)?;
            b.endpoints
                .into_iter()
                .filter(|ep| ep.is_thick_sp_in(profile))
                .collect::<Vec<_>>()
        } else {
            collect_endpoints(framework, profile)?
        };

        if let Some(target_id) = id {
            endpoints.retain(|ep| ep.id == target_id);
            if endpoints.is_empty() {
                return Err(format!("no endpoint with id={:?}", target_id));
            }
        }

        let mut results = Vec::with_capacity(endpoints.len());
        for ep in &endpoints {
            results.push(audit_endpoint(framework, profile, ep, deployed_root)?);
        }

        let summary = AuditSummary::from_results(&results);

        if json {
            let rows: Vec<_> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.row_id,
                        "deployed_exists": r.deployed_exists,
                        "is_legacy": r.is_legacy,
                        "compliant": r.compliant(),
                        "fully_ok": r.fully_ok,
                        "fully_drift": r.fully_drift,
                        "ignore_present": r.ignore_present,
                        "ignore_missing": r.ignore_missing,
                        "missing_blocks": r.missing_blocks,
                        "extra_blocks": r.extra_blocks,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "profile": profile,
                    "summary": {
                        "total": summary.total,
                        "framework_managed": summary.framework_managed,
                        "framework_compliant": summary.framework_compliant,
                        "framework_defects": summary.framework_defects,
                        "legacy": summary.legacy,
                        "missing_files": summary.missing_files,
                        "real_defects": summary.real_defects(),
                    },
                    "results": rows,
                })
            );
        } else {
            println!();
            println!("=== TXN Proc Framework Audit ({}) ===", profile);
            println!("endpoints walked:     {}", summary.total);
            println!(
                "framework-managed:    {} ({} compliant / {} drift)",
                summary.framework_managed,
                summary.framework_compliant,
                summary.framework_managed - summary.framework_compliant,
            );
            println!(
                "legacy (no markers):  {}  [informational - touch-it-migrate-it]",
                summary.legacy
            );
            println!("defects (real):       {}", summary.real_defects());
            println!();

            for r in &results {
                if r.compliant() {
                    if verbose {
                        if r.is_legacy {
                            println!("  --   {}  legacy-not-framework-managed", r.row_id);
                        } else {
                            println!(
                                "  OK   {}  ({} Fully / {} Ignore)",
                                r.row_id, r.fully_ok, r.ignore_present
                            );
                        }
                    }
                    continue;
                }
                println!("  XX   {}", r.row_id);
                if !r.deployed_exists {
                    println!("         deployed file not found");
                    continue;
                }
                for n in &r.fully_drift {
                    println!("         Fully drift: {}", n);
                }
                for n in &r.missing_blocks {
                    println!("         missing:     {}", n);
                }
                for n in &r.ignore_missing {
                    println!("         Ignore missing markers: {}", n);
                }
                if verbose && !r.extra_blocks.is_empty() {
                    println!(
                        "         extra blocks (informational): {}",
                        r.extra_blocks.join(", ")
                    );
                }
            }
        }

        if summary.real_defects() > 0 {
            std::process::exit(1);
        }
        Ok(())
    }

    pub fn cmd_proc_framework_audit_cs(
        framework: &Path,
        profile: &str,
        bundle: Option<&str>,
        id: Option<&str>,
        cs_query_root: &Path,
        verbose: bool,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::proc_framework::{
            cs_audit::{audit_cs_endpoint, CsAuditSummary},
            manifest::{collect_endpoints, load_bundle},
        };

        let mut endpoints = if let Some(bname) = bundle {
            let b = load_bundle(framework, bname)?;
            b.endpoints
                .into_iter()
                .filter(|ep| ep.is_thick_sp_in(profile))
                .collect::<Vec<_>>()
        } else {
            collect_endpoints(framework, profile)?
        };

        if let Some(target_id) = id {
            endpoints.retain(|ep| ep.id == target_id);
            if endpoints.is_empty() {
                return Err(format!("no endpoint with id={:?}", target_id));
            }
        }

        let mut results = Vec::with_capacity(endpoints.len());
        for ep in &endpoints {
            results.push(audit_cs_endpoint(cs_query_root, ep)?);
        }

        let summary = CsAuditSummary::from_results(&results);

        if json {
            let rows: Vec<_> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.row_id,
                        "query_path": r.query_path.display().to_string(),
                        "deployed_exists": r.deployed_exists,
                        "is_legacy": r.is_legacy,
                        "compliant": r.compliant(),
                        "fully_ok": r.fully_ok,
                        "fully_drift": r.fully_drift,
                        "ignore_present": r.ignore_present,
                        "deployed_region_names": r.deployed_region_names,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "profile": profile,
                    "summary": {
                        "total": summary.total,
                        "framework_managed": summary.framework_managed,
                        "framework_compliant": summary.framework_compliant,
                        "framework_defects": summary.framework_defects,
                        "legacy": summary.legacy,
                        "missing_files": summary.missing_files,
                        "real_defects": summary.real_defects(),
                    },
                    "results": rows,
                })
            );
        } else {
            println!();
            println!("=== TXN C# Query Audit ({}) ===", profile);
            println!("endpoints walked:     {}", summary.total);
            println!(
                "framework-managed:    {} ({} compliant / {} drift)",
                summary.framework_managed,
                summary.framework_compliant,
                summary.framework_managed - summary.framework_compliant,
            );
            println!(
                "legacy (no markers):  {}  [informational - touch-it-migrate-it]",
                summary.legacy
            );
            println!("missing files:        {}", summary.missing_files);
            println!("defects (real):       {}", summary.real_defects());
            println!();

            for r in &results {
                if r.compliant() {
                    if verbose {
                        if r.is_legacy {
                            println!(
                                "  --   {}  legacy-not-framework-managed  ({})",
                                r.row_id,
                                r.query_path.display()
                            );
                        } else {
                            println!(
                                "  OK   {}  ({} Fully / {} Ignore)",
                                r.row_id, r.fully_ok, r.ignore_present
                            );
                        }
                    }
                    continue;
                }
                println!("  XX   {}", r.row_id);
                if !r.deployed_exists {
                    println!("         deployed file not found: {}", r.query_path.display());
                    continue;
                }
                for n in &r.fully_drift {
                    println!("         Fully drift: {}", n);
                }
                if verbose && !r.extra_blocks.is_empty() {
                    println!(
                        "         extra blocks (informational): {}",
                        r.extra_blocks.join(", ")
                    );
                }
            }
        }

        if summary.real_defects() > 0 {
            std::process::exit(1);
        }
        Ok(())
    }

    fn cmd_init(
        project: &str,
        target: Option<&Path>,
        merge: bool,
        force: bool,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::init::{apply_scaffold, ApplyMode};
        let root = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(project));
        let mode = if force {
            ApplyMode::Force
        } else if merge {
            ApplyMode::Merge
        } else {
            ApplyMode::Strict
        };
        let result = apply_scaffold(project, &root, mode)
            .map_err(|e| format!("forge init: {}", e))?;
        let root_str = normalize_display_path(&root);
        let said_str = normalize_display_path(&result.said_path);
        if json {
            let out = serde_json::json!({
                "ok": true,
                "project": project,
                "root": root_str,
                "said_path": said_str,
                "folders_created": result.folders_created,
                "files_created": result.files_created,
                "files_skipped": result.files_skipped,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        } else {
            let sep = platform_sep();
            println!("âœ“ Initialised forge workspace at {}", root_str);
            println!(
                "  {} folders, {} files created, {} skipped.",
                result.folders_created, result.files_created, result.files_skipped
            );
            println!("  Brain: {}", said_str);
            println!();
            println!("Next steps:");
            println!("  1. Drop SQL/code       â†’ {root}{sep}1-ground-truth{sep}", root = root_str, sep = sep);
            println!("  2. Drop existing code  â†’ {root}{sep}2-progress{sep}", root = root_str, sep = sep);
            println!("  3. Drop client PDFs    â†’ {root}{sep}3-requirements{sep}", root = root_str, sep = sep);
            println!("  4. Drop OpenAPI + MDs  â†’ {root}{sep}4-expectations{sep}", root = root_str, sep = sep);
            println!("  5. Run                 â†’ said forge plan");
        }
        Ok(())
    }

    fn cmd_sync(
        target: Option<&Path>,
        dry_run: bool,
        force: bool,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::sync::{build_sync_plan, execute_sync_with_cfg};
        use said_forge::workspace_config::WorkspaceConfig;
        let root = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Load the workspace config, enforce plan_complete gate.
        let cfg_path = WorkspaceConfig::default_path_for(&root);
        if !cfg_path.exists() {
            return Err(format!(
                "no .forge/config.toml at {} â€” run `said forge plan` first",
                cfg_path.display()
            ));
        }
        let cfg = WorkspaceConfig::load(&cfg_path)
            .map_err(|e| format!("load config: {}", e))?;
        if !cfg.plan_complete {
            return Err(format!(
                "config at {} has plan_complete=false â€” run `said forge plan` first",
                cfg_path.display()
            ));
        }

        // Resolve the .said file. Convention: `<project>/<project-name>.said`.
        // We accept any `*.said` in the root dir when there's exactly one.
        let said_path = find_workspace_said(&root)
            .map_err(|e| format!("locate .said: {}", e))?;

        // Build the plan.
        let mut plan = build_sync_plan(&root, &cfg)
            .map_err(|e| format!("build plan: {}", e))?;

        // Load manifest + filter unchanged (unless --force).
        if !force && said_path.exists() {
            let mut said = SaidFile::open(&said_path)?;
            let project_name = said_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("forge");
            if let Some(mf) = said_forge::sync::load_manifest_public(&mut said, project_name) {
                mf.filter_unchanged(&mut plan);
            }
        }

        if dry_run {
            if json {
                let out = serde_json::json!({
                    "ok": true,
                    "dry_run": true,
                    "to_ingest": plan.entries.len(),
                    "skipped_unchanged": plan.skipped_unchanged.len(),
                    "orphans": plan.orphans.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    "entries": plan.entries.iter().map(|e| serde_json::json!({
                        "path": e.rel_path.display().to_string(),
                        "kind": format!("{:?}", e.kind),
                        "authority": e.authority.to_tag(),
                        "size": e.size,
                    })).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
            } else {
                println!("Dry run â€” {} file(s) to ingest, {} unchanged (skip), {} orphan(s) at root",
                         plan.entries.len(), plan.skipped_unchanged.len(), plan.orphans.len());
                for e in &plan.entries {
                    println!("  [{:?}] {} â†’ {}", e.kind, e.rel_path.display(), e.authority.to_tag());
                }
                if !plan.orphans.is_empty() {
                    println!("Orphan files at project root (won't be ingested):");
                    for p in &plan.orphans {
                        println!("  {}", p.display());
                    }
                }
            }
            return Ok(());
        }

        let mut said = if said_path.exists() {
            SaidFile::open(&said_path)?
        } else {
            SaidFile::create(&said_path)
        };
        let project_name = said_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("forge")
            .to_string();
        let result = execute_sync_with_cfg(&plan, &mut said, &project_name, Some(&cfg))
            .map_err(|e| format!("execute sync: {}", e))?;
        said.save().map_err(|e| format!("save .said: {}", e))?;

        if json {
            let out = serde_json::json!({
                "ok": true,
                "said": normalize_display_path(&said_path),
                "frames_written": result.frames_written,
                "bytes_ingested": result.bytes_ingested,
                "skipped_unchanged": result.files_skipped_unchanged,
                "skipped_xlsx_unsupported": result.files_skipped_xlsx_unsupported,
                "by_kind": result.by_kind,
                "by_authority": result.by_authority,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        } else {
            println!("âœ“ Sync complete â€” {} frames written ({} bytes), {} unchanged",
                     result.frames_written, result.bytes_ingested, result.files_skipped_unchanged);
            if result.files_skipped_xlsx_unsupported > 0 {
                println!("  âš  {} XLSX file(s) skipped (enable --features forge-xlsx in phase 15)",
                         result.files_skipped_xlsx_unsupported);
            }
            if !result.by_authority.is_empty() {
                print!("  by authority: ");
                let parts: Vec<String> = result
                    .by_authority
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                println!("{}", parts.join(", "));
            }
        }
        Ok(())
    }

    /// Find the workspace `.said` file. Accepts:
    /// - a single `*.said` sitting in `root`
    /// - `<dirname>.said` if the folder name matches
    fn find_workspace_said(root: &Path) -> Result<PathBuf, String> {
        // Prefer a `.said` whose stem matches the folder name.
        if let Some(folder_name) = root.file_name().and_then(|n| n.to_str()) {
            let candidate = root.join(format!("{}.said", folder_name));
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        let mut found = Vec::new();
        for entry in std::fs::read_dir(root).map_err(|e| format!("read {}: {}", root.display(), e))? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.extension().and_then(|e| e.to_str()) == Some("said") {
                found.push(p);
            }
        }
        match found.len() {
            0 => Err(format!("no .said file at {}", root.display())),
            1 => Ok(found.into_iter().next().unwrap()),
            _ => Err(format!(
                "multiple .said files at {} â€” pass --path to disambiguate",
                root.display()
            )),
        }
    }

    /// Walk upward from `starting_path` until we find a folder whose
    /// immediate children are group folders (each containing MD files).
    /// Returns that folder, or None if the heuristic can't settle.
    fn resolve_dev_planning_root(starting_path: &Path) -> Option<PathBuf> {
        let mut current = if starting_path.is_dir() {
            starting_path.to_path_buf()
        } else {
            starting_path.parent()?.to_path_buf()
        };
        // Walk up to 5 levels looking for a folder whose direct children
        // are mostly group-folders (each with MD files inside).
        for _ in 0..5 {
            if looks_like_dev_planning_root(&current) {
                return Some(current);
            }
            let parent = match current.parent() {
                Some(p) => p.to_path_buf(),
                None => break,
            };
            if parent == current {
                break;
            }
            current = parent;
        }
        None
    }

    fn looks_like_dev_planning_root(dir: &Path) -> bool {
        if !dir.is_dir() {
            return false;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return false,
        };
        let mut group_candidates = 0;
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            // Does this subfolder contain at least one .md file that isn't api.md?
            if let Ok(subentries) = std::fs::read_dir(&p) {
                let has_md = subentries.flatten().any(|e| {
                    let p = e.path();
                    p.is_file()
                        && p.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase)
                            == Some("md".into())
                        && p.file_name().and_then(|n| n.to_str()) != Some("api.md")
                });
                if has_md {
                    group_candidates += 1;
                }
            }
        }
        group_candidates >= 2
    }

    async fn cmd_gaps(
        target: Option<&Path>,
        out: Option<&Path>,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::gap_structured::{
            generate_structured_gaps, read_xlsx_rows_from_brain, StructuredGapsInput,
        };
        use said_forge::workspace_config::{DirectiveMode, WorkspaceConfig};

        let root = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let cfg_path = WorkspaceConfig::default_path_for(&root);
        if !cfg_path.exists() {
            return Err(format!(
                "no .forge/config.toml at {} â€” run `said forge plan` first",
                cfg_path.display()
            ));
        }
        let cfg = WorkspaceConfig::load(&cfg_path)
            .map_err(|e| format!("load config: {}", e))?;
        if !cfg.plan_complete {
            return Err("config.toml has plan_complete=false â€” run `said forge plan` first".into());
        }
        if cfg.directive.mode == DirectiveMode::Unset {
            return Err("no directive configured in .forge/config.toml â€” re-run `said forge plan`".into());
        }

        // Dev Planning root â€” we walk this folder tree looking for
        // per-op MDs. Config.toml primary often points at a specific file
        // inside a group folder (e.g. `.../Cardholders/api.md`). We walk
        // upward until we find a folder that contains MULTIPLE subdirs
        // each with MD files (the "groups of groups" signal). Falls back
        // to the default 4-expectations/Dev Planning layout.
        let dev_planning_root: PathBuf = match cfg.directive.primary.as_deref() {
            Some(p) => resolve_dev_planning_root(&root.join(p))
                .unwrap_or_else(|| root.join("4-expectations/Dev Planning")),
            None => root.join("4-expectations/Dev Planning"),
        };

        // OpenAPI â€” if secondary is set AND Both mode is active, use it.
        // Otherwise fall back to scanning 4-expectations/ for an openapi yaml.
        let openapi_path: Option<PathBuf> = if cfg.directive.mode == DirectiveMode::Both {
            cfg.directive.secondary.as_ref().map(|p| root.join(p))
        } else {
            None
        };

        // Project name from the .said stem.
        let said_path = find_workspace_said(&root)?;
        let project = project_name_from(&said_path);

        // Load XLSM rows from the brain.
        let mut brain = SaidFile::open(&said_path)?;
        let xlsx_rows = read_xlsx_rows_from_brain(&mut brain);

        let out_dir = out.map(Path::to_path_buf).unwrap_or_else(|| root.join(".forge/gaps"));
        // Wipe any legacy monolithic gaps.md in the same parent so users
        // don't accidentally keep reading the old file.
        let legacy = out_dir.with_extension("md");
        if legacy.is_file() {
            let _ = std::fs::remove_file(&legacy);
        }

        let result = generate_structured_gaps(StructuredGapsInput {
            project: &project,
            workspace_root: &root,
            dev_planning_root: &dev_planning_root,
            openapi_path: openapi_path.as_deref(),
            xlsx_rows,
            out_dir: &out_dir,
        })
        .map_err(|e| format!("gap report: {}", e))?;

        if json {
            let payload = serde_json::json!({
                "ok": true,
                "out_dir": normalize_display_path(&out_dir),
                "groups_written": result.groups_written,
                "op_files_written": result.op_files_written,
                "orphan_openapi_ops": result.orphan_openapi_ops.len(),
                "xlsx_rows_loaded": result.total_xlsx_rows,
            });
            println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
        } else {
            println!("âœ“ Structured gap report written to {}", normalize_display_path(&out_dir));
            println!(
                "  {} group folders Â· {} per-op files Â· {} orphan OpenAPI ops Â· {} XLSM rows loaded",
                result.groups_written,
                result.op_files_written,
                result.orphan_openapi_ops.len(),
                result.total_xlsx_rows
            );
        }
        Ok(())
    }

    async fn cmd_viz(
        target: Option<&Path>,
        out: Option<&Path>,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::gap_structured::walk_dev_planning;
        use said_forge::sql_catalog::build_catalog;
        use said_forge::workspace_config::{DirectiveMode, WorkspaceConfig};
        use said_forge::{
            render_viz_document, AuthorityPaths, Glossary, MappingOverrides, MappingService,
        };

        let root = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let cfg_path = WorkspaceConfig::default_path_for(&root);
        if !cfg_path.exists() {
            return Err(format!(
                "no .forge/config.toml at {} â€” run `said forge plan` first",
                cfg_path.display()
            ));
        }
        let cfg = WorkspaceConfig::load(&cfg_path)
            .map_err(|e| format!("load config: {}", e))?;

        let catalog =
            build_catalog(&root).map_err(|e| format!("build catalog: {}", e))?;
        let glossary = Glossary::build(&catalog);
        let overrides_path = root.join(".forge").join("mapping.toml");
        let overrides = MappingOverrides::load(&overrides_path)
            .map_err(|e| format!("load mapping.toml: {}", e))?;
        let service = MappingService::new(&catalog, &glossary, &overrides);

        let ops: Vec<_> = if cfg.directive.mode == DirectiveMode::Unset {
            Vec::new()
        } else {
            let dev_planning_root: PathBuf = match cfg.directive.primary.as_deref() {
                Some(p) => resolve_dev_planning_root(&root.join(p))
                    .unwrap_or_else(|| root.join("4-expectations/Dev Planning")),
                None => root.join("4-expectations/Dev Planning"),
            };
            walk_dev_planning(&dev_planning_root)
                .map_err(|e| format!("walk dev planning: {}", e))?
                .into_values()
                .flat_map(|group| group.into_iter().map(|(_, op)| op))
                .collect()
        };

        let authority = AuthorityPaths {
            law: Some(normalize_display_path(&root.join("1-ground-truth"))),
            existing: None,
            agreed: None,
            requested: cfg
                .directive
                .primary
                .clone()
                .map(|p| normalize_display_path(Path::new(&p))),
            wishlist: cfg
                .directive
                .secondary
                .clone()
                .map(|p| normalize_display_path(Path::new(&p))),
        };

        let body = render_viz_document(&catalog, &service, &ops, &authority);
        let out_path = out
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.join(".forge").join("viz.md"));
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {}", parent.display(), e))?;
        }
        std::fs::write(&out_path, &body)
            .map_err(|e| format!("write {}: {}", out_path.display(), e))?;

        if json {
            let payload = serde_json::json!({
                "ok": true,
                "out": normalize_display_path(&out_path),
                "bytes": body.len(),
                "catalog_tables": catalog.tables.len(),
                "ops": ops.len(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
        } else {
            println!(
                "âœ“ Viz written to {} ({} bytes Â· {} tables Â· {} ops)",
                normalize_display_path(&out_path),
                body.len(),
                catalog.tables.len(),
                ops.len()
            );
        }
        Ok(())
    }

    /// Run the optional read-only sandbox verification pass and
    /// return any closed-set enum overrides for the OpenAPI emitter.
    /// Implemented in two arms so the binary builds with or without
    /// the `forge-sql-verify` feature.
    #[cfg(feature = "forge-sql-verify")]
    async fn run_sandbox_verification_if_requested(
        verify_against_sandbox: bool,
        catalog: &said_forge::sql_catalog::SqlCatalog,
        max_enum_rows: usize,
        client_hint: Option<&str>,
    ) -> Option<said_forge::sql_to_openapi::EnumOverrides> {
        if !verify_against_sandbox {
            return None;
        }
        let _ = max_enum_rows; // threaded into run_verification below
        use said_forge::sql_verify;
        let sandbox = match sql_verify::discover_sandbox_with_hint(client_hint) {
            Some(s) => s,
            None => {
                let extra = client_hint
                    .map(|c| format!(" (looked for one matching `{}`)", c))
                    .unwrap_or_default();
                eprintln!(
                    "  ! --verify-against-sandbox requested but no `said-sbx-*` container is running{}.",
                    extra
                );
                return None;
            }
        };
        eprintln!(
            "  â†’ verifying against sandbox `{}` on port {}",
            sandbox.container_name, sandbox.host_port
        );
        // Build the per-lookup-table column map. Forge already knows
        // these from the proc-body lookup-hint chain, but at this
        // point in cmd_docs we only have the SqlCatalog. Walk every
        // lookup-table TableSchema (from the catalog or the proc body
        // again) and pick the most likely (code_col, desc_col) pair
        // by Hungarian convention: `<prefix>_Code` + optional
        // `<prefix>_Desc`.
        let mut lookup_columns: std::collections::BTreeMap<
            String,
            (String, Option<String>),
        > = std::collections::BTreeMap::new();
        for t in &catalog.tables {
            let schema = t.schema.as_deref().unwrap_or("");
            if !schema.eq_ignore_ascii_case("lookups") {
                continue;
            }
            let prefix = t
                .name
                .splitn(2, '_')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let mut code_col = format!("{}_Code", prefix);
            let mut desc_col: Option<String> = None;
            // Pick whatever the table actually has; tolerate naming
            // drift (col_Country_Lookup â†’ col_Code; cps_*_Status â†’ cps_Status;
            // some tables use the bare prefix as the PK column).
            if !t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&code_col)) {
                if let Some(c) = t.columns.iter().find(|c| {
                    let lc = c.name.to_ascii_lowercase();
                    lc.ends_with("_code")
                        || lc.ends_with("_status")
                        || lc == "code"
                        || lc == "status"
                }) {
                    code_col = c.name.clone();
                }
            }
            if let Some(c) = t.columns.iter().find(|c| {
                let lc = c.name.to_ascii_lowercase();
                lc.ends_with("_desc") || lc.ends_with("_description") || lc == "description"
            }) {
                desc_col = Some(c.name.clone());
            }
            lookup_columns.insert(format!("lookups.{}", t.name), (code_col, desc_col));
        }
        match sql_verify::run_verification_with_cap(&sandbox, &lookup_columns, max_enum_rows).await {
            Ok(report) => {
                eprintln!(
                    "  â†’ sandbox: {} procs, {} closed-set lookups verified, {} open lookups skipped",
                    report.procs_verified,
                    report.closed_lookup_enums.len(),
                    report.skipped_open_lookups.len()
                );
                let mut overrides: said_forge::sql_to_openapi::EnumOverrides =
                    Default::default();
                for (table_full, values) in report.closed_lookup_enums {
                    overrides
                        .insert(table_full, values.into_iter().map(|v| v.code).collect());
                }
                Some(overrides)
            }
            Err(e) => {
                eprintln!("  ! sandbox verification failed: {}", e);
                None
            }
        }
    }

    #[cfg(not(feature = "forge-sql-verify"))]
    async fn run_sandbox_verification_if_requested(
        verify_against_sandbox: bool,
        _catalog: &said_forge::sql_catalog::SqlCatalog,
        _max_enum_rows: usize,
        _client_hint: Option<&str>,
    ) -> Option<said_forge::sql_to_openapi::EnumOverrides> {
        if verify_against_sandbox {
            eprintln!(
                "  ! --verify-against-sandbox requested but this build was \
                 compiled without the `forge-sql-verify` feature. Re-run \
                 `cargo build -p said-cli --features forge,forge-sql-verify`."
            );
        }
        None
    }

    /// Pipe a SQL file's contents into `sqlcmd` running inside the
    /// named sandbox container. Mirrors the seed-data loading pattern
    /// in handler.rs (stdin pipe instead of volume mount). Treats a
    /// missing input file as a no-op so callers don't have to guard.
    fn apply_sql_to_sandbox(
        container_name: &str,
        sql_file_host: &Path,
    ) -> Result<(), String> {
        if !sql_file_host.exists() {
            return Ok(()); // nothing to apply
        }
        let sql = std::fs::read_to_string(sql_file_host)
            .map_err(|e| format!("read {}: {}", sql_file_host.display(), e))?;
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("docker")
            .args([
                "exec", "-i", container_name,
                "/opt/mssql-tools18/bin/sqlcmd",
                "-S", "localhost", "-U", "sa", "-P", "Said_Test_2026!",
                "-C", "-b",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn sqlcmd: {e}"))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(sql.as_bytes())
                .map_err(|e| format!("write sql: {e}"))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("wait sqlcmd: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "sqlcmd failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }

    /// Run the spec fitter if a sandbox is reachable. Returns the
    /// corrected YAML string, OR the original on any failure (we
    /// never want a fit-pass error to lose the spec).
    #[cfg(feature = "forge-sql-verify")]
    /// Try registry-first generation: when the sandbox has a recognised
    /// API registry table, emit the spec directly from those rows
    /// (skipping the heuristic name-mapper) and bind procs via the
    /// 6-gate matcher. Returns `Ok(None)` when no registry is available
    /// â€” caller falls back to the catalog-walk generator.
    #[cfg(feature = "forge-sql-verify")]
    async fn build_registry_first_spec(
        title: &str,
        deliverables_root: &Path,
        client_hint: &str,
        catalog: &said_forge::sql_catalog::SqlCatalog,
        workspace_root: &Path,
    ) -> Result<Option<String>, String> {
        use said_forge::sql_verify::discover_sandbox_with_hint;
        let sandbox = match discover_sandbox_with_hint(Some(client_hint)) {
            Some(s) => s,
            None => return Ok(None),
        };
        // Parse Dev Spec markdown so the registry-first spec gets
        // fully-enriched operations + components/schemas. Missing
        // directory or parse failure â†’ degrade to the bare-bones
        // (Task 1) emission.
        let dev_planning = workspace_root.join("4-expectations").join("Dev Planning");
        let standard = said_forge::OpenApiStandard::load(workspace_root, Some(client_hint))
            .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
        let mut dev_spec_rewrites: Vec<said_forge::dev_spec::parser::PathRewriteLog> = Vec::new();
        let dev_eps_owned: Vec<said_forge::dev_spec::types::DevSpecEndpoint> =
            if dev_planning.exists() {
                said_forge::dev_spec::parser::walk_dev_spec_dir_logged(
                    &dev_planning, &standard, &mut dev_spec_rewrites,
                )
                .unwrap_or_default()
            } else {
                Vec::new()
            };
        let dev_eps_ref: Option<&[said_forge::dev_spec::types::DevSpecEndpoint]> =
            if dev_eps_owned.is_empty() {
                None
            } else {
                Some(&dev_eps_owned)
            };
        let resolutions = said_forge::fitter::AmbiguityResolutions::load(workspace_root);
        if resolutions.len() > 0 {
            eprintln!("  â†’ loaded {} ambiguity resolutions", resolutions.len());
        }
        let result = said_forge::fitter::build_spec_from_registry(
            &sandbox, title, catalog, dev_eps_ref, &standard, &resolutions,
        )
        .await?;
        let (yaml, mut report) = match result {
            Some(pair) => pair,
            None => return Ok(None),
        };
        // Promote Dev Spec path rewrites (pluralisation, etc.) into the
        // FitReport so `write_fixes` records them in fixes.md.
        for rw in dev_spec_rewrites {
            report.standard_normalisations.push(
                said_forge::fitter::StandardNormalisation {
                    from: rw.from,
                    to: rw.to,
                    reason: rw.reason,
                },
            );
        }
        let corrections_path = deliverables_root.join("corrections.md");
        said_forge::fitter::write_corrections(&report, client_hint, &corrections_path)?;
        let fixes_path = deliverables_root.join("fixes.md");
        said_forge::fitter::write_fixes(&report, client_hint, &fixes_path)?;
        let ambiguities_path = deliverables_root.join("ambiguities.md");
        said_forge::fitter::write_ambiguities(&report, client_hint, &ambiguities_path)?;

        // Four-signal coverage report: combines Dev Spec âˆª Registry
        // (signals 1+2), the proc-binding outcome already in `report`
        // (signal 4), and Bruno fixture presence across both collection
        // roots (signal 3) into a single per-endpoint Phase A / Phase B
        // classification. Drives the `said test` workflow + Phase B
        // worklist downstream.
        let registry = said_forge::fitter::fetch_registry(&sandbox)
            .await
            .unwrap_or_default();
        let bruno_roots = bruno_collection_roots(workspace_root, client_hint);
        let coverage_rows = said_forge::coverage::build_coverage(
            &dev_eps_owned, &registry, &report, &bruno_roots,
        );
        let coverage_path = deliverables_root.join("coverage.md");
        said_forge::coverage::write_report(
            &coverage_rows, client_hint, &coverage_path,
        )?;
        let testable = coverage_rows.iter().filter(|r| {
            r.status == said_forge::coverage::CoverageStatus::PhaseATestable
        }).count();
        let phase_b = coverage_rows.iter().filter(|r| matches!(
            r.status,
            said_forge::coverage::CoverageStatus::PhaseBGenerateApi
            | said_forge::coverage::CoverageStatus::PhaseBGenerateSp
            | said_forge::coverage::CoverageStatus::PhaseBAuthorDevSpec
        )).count();
        eprintln!(
            "  â†’ coverage: {} endpoints assessed ({} Phase A â€” testable, {} Phase B â€” needs generation)",
            coverage_rows.len(), testable, phase_b,
        );

        eprintln!(
            "  â†’ registry-first spec: {} endpoints from `{}` ({} bound, {} ambiguous, {} unbound)",
            report.registry_size,
            report.registry_source,
            report.missing_ops_added.len(),
            report
                .missing_ops_logged
                .iter()
                .filter(|m| m.reason.starts_with("AMBIGUOUS"))
                .count(),
            report
                .missing_ops_logged
                .iter()
                .filter(|m| !m.reason.starts_with("AMBIGUOUS"))
                .count(),
        );
        Ok(Some(yaml))
    }

    #[cfg(not(feature = "forge-sql-verify"))]
    async fn build_registry_first_spec(
        _title: &str,
        _deliverables_root: &Path,
        _client_hint: &str,
        _catalog: &said_forge::sql_catalog::SqlCatalog,
        _workspace_root: &Path,
    ) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Apply Dev Spec table definitions + registry amendments to the
    /// running sandbox. Runs before spec generation so the live DB
    /// has every entity Dev Spec demands. Idempotent (CREATE TABLE
    /// IF NOT EXISTS, MERGE for registry rows) â€” safe to re-run.
    ///
    /// Steps:
    ///   1. Walk Dev Spec markdown â†’ derive ERD
    ///   2. Borrow decisions vs SQL catalog
    ///   3. Emit tables.sql (CREATE TABLE + deferred FK ALTERs)
    ///   4. Emit registry-amendments.sql (MERGE per endpoint)
    ///   5. Apply both to the sandbox over the existing tiberius
    ///      connection
    #[cfg(feature = "forge-sql-verify")]
    async fn apply_dev_spec_to_sandbox(
        client_hint: &str,
        workspace_root: &Path,
        deliverables_root: &Path,
    ) -> Result<(), String> {
        use said_forge::sql_verify::{apply_sql_script, discover_sandbox_with_hint};

        let sandbox = match discover_sandbox_with_hint(Some(client_hint)) {
            Some(s) => s,
            None => return Err("no sandbox to apply Dev Spec against".into()),
        };

        // Generate (or regenerate) tables.sql + registry-amendments.sql
        // so the on-disk SQL matches the current Dev Spec.
        let dev_planning = workspace_root.join("4-expectations").join("Dev Planning");
        if !dev_planning.exists() {
            return Err(format!("no Dev Planning dir at {}", dev_planning.display()));
        }
        let standard = said_forge::OpenApiStandard::load(workspace_root, Some(client_hint))
            .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
        let endpoints = said_forge::dev_spec::parser::walk_dev_spec_dir(
            &dev_planning, &standard,
        )?;
        let erd = said_forge::dev_spec::erd::derive_erd(&endpoints);
        let catalog = said_forge::sql_catalog::build_catalog(workspace_root)
            .map_err(|e| format!("build catalog: {}", e))?;
        let decisions = said_forge::dev_spec::borrow::decide_borrows(&erd, &catalog);

        let sql_dir = deliverables_root.join("sql");
        std::fs::create_dir_all(&sql_dir).map_err(|e| format!("create sql dir: {}", e))?;
        let tables_sql_path = sql_dir.join("tables.sql");
        let amend_sql_path = sql_dir.join("registry-amendments.sql");

        let tables_sql = said_forge::dev_spec::sql_emit::emit_all_tables(&erd, &decisions);
        std::fs::write(&tables_sql_path, &tables_sql)
            .map_err(|e| format!("write tables.sql: {}", e))?;
        let amend_sql = said_forge::dev_spec::registry_amend::emit_registry_merge(&erd);
        std::fs::write(&amend_sql_path, &amend_sql)
            .map_err(|e| format!("write registry-amendments.sql: {}", e))?;

        // Apply tables first (procs/registry can FK to them).
        eprintln!("  â†’ applying Dev Spec tables.sql to sandbox `{}`",
            sandbox.container_name);
        if let Err(e) = apply_sql_script(sandbox.host_port, &tables_sql).await {
            // Soft warning â€” many CREATE TABLE statements use IF NOT
            // EXISTS guards, so some failures are expected on re-runs.
            eprintln!("  âš  tables.sql apply: {}", e);
        }

        eprintln!("  â†’ applying Dev Spec registry-amendments.sql to sandbox");
        if let Err(e) = apply_sql_script(sandbox.host_port, &amend_sql).await {
            eprintln!("  âš  registry-amendments.sql apply: {}", e);
        }

        // Sandbox-bootstrap pass â€” applies `dtcard/.forge/sandbox-bootstrap.sql`
        // (seed data: gsv defaults, lookup-table fills, etc.) and
        // `dtcard/.forge/proc-patches.sql` (temporary proc patches).
        //
        // Both files are operator-editable. Adding a new seed row or
        // proc patch does NOT require rebuilding said-cli â€” engineers
        // edit the file and re-run `forge docs --verify-against-sandbox`.
        // See the file headers for conventions and "when to delete"
        // notes per entry.
        let forge_dir = workspace_root.join(".forge");
        let bootstrap_path = forge_dir.join("sandbox-bootstrap.sql");
        if bootstrap_path.exists() {
            eprintln!("  â†’ applying sandbox bootstrap from {}", bootstrap_path.display());
            let sql = std::fs::read_to_string(&bootstrap_path)
                .map_err(|e| format!("read {}: {}", bootstrap_path.display(), e))?;
            if let Err(e) = apply_sql_script(sandbox.host_port, &sql).await {
                eprintln!("  âš  sandbox bootstrap apply: {}", e);
            }
        }
        // proc-patches.sql is applied AFTER the ground-truth API procs
        // (further down) so its idempotent ALTER patches survive the
        // canonical re-deploy. Applying here would let ground-truth
        // overwrite the patched bodies and leave the sandbox running
        // the buggy upstream version. See "ground-truth API procs"
        // section below for the actual application.
        // Registry cleanup â€” UPDATE/DELETE rows in
        // `lookups.ars_Api_Rule_Settings` so it lines up with the
        // Dev Spec + `openapi-standard.toml` rules. Source-of-truth
        // for the contents is `dtcard/5-deliverables/<CLIENT>/dev-spec-vs-openapi-drift.md`
        // (re-derive via `dtcard/.forge/check_spec_alignment.py`).
        // Runs AFTER bootstrap so seed rows exist before we try to
        // align them, and AFTER proc-patches so the validator etc.
        // are correct first.
        let cleanup_path = forge_dir.join("registry-cleanup.sql");
        if cleanup_path.exists() {
            eprintln!("  â†’ applying registry cleanup from {}", cleanup_path.display());
            let sql = std::fs::read_to_string(&cleanup_path)
                .map_err(|e| format!("read {}: {}", cleanup_path.display(), e))?;
            if let Err(e) = apply_sql_script(sandbox.host_port, &sql).await {
                eprintln!("  âš  registry cleanup apply: {}", e);
            }
        }

        // Ground-truth API procs â€” deploy any `p_txn_API_*.sql` files
        // under `1-ground-truth/<Client>/` so the procs we hand-write
        // for Phase B (Fee, Webhook, â€¦) land in the running sandbox
        // without `docker cp` workarounds. The naming convention
        // (`p_txn_API_*`) is what every new API proc follows; the
        // sandbox snapshot at `.said-code/<module>/sandbox/schema.sql`
        // is regenerated by `forge sync` + `said sandbox` and stays
        // the authoritative deploy target for fresh containers â€” but
        // for the running container we want changes to land on every
        // `forge docs --verify-against-sandbox`.
        //
        // Drop CREATE PROCEDURE â†’ CREATE OR ALTER PROCEDURE on the
        // way in so re-runs don't fail with "procedure already exists".
        let ground_truth_root = workspace_root.join("1-ground-truth").join(client_hint);
        if ground_truth_root.exists() {
            let mut api_proc_files: Vec<std::path::PathBuf> = Vec::new();
            collect_api_proc_files(&ground_truth_root, &mut api_proc_files);
            if !api_proc_files.is_empty() {
                eprintln!(
                    "  â†’ applying {} ground-truth API procs from 1-ground-truth/{}/",
                    api_proc_files.len(), client_hint,
                );
                for path in &api_proc_files {
                    let sql = match std::fs::read_to_string(path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("    âš  read {}: {}", path.display(), e);
                            continue;
                        }
                    };
                    // Strip a UTF-8 BOM if present â€” the on-disk files
                    // were authored by various tools (Visual Studio,
                    // VS Code, hand-edits) and some carry the BOM.
                    // SQL Server's parser raises "Incorrect syntax
                    // near ''" when it encounters one.
                    let sql = sql.trim_start_matches('\u{FEFF}');
                    // Convert CREATE PROCEDURE â†’ CREATE OR ALTER
                    // PROCEDURE so re-runs are idempotent. Hand-written
                    // procs sometimes use multi-space variants like
                    // "CREATE   PROCEDURE", so do a lower-case word-
                    // boundary search rather than a fixed string match.
                    let sql = rewrite_create_procedure(sql);
                    if let Err(e) = apply_sql_script(sandbox.host_port, &sql).await {
                        eprintln!(
                            "    âš  apply {}: {}",
                            path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                            e,
                        );
                    }
                }
            }
        }

        // proc-patches.sql â€” apply NOW, after ground-truth re-deploy,
        // so the idempotent ALTER patches outlive the canonical body.
        // Operator-editable; see file headers for "why" + "when to delete".
        let patches_path = forge_dir.join("proc-patches.sql");
        if patches_path.exists() {
            eprintln!("  â†’ applying proc patches from {}", patches_path.display());
            let sql = std::fs::read_to_string(&patches_path)
                .map_err(|e| format!("read {}: {}", patches_path.display(), e))?;
            if let Err(e) = apply_sql_script(sandbox.host_port, &sql).await {
                eprintln!("  âš  proc patches apply: {}", e);
            }
        }

        // Validation-seed pass â€” ensures `lookups.arc_Api_Rule_Validations`
        // has a row for every (method, path, field) the Bruno fixtures
        // POST/PUT. Without this, every Create proc raises PrcCode 1001
        // ("Field configuration not found") on the first request after
        // a fresh sandbox container comes up. The file is generated by
        // `dtcard/.forge/seed_validation.py --client <CLIENT>`; we
        // re-apply whatever's currently on disk on every run.
        let validation_seed_path = deliverables_root
            .join("sql")
            .join("validation-seed.sql");
        if validation_seed_path.exists() {
            eprintln!(
                "  â†’ applying validation seed from {}",
                validation_seed_path.display(),
            );
            let sql = std::fs::read_to_string(&validation_seed_path)
                .map_err(|e| format!("read {}: {}", validation_seed_path.display(), e))?;
            if let Err(e) = apply_sql_script(sandbox.host_port, &sql).await {
                eprintln!("  âš  validation seed apply: {}", e);
            }
        }

        // OldData-seed pass â€” applied by `said test` after the
        // synthetic test-data-reset.sql wipes lookups (mbl / cbl /
        // fbl / crv / acn). The seed itself is idempotent (`IF NOT
        // EXISTS` guards in dtcard/.forge/load_olddata.py output)
        // so re-running across multiple harness invocations is safe.
        //
        // Not applied here in `forge docs` because the deploy path
        // doesn't run reset between calls â€” the seed lives next to
        // `said test` for the rich-fixture mode. To regenerate after
        // adding/changing a table whitelist entry, run:
        //   py dtcard/.forge/load_olddata.py --client <CLIENT>
        // and commit the regenerated seed under
        //   5-deliverables/<CLIENT>/sql/olddata-seed.sql
        let _ = deliverables_root.join("sql").join("olddata-seed.sql");

        Ok(())
    }

    /// Replace the first `CREATE PROCEDURE` (or any CREATEâ€¦PROCEDURE
    /// where the gap between tokens is whitespace only) with
    /// `CREATE OR ALTER PROCEDURE`. Idempotent â€” if the statement
    /// already says `CREATE OR ALTER PROCEDURE`, the input is returned
    /// unchanged. Case-insensitive on both keywords.
    #[cfg(feature = "forge-sql-verify")]
    fn rewrite_create_procedure(input: &str) -> String {
        let lower = input.to_ascii_lowercase();
        // Already idempotent? Return as-is.
        if lower.contains("create or alter procedure") {
            return input.to_string();
        }
        // Find the first "create" followed by whitespace and "procedure".
        let bytes = lower.as_bytes();
        let needle_create = b"create";
        let needle_proc = b"procedure";
        let mut i = 0;
        while i + needle_create.len() <= bytes.len() {
            if &bytes[i..i + needle_create.len()] == needle_create {
                let mut j = i + needle_create.len();
                while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                    j += 1;
                }
                if j + needle_proc.len() <= bytes.len()
                    && &bytes[j..j + needle_proc.len()] == needle_proc
                {
                    // Replace the [i .. j+needle_proc.len()] slice with
                    // "CREATE OR ALTER PROCEDURE" â€” preserving the
                    // remainder verbatim. Use byte indices on the
                    // original string (ASCII keywords, so byte == char).
                    let mut out = String::with_capacity(input.len() + 9);
                    out.push_str(&input[..i]);
                    out.push_str("CREATE OR ALTER PROCEDURE");
                    out.push_str(&input[j + needle_proc.len()..]);
                    return out;
                }
            }
            i += 1;
        }
        // No CREATE PROCEDURE found â€” leave input untouched (it might
        // be a TRIGGER or something else; let the SQL Server parser
        // surface the real error).
        input.to_string()
    }

    /// Walk `<root>` recursively, collect every file matching the API-proc
    /// naming convention `p_txn_API_*.sql` under any `Stored Procedures`
    /// directory. Used by `apply_dev_spec_to_sandbox` to push hand-written
    /// API procs into the running container without manual `docker cp`.
    #[cfg(feature = "forge-sql-verify")]
    fn collect_api_proc_files(root: &Path, out: &mut Vec<std::path::PathBuf>) {
        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_api_proc_files(&path, out);
            } else if path.is_file() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                // Accept both `p_txn_API_*` (current standard) and the
                // older `p_txn_Api_*` (mixed-case) naming. Some early
                // entities (Product, Business) were authored before the
                // convention was tightened to all-caps API and still ship
                // the older filename â€” without this they never reach the
                // sandbox on `forge docs --verify-against-sandbox`.
                let lname = name.to_ascii_lowercase();
                if lname.starts_with("p_txn_api_") && lname.ends_with(".sql") {
                    out.push(path);
                }
            }
        }
    }

    /// ERD drift loop: compare what's in the live database against
    /// what the Dev Spec + borrow decisions say should be there. Writes
    /// `erd-drift.md` (human report) + `sql/erd-drift-alter.sql`
    /// (additive ALTER statements) + updates `2-progress/<client>-erd.json`
    /// to reflect the live database. **No SQL executed against DB.**
    #[cfg(feature = "forge-sql-verify")]
    async fn run_erd_drift_check(
        deliverables_root: &Path,
        client_hint: &str,
        workspace_root: &Path,
        strict: bool,
    ) -> Result<(), String> {
        use said_forge::sql_verify::discover_sandbox_with_hint;
        use std::collections::BTreeSet;

        let sandbox = match discover_sandbox_with_hint(Some(client_hint)) {
            Some(s) => s,
            None => return Err("no sandbox to read live ERD from".into()),
        };

        // Re-derive the Expected ERD from Dev Spec + borrow decisions.
        let dev_planning = workspace_root.join("4-expectations").join("Dev Planning");
        if !dev_planning.exists() {
            return Err(format!(
                "no Dev Planning dir at {}", dev_planning.display()
            ));
        }
        let standard = said_forge::OpenApiStandard::load(workspace_root, Some(client_hint))
            .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
        let endpoints = said_forge::dev_spec::parser::walk_dev_spec_dir(
            &dev_planning, &standard,
        )?;
        let expected = said_forge::dev_spec::erd::derive_erd(&endpoints);
        let catalog = said_forge::sql_catalog::build_catalog(workspace_root)
            .map_err(|e| format!("build catalog: {}", e))?;
        let borrows = said_forge::dev_spec::borrow::decide_borrows(&expected, &catalog);

        // Build the entity-set scope: each Expected entity â†’
        // (schema, table) pair. Borrow decisions resolve target tables.
        let mut scope: BTreeSet<(String, String)> = BTreeSet::new();
        for (entity_name, _) in &expected.entities {
            let (schema, table) = match borrows.iter().find(|b| b.entity == *entity_name) {
                Some(b) => (b.schema.clone(), b.table_name.clone()),
                None => ("dbo".into(), entity_name.clone()),
            };
            scope.insert((schema.to_lowercase(), table.to_lowercase()));
        }

        // Two reads from the live DB:
        //
        //   1. **Scoped read** â€” Expected ERD entities + borrow targets.
        //      Used for drift comparison (we only care about contract
        //      divergence, not random catalog tables).
        //   2. **Unscoped read** â€” every table in every schema.
        //      Used for the final ERD render: the master TXN catalog
        //      (~256 tables) + the new Dev Spec tables (~74) = the
        //      complete database picture.
        let live_scoped = said_forge::dev_spec::drift::read_live_erd(&sandbox, &scope).await?;
        let live_full = said_forge::dev_spec::drift::read_live_erd(
            &sandbox, &BTreeSet::new()
        ).await?;

        // Build the expected (path, method) set for registry-row drift.
        let mut expected_paths: BTreeSet<(String, String)> = BTreeSet::new();
        for ep in &endpoints {
            expected_paths.insert((ep.path.clone(), ep.method.to_uppercase()));
        }

        // Compute drift against the SCOPED live read â€” drift only
        // concerns the Dev Spec contract.
        let report = said_forge::dev_spec::drift::compute_drift(
            &expected, &borrows, &live_scoped, &expected_paths, strict,
        );
        said_forge::dev_spec::drift::write_drift_artifacts(
            &report, client_hint, deliverables_root, strict,
        )?;

        // Write the **final ERD** (UNSCOPED live DB) to deliverables.
        // The **start ERD** at `2-progress/<client>-erd.json` is left
        // untouched so the Dev-Spec-derived contract stays diffable
        // against what's actually in the database.
        let live_erd = said_forge::dev_spec::drift::live_erd_to_erd_json(&live_full, &expected);
        let final_erd_json_path = deliverables_root.join("erd.json");
        let final_erd_md_path = deliverables_root.join("erd.md");
        std::fs::create_dir_all(deliverables_root)
            .map_err(|e| format!("create deliverables dir: {}", e))?;
        let json = said_forge::dev_spec::erd::to_canonical_json(&live_erd)?;
        std::fs::write(&final_erd_json_path, json)
            .map_err(|e| format!("write {}: {}", final_erd_json_path.display(), e))?;
        let md = said_forge::dev_spec::erd::render_mermaid(&live_erd);
        std::fs::write(&final_erd_md_path, md)
            .map_err(|e| format!("write {}: {}", final_erd_md_path.display(), e))?;

        // One-line operator hint.
        let mut tags = Vec::new();
        if !report.missing_tables.is_empty() {
            tags.push(format!("{} missing tables", report.missing_tables.len()));
        }
        if !report.missing_columns.is_empty() {
            tags.push(format!("{} missing cols", report.missing_columns.len()));
        }
        if !report.missing_fks.is_empty() {
            tags.push(format!("{} missing FKs", report.missing_fks.len()));
        }
        if !report.missing_registry_rows.is_empty() {
            tags.push(format!("{} missing registry rows",
                report.missing_registry_rows.len()));
        }
        let summary = if tags.is_empty() {
            "no drift".to_string()
        } else {
            tags.join(", ")
        };
        eprintln!(
            "  â†’ ERD drift: {} (in-scope live: {} tables, {} registry rows; report at erd-drift.md)",
            summary, report.live_table_count, report.live_registry_count,
        );
        eprintln!(
            "  â†’ final ERD: {} entities (full DB) â†’ erd.json + erd.md",
            live_full.tables.len(),
        );
        Ok(())
    }

    #[cfg(feature = "forge-sql-verify")]
    async fn run_fit_pass_if_possible(
        original_yaml: &str,
        deliverables_root: &Path,
        client_hint: &str,
        catalog: &said_forge::sql_catalog::SqlCatalog,
        workspace_root: &Path,
    ) -> Result<String, String> {
        use said_forge::sql_verify::discover_sandbox_with_hint;
        let sandbox = match discover_sandbox_with_hint(Some(client_hint)) {
            Some(s) => s,
            None => {
                // No sandbox = no truth source = no corrections.
                return Ok(original_yaml.to_string());
            }
        };
        let standard = said_forge::OpenApiStandard::load(workspace_root, Some(client_hint))
            .unwrap_or_else(|_| said_forge::OpenApiStandard::defaults());
        let (corrected_yaml, report) =
            said_forge::fitter::fit_yaml(&sandbox, original_yaml, catalog, &standard).await?;
        // Write corrections.md alongside the spec.
        let corrections_path = deliverables_root.join("corrections.md");
        said_forge::fitter::write_corrections(&report, client_hint, &corrections_path)?;
        let fixes_path = deliverables_root.join("fixes.md");
        said_forge::fitter::write_fixes(&report, client_hint, &fixes_path)?;
        let ambiguities_path = deliverables_root.join("ambiguities.md");
        said_forge::fitter::write_ambiguities(&report, client_hint, &ambiguities_path)?;
        // One-line operator hint.
        if report.has_anything() {
            eprintln!(
                "  â†’ fit pass: {} corrections applied ({} paths, {} params, {} added) â€” {}",
                report.total_corrections(),
                report.path_rewrites.len(),
                report.param_rewrites.len(),
                report.missing_ops_added.len(),
                corrections_path.display(),
            );
            if !report.missing_ops_logged.is_empty() {
                eprintln!(
                    "  â“˜ {} registry path(s) without a matching proc â€” logged for review",
                    report.missing_ops_logged.len()
                );
            }
            if !report.surplus_ops.is_empty() {
                eprintln!(
                    "  â“˜ {} spec op(s) not in registry â€” left in spec untouched (v1)",
                    report.surplus_ops.len()
                );
            }
        }
        Ok(corrected_yaml)
    }

    #[cfg(not(feature = "forge-sql-verify"))]
    async fn run_fit_pass_if_possible(
        original_yaml: &str,
        _deliverables_root: &Path,
        _client_hint: &str,
        _catalog: &said_forge::sql_catalog::SqlCatalog,
        _workspace_root: &Path,
    ) -> Result<String, String> {
        Ok(original_yaml.to_string())
    }

    async fn cmd_docs(
        target: Option<&Path>,
        out: Option<&Path>,
        only: Option<&str>,
        verify_against_sandbox: bool,
        max_enum_rows: usize,
        client: Option<&str>,
        from_sql: bool,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::comparison::{build_comparison, emit_report_file};
        use said_forge::gap_structured::{
            load_openapi_ops, read_xlsx_rows_from_brain, walk_dev_planning,
        };
        use said_forge::sql_catalog::build_catalog;
        use said_forge::workspace_config::{DirectiveMode, WorkspaceConfig};
        use said_forge::{
            generate_story_docs, BusinessConfig, Glossary, MappingOverrides, MappingService,
            StoryMeta,
        };
        use std::collections::BTreeMap;

        let root = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let cfg_path = WorkspaceConfig::default_path_for(&root);
        if !cfg_path.exists() {
            return Err(format!(
                "no .forge/config.toml at {} â€” run `said forge plan` first",
                cfg_path.display()
            ));
        }
        let cfg = WorkspaceConfig::load(&cfg_path)
            .map_err(|e| format!("load config: {}", e))?;
        if cfg.directive.mode == DirectiveMode::Unset {
            return Err(
                "no directive configured in .forge/config.toml â€” re-run `said forge plan`".into(),
            );
        }

        let dev_planning_root: PathBuf = match cfg.directive.primary.as_deref() {
            Some(p) => resolve_dev_planning_root(&root.join(p))
                .unwrap_or_else(|| root.join("4-expectations/Dev Planning")),
            None => root.join("4-expectations/Dev Planning"),
        };

        let groups = walk_dev_planning(&dev_planning_root)
            .map_err(|e| format!("walk dev planning: {}", e))?;
        let mut ops: Vec<_> = groups
            .into_values()
            .flat_map(|group| group.into_iter().map(|(_, op)| op))
            .collect();
        if let Some(needle) = only {
            let needle_lc = needle.to_ascii_lowercase();
            ops.retain(|op| {
                op.slug.to_ascii_lowercase().contains(&needle_lc)
                    || op.label.to_ascii_lowercase().contains(&needle_lc)
            });
            if ops.is_empty() {
                return Err(format!(
                    "no op matched --only `{}` â€” try a broader substring of the slug or label",
                    needle
                ));
            }
        }
        if ops.is_empty() && !from_sql {
            return Err(format!(
                "no ops found under {} â€” run `said forge sync` to ingest Dev Planning first, \
                 or pass --from-sql to generate the spec from SQL alone",
                dev_planning_root.display()
            ));
        }

        let mut catalog =
            build_catalog(&root).map_err(|e| format!("build catalog: {}", e))?;
        // Multi-client: keep only the catalog entries whose source file
        // lives under `1-ground-truth/<Client>/`. Single-client
        // workspaces (no `--client`) keep the full catalog.
        if let Some(client_name) = client {
            let needle = format!("/{}/", client_name);
            let needle_lc = needle.to_lowercase();
            // TableSchema has no source_path â€” keep all tables when
            // filtering by client (the SqlObject filter below still
            // narrows procs/views/triggers to the right client).
            let _ = &needle_lc;
            catalog.objects.retain(|o| {
                o.source_path.replace('\\', "/").to_lowercase().contains(&needle_lc)
            });
        }
        let glossary = Glossary::build(&catalog);
        let overrides_path = root.join(".forge").join("mapping.toml");
        let overrides = MappingOverrides::load(&overrides_path)
            .map_err(|e| format!("load mapping.toml: {}", e))?;

        let service = MappingService::new(&catalog, &glossary, &overrides);

        // XLSM rows from the brain â€” feeds Section 12 of each story.
        let xlsx_rows = match find_workspace_said(&root) {
            Ok(said_path) => match SaidFile::open(&said_path) {
                Ok(mut brain) => read_xlsx_rows_from_brain(&mut brain),
                Err(_) => Vec::new(),
            },
            Err(_) => Vec::new(),
        };

        // Load business config â€” default placeholders if missing.
        let business_path = root.join(".forge").join("business.toml");
        let business = BusinessConfig::load(&business_path)
            .map_err(|e| format!("load business.toml: {}", e))?;
        let business_is_placeholder = business.is_placeholder();

        // StoryMeta lookup â€” was previously populated from XLSM rows
        // via said_forge::build_metas_from_xlsx, but that helper was
        // removed. Ops without a meta entry use TBD placeholders, which
        // is the correct fallback for now.
        let metas: BTreeMap<String, StoryMeta> = BTreeMap::new();
        let _ = &xlsx_rows;

        // â”€â”€ Comparison report (per epic) â”€â”€
        // Built before generate_story_docs so the per-row data can be
        // injected into each story's "SQL Source of Truth" section.
        // We re-walk Dev Planning unfiltered so the comparison covers
        // the full epic even when --only narrowed the story emit.
        let groups_full = walk_dev_planning(&dev_planning_root)
            .map_err(|e| format!("walk dev planning (full): {}", e))?;
        let dev_planning_ops_full: Vec<_> = groups_full
            .into_values()
            .flat_map(|group| group.into_iter().map(|(_, op)| op))
            .collect();
        let openapi_ops: Vec<_> = match cfg.directive.secondary.as_deref() {
            Some(p) => {
                let abs = root.join(p);
                if abs.is_file() {
                    load_openapi_ops(&abs)
                        .map_err(|e| format!("load openapi: {}", e))?
                } else {
                    Vec::new()
                }
            }
            None => Vec::new(),
        };
        // Cross-layer literal lineage scan: previously read from
        // `[lineage] cs_root` in `.forge/config.toml`. The lineage
        // field was removed from WorkspaceConfig â€” fall back to the
        // default scan (`<root>/2-progress/` if it exists).
        let cs_root_path: Option<std::path::PathBuf> = None;
        let cs_root_override = cs_root_path.as_deref();
        let _ = &cfg;
        let comparison = build_comparison(
            &business.epic,
            "cardholder",
            &catalog,
            &openapi_ops,
            &dev_planning_ops_full,
            &root,
            &root,
            cs_root_override,
        );
        // Multi-client output: `5-deliverables/<Client>/...` when
        // `--client` is set, falling back to the legacy flat layout
        // `5-deliverables/...` for single-client workspaces.
        let deliverables_root: PathBuf = match client {
            Some(c) => root.join("5-deliverables").join(c),
            None => root.join("5-deliverables"),
        };
        let comparison_dir = deliverables_root.join("comparison");
        let comparison_path = emit_report_file(&comparison, &comparison_dir)
            .map_err(|e| format!("emit comparison: {}", e))?;

        // â”€â”€ SQL â†’ OpenAPI generated spec â”€â”€
        // Database-first: regenerate the API spec from SQL ground
        // truth. The client's wishlist OpenAPI lives in 4-expectations/
        // and gets diffed against this canonical spec by the
        // comparison report above.
        //
        // When --verify-against-sandbox is set AND forge was built
        // with the `forge-sql-verify` feature, we connect to a
        // running `said-sbx-*` container, pull the lookup row counts
        // and pull values for closed-set tables (â‰¤25 rows). Those
        // become `enum:` overrides in the generated spec.
        let enum_overrides = run_sandbox_verification_if_requested(
            verify_against_sandbox,
            &catalog,
            max_enum_rows,
            client,
        )
        .await;
        let enum_overrides_ref = enum_overrides.as_ref();
        // `--from-sql` walks every schema in the catalog (use case:
        // clients without a curated single-module slice â€” e.g. Vivere).
        // Otherwise restrict to `cardholder` for the curated TXN flow.
        let openapi_schema_filter: &str = if from_sql { "" } else { "cardholder" };
        // Client-aware spec title: any `--client X` invocation should
        // produce a client-named spec (e.g. "TXN API"), regardless of
        // whether `--from-sql` is also passed. The legacy
        // `business.epic` value ("Cardholder Management") is the
        // single-client default kept only for backwards compat when
        // no `--client` is given.
        let title = match client {
            Some(c) => format!("{} API", c),
            None => business.epic.clone(),
        };
        let generated = said_forge::sql_to_openapi::generate_openapi(
            &title,
            openapi_schema_filter,
            &catalog,
            &root,
            enum_overrides_ref,
        );
        let generated_path = deliverables_root
            .join("api-specification.generated.yml");
        if let Some(parent) = generated_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create deliverables dir: {}", e))?;
        }

        // â”€â”€â”€ APPLY DEV SPEC TABLES + REGISTRY TO SANDBOX â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // The drift check (run after spec generation) compares the
        // Expected ERD against what's actually in the sandbox database.
        // For that comparison to be meaningful, the sandbox must first
        // contain the tables Dev Spec demands and the registry rows
        // Dev Spec amends. Apply both before spec generation so:
        //   1. The registry-first spec emitter sees all 159 paths
        //      (43 seed + 116 amendments) instead of just the 43 seed.
        //   2. The final ERD render reflects the live DB shape.
        //   3. Drift output reports actual divergence, not "everything
        //      is missing because nothing was applied yet".
        #[cfg(feature = "forge-sql-verify")]
        if verify_against_sandbox {
            if let Err(e) = apply_dev_spec_to_sandbox(
                client.unwrap_or("default"),
                &root,
                &deliverables_root,
            ).await {
                eprintln!("  âš  Dev Spec sandbox apply failed: {}", e);
            }
        }

        // â”€â”€â”€ REGISTRY-FIRST SPEC GENERATION â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // When the sandbox has a recognised registry table
        // (`ars_Api_Rule_Settings` or equivalent) we trust it as the
        // source of truth: emit one operation per (path, aml_Code) row
        // and let the 6-gate matcher bind procs. The catalog-walk
        // generator runs only for clients without a registry.
        let final_yaml = if verify_against_sandbox {
            let registry_first = build_registry_first_spec(
                &title,
                &deliverables_root,
                client.unwrap_or("default"),
                &catalog,
                &root,
            )
            .await
            .ok()
            .flatten();
            match registry_first {
                Some(yaml) => yaml,
                None => run_fit_pass_if_possible(
                    &generated.yaml,
                    &deliverables_root,
                    client.unwrap_or("default"),
                    &catalog,
                    &root,
                )
                .await
                .unwrap_or_else(|_| generated.yaml.clone()),
            }
        } else {
            generated.yaml.clone()
        };
        std::fs::write(&generated_path, &final_yaml)
            .map_err(|e| format!("write generated spec: {}", e))?;

        // â”€â”€â”€ ERD DRIFT CHECK â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // After the spec is generated, compare the **Expected ERD**
        // (Dev Spec markdown + borrow decisions) against the **Live DB**
        // (sandbox sys.* + ars_Api_Rule_Settings). Writes an additive
        // `erd-drift.md` + `sql/erd-drift-alter.sql`. Observation only â€”
        // never executes SQL against the database.
        #[cfg(feature = "forge-sql-verify")]
        if verify_against_sandbox {
            if let Err(e) = run_erd_drift_check(
                &deliverables_root,
                client.unwrap_or("default"),
                &root,
                false, // strict mode disabled by default
            ).await {
                eprintln!("  âš  ERD drift check skipped: {}", e);
            }
        }

        // Map ops to comparison rows by op.slug so generate_story_docs
        // can inject the per-op SQL summary. We key on the Dev
        // Planning op slug since that's what the story renderer iterates.
        let mut comparison_by_slug: BTreeMap<String, said_forge::comparison::ComparisonRow> =
            BTreeMap::new();
        for op in &ops {
            // Find the comparison row whose Dev Planning op matches ours.
            for row in &comparison.rows {
                if let Some(dv) = &row.dev_planning {
                    let same_method = op
                        .method
                        .as_deref()
                        .map(|m| m.eq_ignore_ascii_case(&dv.method))
                        .unwrap_or(false);
                    let same_path = op
                        .path
                        .as_deref()
                        .map(|p| p.eq_ignore_ascii_case(&dv.path))
                        .unwrap_or(false);
                    if same_method && same_path {
                        comparison_by_slug.insert(op.slug.clone(), row.clone());
                        break;
                    }
                }
            }
        }

        let out_dir = out
            .map(Path::to_path_buf)
            .unwrap_or_else(|| deliverables_root.join("stories"));
        let _ = &comparison_by_slug; // reserved for future per-story comparison injection
        let report = generate_story_docs(
            &ops,
            &metas,
            &service,
            &catalog,
            &xlsx_rows,
            &business,
            &out_dir,
        )
        .map_err(|e| format!("generate story docs: {}", e))?;

        if json {
            let payload = serde_json::json!({
                "ok": true,
                "out_dir": normalize_display_path(&report.out_dir),
                "files_written": report.files_written.len(),
                "total_ops": report.total_ops,
                "catalog_tables": catalog.tables.len(),
                "glossary_terms": glossary.len(),
                "business_config_is_placeholder": business_is_placeholder,
                "comparison_report": normalize_display_path(&comparison_path),
                "comparison_rows": comparison.rows.len(),
                "comparison_orphans": comparison.orphan_procs.len(),
                "generated_openapi": normalize_display_path(&generated_path),
                "generated_op_count": generated.op_count,
                "generated_orphan_proc_count": generated.orphan_proc_count,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_default()
            );
        } else {
            println!(
                "âœ“ Story docs written for {} ops at {}",
                report.total_ops,
                normalize_display_path(&report.out_dir)
            );
            println!(
                "  {} files Â· catalog {} tables Â· glossary {} terms",
                report.files_written.len(),
                catalog.tables.len(),
                glossary.len()
            );
            println!(
                "âœ“ Comparison report at {} ({} ops, {} orphan procs)",
                normalize_display_path(&comparison_path),
                comparison.rows.len(),
                comparison.orphan_procs.len(),
            );
            println!(
                "âœ“ Generated OpenAPI from SQL at {} ({} ops, {} internal procs)",
                normalize_display_path(&generated_path),
                generated.op_count,
                generated.orphan_proc_count,
            );
            if business_is_placeholder {
                println!(
                    "  ! business.toml has placeholder values â€” edit {} and re-run for signed-off filenames",
                    normalize_display_path(&business_path)
                );
            }
        }
        Ok(())
    }

    fn cmd_forge_snapshot(
        target: Option<&Path>,
        module: &str,
        json: bool,
    ) -> Result<(), String> {
        let target = target.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let brain_path = locate_workspace_brain(&target)?;
        let mut brain = open_brain(&brain_path)?;
        let project_name = workspace_project_name(&target);

        let port = said_forge::sandbox::DEFAULT_FORGE_SANDBOX_PORT;
        let out = said_forge::sandbox::build_forge_sandbox(
            &mut brain, &target, module, port, &project_name,
        )?;

        if json {
            let v = serde_json::json!({
                "ok": true,
                "module": module,
                "project": project_name,
                "sandbox_dir": normalize_display_path(&out.sandbox_dir),
                "container_name": out.container_name,
                "host_port": out.host_port,
                "tables": out.tables,
                "procs": out.procs,
                "functions": out.functions,
                "views": out.views,
                "triggers": out.triggers,
                "merge_files": out.merge_files,
                "seed_inserts": out.seed_inserts,
            });
            println!("{}", v);
        } else {
            println!("Forge sandbox files written to {}/", normalize_display_path(&out.sandbox_dir));
            println!();
            println!("Module:    {}", module);
            println!("Container: {} (will start on port {})", out.container_name, out.host_port);
            println!();
            println!("Schema:");
            println!("  - {} tables (FK-sorted)", out.tables);
            println!("  - {} functions", out.functions);
            println!("  - {} views", out.views);
            println!("  - {} stored procedures", out.procs);
            println!("  - {} triggers", out.triggers);
            println!();
            println!("Data:");
            println!("  - {} INSERT seed statements", out.seed_inserts);
            println!("  - {} MERGE-style lookup-data files", out.merge_files);
            println!();
            println!("Next: said forge sandbox {} --port {}", module, out.host_port);
            println!("       (or: cd {} && bash run.sh)", normalize_display_path(&out.sandbox_dir));
        }
        Ok(())
    }

    fn cmd_forge_sandbox(
        target: Option<&Path>,
        module: &str,
        port: Option<u16>,
        no_up: bool,
        json: bool,
    ) -> Result<(), String> {
        let target = target.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let brain_path = locate_workspace_brain(&target)?;
        let mut brain = open_brain(&brain_path)?;
        let project_name = workspace_project_name(&target);

        let port = port.unwrap_or(said_forge::sandbox::DEFAULT_FORGE_SANDBOX_PORT);
        let out = said_forge::sandbox::build_forge_sandbox(
            &mut brain, &target, module, port, &project_name,
        )?;

        if !json {
            println!("Forge sandbox files written to {}/", normalize_display_path(&out.sandbox_dir));
            println!("Module:    {}", module);
            println!("Container: {}", out.container_name);
            println!("Port:      {}", out.host_port);
            println!("Schema: {} tables, {} functions, {} views, {} procs, {} triggers",
                out.tables, out.functions, out.views, out.procs, out.triggers);
            println!("Data:   {} INSERTs, {} MERGE files",
                out.seed_inserts, out.merge_files);
            println!();
        }

        if no_up {
            if !json {
                println!("Skipping `docker compose up` (--no-up).");
                println!("Bring up manually:  cd {} && bash run.sh",
                    normalize_display_path(&out.sandbox_dir));
            } else {
                println!("{}", serde_json::json!({
                    "ok": true,
                    "module": module,
                    "sandbox_dir": normalize_display_path(&out.sandbox_dir),
                    "container_name": out.container_name,
                    "host_port": out.host_port,
                    "started": false,
                }));
            }
            return Ok(());
        }

        if !json {
            println!("Bringing up container...");
        }
        let report = said_forge::sandbox::bring_up_sandbox(&out)?;

        if json {
            println!("{}", serde_json::json!({
                "ok": true,
                "module": module,
                "sandbox_dir": normalize_display_path(&out.sandbox_dir),
                "container_name": out.container_name,
                "host_port": out.host_port,
                "started": report.compose_started,
                "healthy": report.healthy,
                "script_errors": report.script_errors,
            }));
        } else {
            println!("âœ“ Compose started, SQL Server healthy.");
            for (label, n) in &report.script_errors {
                println!("  {}: {} non-fatal errors", label, n);
            }
            println!();
            println!("ðŸŸ¢ Forge sandbox LIVE on port {}", out.host_port);
            println!("Connection: Server=localhost,{};User=sa;Password={}",
                out.host_port, said_forge::sandbox::SANDBOX_PASSWORD);
        }
        Ok(())
    }

    /// Run the contract test suite for one client. Reads the
    /// already-generated OpenAPI spec at
    /// `<workspace>/5-deliverables/<Client>/api-specification.generated.yml`,
    /// extracts every operation + body schema + closed-set enums, then
    /// drives the tester against the matching live sandbox container.
    /// Writes a markdown report to
    /// `<workspace>/5-deliverables/<Client>/test-report.md`.
    #[cfg(feature = "forge-sql-verify")]
    async fn cmd_forge_test(
        target: Option<&Path>,
        client: Option<&str>,
        only: Option<&str>,
        json: bool,
    ) -> Result<(), String> {
        let workspace = target_or_cwd(target);
        let spec_dir = match client {
            Some(c) => workspace.join("5-deliverables").join(c),
            None => workspace.join("5-deliverables"),
        };
        let spec_path = spec_dir.join("api-specification.generated.yml");
        if !spec_path.exists() {
            return Err(format!(
                "no spec at {} â€” run `said forge docs --client {} --verify-against-sandbox` first",
                spec_path.display(),
                client.unwrap_or(""),
            ));
        }

        // Discover the matching sandbox container. Tester needs a live
        // SQL Server to EXEC procs against.
        use said_forge::sql_verify::discover_sandbox_with_hint;
        let sandbox = discover_sandbox_with_hint(client).ok_or_else(|| {
            format!(
                "no sandbox container found{} â€” run `said sandbox {} --up` first",
                client.map(|c| format!(" matching `{}`", c)).unwrap_or_default(),
                client.unwrap_or("<module>"),
            )
        })?;

        let mut ops = said_forge::tester::extract_ops_from_spec_file(&spec_path)?;
        if let Some(needle) = only {
            let n = needle.to_lowercase();
            ops.retain(|o| {
                o.path.to_lowercase().contains(&n)
                    || o.proc_full_name.to_lowercase().contains(&n)
                    || o.method.to_lowercase().contains(&n)
            });
            if ops.is_empty() {
                return Err(format!(
                    "no operations matched --only `{}`",
                    needle
                ));
            }
        }
        if ops.is_empty() {
            return Err(format!(
                "spec at {} contains no operations â€” nothing to test",
                spec_path.display()
            ));
        }

        if !json {
            eprintln!(
                "Testing {} ops against `{}` (port {})...",
                ops.len(),
                sandbox.container_name,
                sandbox.host_port,
            );
        }
        let client_label = client.unwrap_or("(default)").to_string();
        let report = said_forge::tester::run_tests(&sandbox, &client_label, &ops).await?;

        let report_path = spec_dir.join("test-report.md");
        said_forge::tester::write_report(&report, &report_path)?;

        if json {
            println!("{}", serde_json::to_string_pretty(&report)
                .unwrap_or_default());
        } else {
            println!();
            println!("âœ“ Contract tests complete.");
            println!(
                "  {} of {} assertions passed.",
                report.passed_assertions(),
                report.total_assertions(),
            );
            if report.failed_assertions() > 0 {
                println!(
                    "  {} divergence{} flagged â€” see {}",
                    report.failed_assertions(),
                    if report.failed_assertions() == 1 { "" } else { "s" },
                    report_path.display()
                );
            } else {
                println!("  Report: {}", report_path.display());
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "forge-sql-verify"))]
    async fn cmd_forge_test(
        _target: Option<&Path>,
        _client: Option<&str>,
        _only: Option<&str>,
        _json: bool,
    ) -> Result<(), String> {
        Err(
            "Contract testing requires the `forge-sql-verify` feature. \
             Re-run `cargo build -p said-cli --features forge,forge-sql-verify`."
                .into()
        )
    }

    /// Find the workspace `.said` brain. Convention: a single `*.said` at
    /// workspace root, or `<workspace_name>.said` matching the dir name.
    fn locate_workspace_brain(target: &Path) -> Result<PathBuf, String> {
        // First: any *.said in target directly.
        if let Ok(entries) = std::fs::read_dir(target) {
            let mut candidates: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("said"))
                .collect();
            if candidates.len() == 1 {
                return Ok(candidates.remove(0));
            }
            if candidates.len() > 1 {
                // Prefer one that matches the directory name.
                if let Some(dir_name) = target.file_name().and_then(|n| n.to_str()) {
                    let prefer = candidates.iter().find(|p| {
                        p.file_stem().and_then(|s| s.to_str()) == Some(dir_name)
                    });
                    if let Some(p) = prefer {
                        return Ok(p.clone());
                    }
                }
                return Err(format!(
                    "multiple .said files in {}; pass --path explicitly",
                    target.display()
                ));
            }
        }
        Err(format!(
            "no .said brain found in {}; run `said forge init` first",
            target.display()
        ))
    }

    fn workspace_project_name(target: &Path) -> String {
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("forge-workspace")
            .to_string()
    }

    fn cmd_regen(
        target: Option<&Path>,
        from: Option<&Path>,
        out: Option<&Path>,
        name: &str,
        force: bool,
        json: bool,
    ) -> Result<(), String> {
        use said_forge::sql_catalog::build_catalog;
        use said_forge::{regenerate_skill, Glossary, RegenOptions};

        let root = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let old_skill_dir = from
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.join(".claude").join("skills").join(name));
        let output_root = out.map(Path::to_path_buf).unwrap_or_else(|| root.clone());

        // Build catalog from the workspace's 1-ground-truth SQL dir. Fall
        // back to scanning the workspace root if the standard layout is
        // absent â€” regen still works, it just sees fewer tables.
        let catalog =
            build_catalog(&root).map_err(|e| format!("build catalog: {}", e))?;
        let glossary = Glossary::build(&catalog);

        let opts = RegenOptions {
            force,
            skill_name: name.to_string(),
            ..RegenOptions::default()
        };

        match regenerate_skill(&old_skill_dir, &output_root, &catalog, &glossary, &opts) {
            Ok(report) => {
                if json {
                    let payload = serde_json::json!({
                        "ok": true,
                        "skill_path": normalize_display_path(&report.skill_path),
                        "preserved": report.preserved,
                        "regenerated": report.regenerated.iter().map(|e| &e.name).collect::<Vec<_>>(),
                        "generated_new": report.generated_new,
                        "removed": report.removed,
                        "blocked_large_diff": report.blocked_large_diff.iter().map(|e| &e.name).collect::<Vec<_>>(),
                        "catalog_tables": catalog.tables.len(),
                        "glossary_terms": glossary.len(),
                    });
                    println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
                } else {
                    println!(
                        "âœ“ Skill regenerated at {}",
                        normalize_display_path(&report.skill_path)
                    );
                    println!(
                        "  {} preserved Â· {} regenerated Â· {} new Â· {} removed (catalog: {} tables, glossary: {} terms)",
                        report.preserved.len(),
                        report.regenerated.len(),
                        report.generated_new.len(),
                        report.removed.len(),
                        catalog.tables.len(),
                        glossary.len(),
                    );
                    println!(
                        "  Review changes in {}/REVIEW.md before handing to developers.",
                        normalize_display_path(&report.skill_path)
                    );
                }
                Ok(())
            }
            Err(e) => Err(format!("regen: {}", e)),
        }
    }

    async fn load_directive_stories(
        path: &Path,
    ) -> Result<Vec<said_forge::Story>, String> {
        let registry = SourceRegistry::default();
        let path_str = path.to_string_lossy().to_string();
        let adapter = registry
            .detect(&path_str)
            .map_err(|e| format!("detect adapter for {}: {}", path.display(), e))?;
        let doc = adapter
            .load(&path_str, "forge-gaps")
            .await
            .map_err(|e| format!("load {}: {}", path.display(), e))?;
        adapter
            .extract_stories(&doc)
            .map_err(|e| format!("extract stories from {}: {}", path.display(), e))
    }

    fn project_name_from(said_path: &Path) -> String {
        said_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("forge")
            .to_string()
    }

    fn cmd_plan(
        target: Option<&Path>,
        reconfigure: bool,
        json: bool,
    ) -> Result<(), String> {
        let root = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let cfg = said_forge::plan_cli::run_interactive_plan(&root, reconfigure)
            .map_err(|e| format!("forge plan: {}", e))?;
        if json {
            let out = serde_json::json!({
                "ok": true,
                "plan_complete": cfg.plan_complete,
                "directive_mode": format!("{:?}", cfg.directive.mode),
                "config_path": normalize_display_path(
                    &said_forge::workspace_config::WorkspaceConfig::default_path_for(&root),
                ),
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        }
        Ok(())
    }

    /// Render a path for display using the native platform separator. On
    /// Windows the bash-shell-friendly forward slashes look inconsistent; on
    /// Unix backslashes are wrong. Normalise per-target.
    fn normalize_display_path(p: &Path) -> String {
        let s = p.display().to_string();
        if cfg!(windows) {
            s.replace('/', "\\")
        } else {
            s.replace('\\', "/")
        }
    }

    fn platform_sep() -> char {
        if cfg!(windows) { '\\' } else { '/' }
    }

    fn resolve_path(path: Option<&str>) -> Result<PathBuf, String> {
        match path {
            Some(p) => Ok(PathBuf::from(p)),
            None => Err("--path <file.said> required for said forge".into()),
        }
    }

    fn project_root_from(brain_path: &Path) -> PathBuf {
        brain_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn open_brain(path: &Path) -> Result<SaidFile, String> {
        if path.exists() {
            SaidFile::open(path)
        } else {
            Ok(SaidFile::create(path))
        }
    }

    fn whoami() -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "anonymous".into())
    }

    // â”€â”€â”€ load â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    async fn cmd_load(
        brain_path: &Path,
        _cfg: &ForgeConfig,
        path_or_url: &str,
        source: Option<&str>,
        json: bool,
    ) -> Result<(), String> {
        let registry = SourceRegistry::default();
        let adapter = match source {
            Some(name) => registry.by_name(name).map_err(|e| e.to_string())?,
            None => registry.detect(path_or_url).map_err(|e| e.to_string())?,
        };
        let operator = whoami();
        let doc = adapter
            .load(path_or_url, &operator)
            .await
            .map_err(|e| e.to_string())?;
        let stories = adapter.extract_stories(&doc).map_err(|e| e.to_string())?;

        let mut brain = open_brain(brain_path)?;
        let mut sfb = SaidFileBrain::new(&mut brain);
        let hash = frame::write_directive(&mut sfb, &doc).map_err(|e| e.to_string())?;
        frame::write_stories(&mut sfb, &stories).map_err(|e| e.to_string())?;
        drop(sfb);
        brain.save()?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "directive_hash": hash,
                    "adapter": adapter.name(),
                    "stories": stories.len(),
                })
            );
        } else {
            println!(
                "Loaded directive {} via {}: {} stories",
                hash,
                adapter.name(),
                stories.len()
            );
        }
        Ok(())
    }

    // â”€â”€â”€ list â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn cmd_list(brain_path: &Path, filter: Option<&str>, json: bool) -> Result<(), String> {
        let mut brain = open_brain(brain_path)?;
        let mut sfb = SaidFileBrain::new(&mut brain);
        let hash = match frame::latest_directive_hash(&sfb) {
            Some(h) => h,
            None => {
                if json {
                    println!("{}", serde_json::json!({"stories": []}));
                } else {
                    println!("No directive loaded. Run `said forge load <path>` first.");
                }
                return Ok(());
            }
        };
        let stories = read_stories(&mut sfb, &hash);
        let f = match filter {
            Some(expr) => Some(said_forge::filter_parse(expr).map_err(|e| e.to_string())?),
            None => None,
        };

        let filtered: Vec<&Story> = stories
            .iter()
            .filter(|s| f.as_ref().map_or(true, |ff| said_forge::filter_matches(s, ff)))
            .collect();

        if json {
            let rows: Vec<serde_json::Value> = filtered
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "slug": s.slug,
                        "title": s.title,
                        "kind": s.kind,
                        "method": s.fields.get("method"),
                        "path": s.fields.get("path"),
                    })
                })
                .collect();
            println!("{}", serde_json::json!({"stories": rows}));
        } else {
            for (i, s) in filtered.iter().enumerate() {
                let method = s
                    .fields
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let path = s
                    .fields
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                println!(
                    "#{:03}  {:<6}  {:<40}  {}",
                    i + 1,
                    method,
                    path,
                    s.title
                );
            }
            if filtered.is_empty() {
                println!("(no stories match filter)");
            }
        }
        Ok(())
    }

    // â”€â”€â”€ show â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn cmd_show(brain_path: &Path, slug: &str, json: bool) -> Result<(), String> {
        let mut brain = open_brain(brain_path)?;
        let mut sfb = SaidFileBrain::new(&mut brain);
        let hash = frame::latest_directive_hash(&sfb)
            .ok_or_else(|| "No directive loaded.".to_string())?;
        let sanitized = said_forge::sanitize_slug(slug);
        let mut out = Vec::<(String, String)>::new();
        for ty in ["spec", "plan", "tasks", "brain"] {
            let tag = said_forge::forge_tag(ty, &hash, &sanitized, None, None);
            let body = said_forge::frame::BrainIo::find_body_by_tag(&mut sfb, &tag)
                .unwrap_or_else(|| "(not yet generated)".into());
            out.push((ty.into(), body));
        }
        if json {
            let mut obj = serde_json::Map::new();
            for (ty, body) in &out {
                obj.insert(ty.clone(), serde_json::Value::String(body.clone()));
            }
            println!("{}", serde_json::Value::Object(obj));
        } else {
            for (ty, body) in &out {
                println!("\n========== {} ==========\n{}", ty, body);
            }
        }
        Ok(())
    }

    // â”€â”€â”€ status â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn cmd_status(brain_path: &Path, story: Option<&str>, json: bool) -> Result<(), String> {
        let mut brain = open_brain(brain_path)?;
        let sfb = SaidFileBrain::new(&mut brain);
        let hash = frame::latest_directive_hash(&sfb)
            .ok_or_else(|| "No directive loaded.".to_string())?;
        match story {
            Some(slug) => {
                let sanitized = said_forge::sanitize_slug(slug);
                let run_n = frame::latest_run_n(&sfb, &hash, &sanitized);
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "slug": sanitized,
                            "latest_run": run_n,
                        })
                    );
                } else {
                    println!("story: {}  latest_run: r{}", sanitized, run_n);
                }
            }
            None => {
                drop(sfb);
                let mut sfb2 = SaidFileBrain::new(&mut brain);
                let stories = read_stories(&mut sfb2, &hash);
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "directive": hash,
                            "total_stories": stories.len(),
                        })
                    );
                } else {
                    println!(
                        "directive: {}   total stories: {}",
                        hash,
                        stories.len()
                    );
                }
            }
        }
        Ok(())
    }

    // â”€â”€â”€ run â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    async fn cmd_run(
        project_root: &Path,
        brain_path: &Path,
        cfg: &ForgeConfig,
        all: bool,
        ids: Option<&str>,
        filter: Option<&str>,
        force: bool,
        yes: bool,
        halt_after: Option<u32>,
        json: bool,
    ) -> Result<(), String> {
        let mut brain = open_brain(brain_path)?;
        let hash = {
            let sfb = SaidFileBrain::new(&mut brain);
            frame::latest_directive_hash(&sfb)
                .ok_or_else(|| "No directive loaded.".to_string())?
        };
        let stories = {
            let mut sfb = SaidFileBrain::new(&mut brain);
            read_stories(&mut sfb, &hash)
        };

        let selected: Vec<Story> = if let Some(ids_csv) = ids {
            let wanted: std::collections::HashSet<String> =
                ids_csv.split(',').map(|s| s.trim().to_string()).collect();
            stories.into_iter().filter(|s| wanted.contains(&s.slug)).collect()
        } else if let Some(expr) = filter {
            let f = said_forge::filter_parse(expr).map_err(|e| e.to_string())?;
            stories
                .into_iter()
                .filter(|s| said_forge::filter_matches(s, &f))
                .collect()
        } else if all {
            stories
        } else {
            return Err("must pass --all, --ids, or --filter".into());
        };

        let threshold = halt_after.unwrap_or(cfg.halt_after);

        // Preflight cost estimate
        let estimate = said_forge::preflight_estimate(cfg, selected.len() as u32, 3000, 1000);
        if !json {
            println!(
                "Selected {} stories.  Estimated cost (at 3k in / 1k out each): ${:.2}",
                selected.len(),
                estimate
            );
        }
        if !yes && !json {
            print!("Continue? [y/N] ");
            std::io::stdout().flush().ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("aborted.");
                return Ok(());
            }
        }

        let llm = provider_from_config(&cfg).map_err(|e| e.to_string())?;
        let adapter = ClaudeAdapter;
        let mut breaker = CircuitBreaker::new(threshold);

        let inline_top_n = cfg.grounding.inline_top_n as usize;
        let max_frames = cfg.grounding.max_frames as usize;
        let opts = RunOptions {
            project_root,
            config: cfg,
            force,
            halt_after: threshold,
            operator: whoami(),
            inline_top_n,
            max_frames,
        };

        let total = selected.len();
        let mut succeeded = 0;
        let mut skipped = 0;
        let mut failed = 0;
        let mut halted = false;

        for (idx, story) in selected.iter().enumerate() {
            let mut sfb = SaidFileBrain::new(&mut brain);
            let outcome = run_one(&mut sfb, story, llm.as_ref(), &adapter, &opts).await;
            drop(sfb);

            match outcome.status {
                StoryStatus::Completed => {
                    succeeded += 1;
                    if !json {
                        println!(
                            "[{}/{}] {}  âœ“  {}",
                            idx + 1,
                            total,
                            outcome.slug,
                            outcome.message
                        );
                    }
                }
                StoryStatus::Skipped => {
                    skipped += 1;
                    if !json {
                        println!("[{}/{}] {}  âŠ˜  skipped", idx + 1, total, outcome.slug);
                    }
                }
                _ => {
                    failed += 1;
                    if !json {
                        println!(
                            "[{}/{}] {}  âœ—  {}: {}",
                            idx + 1,
                            total,
                            outcome.slug,
                            outcome.status.class_name(),
                            outcome.message
                        );
                    }
                }
            }
            if breaker.record(&outcome.status) {
                halted = true;
                if !json {
                    println!(
                        "circuit breaker: {} consecutive {} failures â€” halting",
                        threshold,
                        breaker.last_class().unwrap_or("unknown")
                    );
                }
                break;
            }
        }
        brain.save()?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "total": total,
                    "succeeded": succeeded,
                    "skipped": skipped,
                    "failed": failed,
                    "halted": halted,
                })
            );
        } else {
            println!(
                "done â€” {} succeeded, {} skipped, {} failed{}",
                succeeded,
                skipped,
                failed,
                if halted { " (breaker halted batch)" } else { "" }
            );
        }
        Ok(())
    }

    // â”€â”€â”€ reset â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn cmd_reset(
        project_root: &Path,
        brain_path: &Path,
        slug: &str,
        yes: bool,
        json: bool,
    ) -> Result<(), String> {
        if !yes && !json {
            print!(
                "Tombstone all frames + delete .forge/{}/ + .claude/skills/{}/? [y/N] ",
                slug, slug
            );
            std::io::stdout().flush().ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("aborted.");
                return Ok(());
            }
        }
        let mut brain = open_brain(brain_path)?;
        let hash = {
            let sfb = SaidFileBrain::new(&mut brain);
            frame::latest_directive_hash(&sfb)
                .ok_or_else(|| "No directive loaded.".to_string())?
        };
        let sanitized = said_forge::sanitize_slug(slug);
        let n = {
            let mut sfb = SaidFileBrain::new(&mut brain);
            frame::tombstone_story(&mut sfb, &hash, &sanitized).map_err(|e| e.to_string())?
        };
        brain.save()?;
        said_forge::remove_folder(project_root, &sanitized).map_err(|e| e.to_string())?;
        use said_forge::EditorAdapter as _;
        let adapter = said_forge::ClaudeAdapter;
        adapter
            .remove_skill(project_root, &sanitized)
            .map_err(|e| e.to_string())?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "tombstoned_frames": n,
                    "slug": sanitized,
                })
            );
        } else {
            println!(
                "tombstoned {} frames, removed .forge/{}/ and .claude/skills/{}/",
                n, sanitized, sanitized
            );
        }
        Ok(())
    }

    // â”€â”€â”€ helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn read_stories(sfb: &mut SaidFileBrain, hash: &str) -> Vec<Story> {
        use said_forge::frame::BrainIo;
        let mut out = Vec::new();
        let prefix = format!("forge:story:{}:", hash);
        let tags = sfb.iter_tags();
        let doc_ids: Vec<String> = tags
            .into_iter()
            .filter(|(_, tags, _)| tags.iter().any(|t| t.starts_with(&prefix)))
            .map(|(doc_id, _, _)| doc_id)
            .collect();
        for did in doc_ids {
            // Use find_body_by_tag? It needs a specific tag. Instead, walk
            // the matching tag and re-use find_body_by_tag per-tag.
            // Simpler: read the first matching tag's body.
            let tag = {
                let all = sfb.iter_tags();
                all.into_iter()
                    .find(|(d, _, _)| d == &did)
                    .and_then(|(_, tags, _)| {
                        tags.into_iter().find(|t| t.starts_with(&prefix))
                    })
            };
            if let Some(t) = tag {
                if let Some(body) = sfb.find_body_by_tag(&t) {
                    if let Ok(s) = serde_json::from_str::<Story>(&body) {
                        out.push(s);
                    }
                }
            }
        }
        out
    }
}
