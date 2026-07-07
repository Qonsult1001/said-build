//! said-mcp — MCP server for .said portable brain files.
//!
//! Exposes 5 facade tools to LLMs via the Model Context Protocol (stdio):
//!
//!   search  — find information (routes passkey/needle/short/long internally)
//!   get     — read exact frame content by doc_id
//!   ingest  — add files to the brain (PDF, DOCX, TXT, MD, video)
//!   remember — store a piece of text as a memory
//!   status  — brain health check (frame count, dream cycles, etc.)
//!
//! BOUNDARY ENFORCEMENT:
//!   When pointed at a module brain (e.g., card.vivere.said), the server
//!   auto-loads BOUNDARY.md from the same directory and injects it into
//!   the MCP instructions as an architectural constitution. The LLM is
//!   told which tables it owns exclusively and which require API contracts.
//!   Zero cognitive bleed — the LLM cannot see or hallucinate logic from
//!   other modules because the data literally doesn't exist in the brain.
//!
//! Run:
//!   said-mcp                              # stdio server (Claude Code connects here)
//!   said-mcp --path my_brain.said         # explicit .said file
//!   said-mcp --path card.vivere.said      # module brain — auto-enforces boundary

mod tools;
mod handler;

use rust_mcp_sdk::schema::{
    Implementation, InitializeResult, ProtocolVersion, ServerCapabilities,
    ServerCapabilitiesPrompts, ServerCapabilitiesTools,
};
use rust_mcp_sdk::{
    error::SdkResult,
    mcp_server::{server_runtime, McpServerOptions, ServerRuntime},
    McpServer, StdioTransport, ToMcpServerHandler, TransportOptions,
};
use std::sync::Arc;

use handler::SaidServerHandler;

#[tokio::main]
async fn main() -> SdkResult<()> {
    // Human-facing --help / --version. Without this, running the binary by hand just blocks on stdin
    // (it's an MCP stdio server), so a person had no way to see what it is or what tools it exposes.
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("said-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Parse --path flag (same as said-cli)
    let said_path = std::env::args()
        .skip_while(|a| a != "--path")
        .nth(1);

    // PROJECT SCOPE (auto): the engine reads SAID_PROJECT to fold the owning project into fix/blueprint
    // identity + tags (per-project isolation, opt-in cross-project reuse). The MCP server is the natural
    // place to set it from the brain filename, so an agent gets project scoping with zero extra config.
    // Caller-set SAID_PROJECT always wins. Derive from the brain stem: "vivere.said" -> "vivere",
    // "card.vivere.said" (module) -> "vivere" (last dotted segment before .said). Empty/unknown -> unset.
    if std::env::var("SAID_PROJECT").ok().filter(|s| !s.trim().is_empty()).is_none() {
        if let Some(ref p) = said_path {
            if let Some(stem) = std::path::Path::new(p).file_name().and_then(|n| n.to_str()) {
                let stem = stem.strip_suffix(".said").unwrap_or(stem);
                // module brains are "<module>.<project>.said" -> take the project (last segment).
                let project = stem.rsplit('.').next().unwrap_or(stem).trim();
                if !project.is_empty() && project != "said" {
                    std::env::set_var("SAID_PROJECT", project);
                }
            }
        }
    }

    // If this is a module brain (e.g., card.vivere.said), load BOUNDARY.md
    // from the same directory as the architectural constitution.
    let boundary = said_path.as_ref().and_then(|p| {
        let said_dir = std::path::Path::new(p).parent()?;
        let boundary_path = said_dir.join("BOUNDARY.md");
        if boundary_path.exists() {
            std::fs::read_to_string(&boundary_path).ok()
        } else {
            None
        }
    });

    let is_module = boundary.is_some();

    // Build MCP instructions: boundary constitution for modules, generic for monoliths
    let instructions = if let Some(ref boundary_content) = boundary {
        format!(
            "This server provides access to an ISOLATED MODULE brain. \
             You are working ONLY within this module's boundary.\n\n\
             ## ARCHITECTURAL CONSTITUTION\n\n\
             {}\n\n\
             ## RULES (NON-NEGOTIABLE)\n\n\
             1. You may ONLY write direct database code for tables listed under 'Exclusive Objects'.\n\
             2. For tables listed under 'Shared Hub Tables', you MUST generate interface contracts \
                (e.g., IAccountService, IClientService) — NEVER direct SQL or database calls.\n\
             3. Hidden triggers listed in the trigger report MUST be converted to domain events \
                in the application layer — do NOT rely on database triggers.\n\
             4. Use 'search' to find stored procedure and trigger implementation details.\n\
             5. Use 'get' to read exact frame content by doc_id.\n\
             6. Use 'status' to verify what the module brain contains.\n\
             7. NEVER generate code that directly accesses tables from other modules.",
            boundary_content
        )
    } else {
        // BRAIN (free, memory-only) INSTRUCTIONS — this bundle exposes ONLY the memory tools
        // (remember/ask/get/create/open/delete/history/checkout/status/list_concepts/admin), so the
        // nudge must reference ONLY those. It must NOT mention code/SQL/modules/search/snapshot/ingest
        // — those are paid tiers (Pro/Developer) not present here, and advertising them in the free
        // product both confuses the user and leaks the paywall.
        #[cfg(not(feature = "code"))]
        let caps = String::from(
            "This server provides access to a .said portable brain file — your personal, \
             single-file memory. It stores notes, facts, decisions, and preferences, and finds \
             them back by meaning when you ask in plain English.\n\n\
             ## HOW TO TALK TO USERS\n\n\
             Users are often non-technical. NEVER dump raw tool output. Every response must:\n\
             1. State what happened in plain English first (e.g. \"✓ Saved that to your brain.\").\n\
             2. Suggest the next 1-2 likely actions with copy-paste-ready phrasing.\n\
             3. If the user asks 'how do I…', give the exact tool call, not an abstract description.\n\n\
             ## ANTI-HALLUCINATION\n\n\
             Report the EXACT values the tool returned this turn. Never invent counts or status. \
             If a tool errors, say so — don't fabricate a next step.\n\n\
             ## MEMORY CONSULTATION (the brain is PRIMARY)\n\n\
             The brain is the user's persistent memory. **ALWAYS call `ask` first** when the question \
             contains any of: \"you/we/our/my\", memory cues (\"remember\", \"earlier\", \"last time\", \
             \"we decided\", \"told you\", \"saved\"), or \"what do I have on X / is there anything about X\". \
             If `ask` returns a relevant result, quote it and cite the id — never override a stored \
             memory with training knowledge. For purely general knowledge (\"how does async work?\"), \
             answer from training and skip the brain. If the brain is empty, say so; don't pretend to recall.\n\n\
             ## AUTO-MEMORY (act as the user's note-taker)\n\n\
             Chat turns are ephemeral — the brain only remembers what you save with `remember`. \
             Call `remember` whenever the user: makes a decision, states a preference/constraint, \
             shares a fact worth keeping, or asks you to remember (always confirm you did).\n\
             How: `remember content=\"<self-contained sentence>\" id=\"<short-slug>\"`. \
             Write content that will still make sense in 6 months without the surrounding chat. \
             After saving, tell the user briefly (\"✓ saved that\") and move on.\n\n\
             ## TOOLS (this is a memory brain)\n\
             - `remember` — save a note/fact/decision as a memory.\n\
             - `ask` — find memories by meaning, in plain English. The main command.\n\
             - `get` — read one memory's exact text by its id.\n\
             - `delete` — remove a memory (recoverable via `admin`).\n\
             - `history` / `checkout` — see or restore earlier versions of a memory.\n\
             - `status` — how many memories the brain holds.\n\
             - `list_concepts` — the topics your memories connect to.\n\
             - `create` / `open` — make or switch to a brain file.\n\
             - `admin` — recover deleted memories, manage retention.\n\n\
             ## FIRST CONTACT\n\
             If the brain is empty, call `prompts/get name=\"onboard\"` and paste the welcome. \
             Otherwise greet briefly and offer `ask`. Check `status` if unsure — never overwrite blindly.\n");

        // Build dynamic instructions based on compiled features
        #[cfg(feature = "code")]
        let mut caps = String::from(
            "This server provides access to a .said portable brain file — a single-file \
             searchable brain containing code, SQL schemas, and memories.\n\n\
             ## HOW TO TALK TO USERS (IMPORTANT — read before every response)\n\n\
             Users of this MCP server are often NON-TECHNICAL or unfamiliar with .said. \
             NEVER dump raw tool output at them. Every response must:\n\n\
             1. **State what happened in plain English first** (e.g. \"✓ Your brain is \
                attached and empty — ready to ingest code.\")\n\
             2. **Suggest the next 2-3 likely actions** with copy-paste-ready phrasing. \
                Never say \"you can now query it\" without listing exactly HOW.\n\
             3. **Translate numbers into meaning** — don't just say \"4957 frames\", say \
                \"4,957 pieces of code indexed — roughly 1,690 files, AST-chunked into \
                functions and classes for precise search.\"\n\
             4. **If the user asks 'what now?' or 'how do I…' ALWAYS include concrete \
                command examples** with the exact tool name and argument shape, NOT \
                abstract descriptions.\n\n\
             ## ANTI-HALLUCINATION RULES (CRITICAL — violations are reportable bugs)\n\n\
             When you report tool results to the user, you MUST use the EXACT values \
             returned by the tool in the current turn. Never:\n\n\
             - Substitute numbers from memory or prior runs (e.g. reporting \"189 files \
                copied\" when the current tool output says 0 or a different value)\n\
             - Paraphrase a failure as success (if the 'Ground-truth / DISK CHECK' block \
                says a folder is MISSING, the snapshot FAILED — tell the user that)\n\
             - Invent counts, paths, or status. If a field in the tool output is 0, report 0.\n\n\
             Tool responses now include a 'Ground-truth (verified on disk)' block for \
             operations that write files. Quote those numbers verbatim. If the disk \
             check contradicts the tool's own JSON counters, trust the disk check.\n\n\
             If a tool errors out, say so. Do not fabricate a next step as if it succeeded.\n\n\
             ## FIRST-CONTACT BEHAVIOUR\n\n\
             On the user's FIRST interaction in this session:\n\
             • If the attached brain is empty (0 frames), call `prompts/get name=\"onboard\"` \
                and paste the welcome text to guide the user through brain-naming + use case.\n\
             • If the brain is populated, greet briefly and show `overview` as a starting point.\n\
             • Always check status first if unsure — don't ingest or overwrite blindly.\n\n\
             ## SUGGESTED PHRASINGS FOR COMMON USER QUESTIONS\n\n\
             - \"How do I add files?\"       → use `init dir=\"<folder>\"` for a whole folder, \
                                              or `ingest path=\"<file>\"` for one file. Give the EXACT tool call.\n\
             - \"What's in this brain?\"     → call `overview` then summarise the top 5 modules.\n\
             - \"Find X\"                    → call `search query=\"X\"` and summarise top results \
                                              with file paths; offer `get doc_id=\"…\"` for exact content.\n\
             - \"Start over\"                → call `clean all=true` (warn them it deletes brain + sandboxes).\n\
             - \"Set up a test database\"    → `snapshot <module>` then `sandbox <module>`.\n\
             - \"Do you remember when…\" / \"Did I tell you about…\" \
                                              → call `search query=\"<their topic>\"` FIRST. The brain may \
                                              contain memories from prior sessions. Only say 'I don't know' \
                                              after confirming the search came back empty.\n\
             - \"I deleted file X, update my brain\" / \"forget about file X\" \
                                              → call `sync` (scans all frames, tombstones ones whose source \
                                              file is gone). For a specific frame use `delete doc_id=\"…\"`.\n\n\
             ## MEMORY CONSULTATION PROTOCOL (brain is PRIMARY, not secondary)\n\n\
             The brain is the user's persistent memory — an authoritative source of \
             truth for anything specific to them, their projects, their decisions, \
             their codebase. Treat it like a colleague's notebook that YOU share. \
             Query-classification rules:\n\n\
             **ALWAYS call `search` first (brain-primary) when the question contains ANY of:**\n\
             - 2nd-person/collective: \"you\", \"we\", \"our\", \"my\"\n\
             - Memory cues: \"remember\", \"earlier\", \"yesterday\", \"last time\", \
                \"before\", \"we decided\", \"we discussed\", \"told you\", \"saved\"\n\
             - Named entities you know are in this brain (modules, files, people, \
                project names — check `overview` or `status` if unsure)\n\
             - \"What do I have on X\", \"is there anything about X\"\n\n\
             If `search` returns a relevant result (score ≥ 0.3), quote it verbatim \
             and cite the doc_id. NEVER substitute your training knowledge over a \
             retrieved memory — the memory is newer and user-specific.\n\n\
             **Skip the brain (answer from training) for purely general knowledge:**\n\
             - \"How does async/await work in Rust?\"\n\
             - \"What's the syntax for a SQL CTE?\"\n\
             - \"Explain PostgreSQL MVCC\"\n\n\
             **Brain THEN training** (combined) for:\n\
             - Implementation questions about the user's code (\"how does OUR card \
                validation flow?\") — find relevant procs via `search`, then explain.\n\n\
             Edge cases:\n\
             - If the brain is empty (0 frames from `status`), say so and proceed \
                with training knowledge. Don't pretend to search.\n\
             - If `search` returns low-confidence results, still show them but flag \
                the uncertainty — never silently fall back to generic knowledge.\n\n\
             ## AUTO-MEMORY (IMPORTANT — the brain is not automatic)\n\n\
             Chat turns are ephemeral. The brain only remembers things you explicitly save \
             via the `remember` tool. You MUST act as the user's note-taker:\n\n\
             Call `remember` whenever any of these happen in the conversation:\n\n\
             1. **User makes a decision** — \"let's use port 1434\", \"we'll go with willie.said\", \
                \"don't extract billing, focus on card first\".\n\
             2. **User states a preference or constraint** — \"I need this for a demo Friday\", \
                \"we can't use docker on prod\", \"SQL Server 2019 only\".\n\
             3. **User shares domain knowledge** — \"FICA is South African KYC\", \
                \"our billing runs at 02:00 daily\", \"the card team owns these 62 procs\".\n\
             4. **You discovered something useful** — \"the vivere brain has 2,652 objects \
                across 20 detected modules\", \"snapshot writes to .said-code/<module>.<brain>/\".\n\
             5. **User asks you to remember** — always, and confirm you did.\n\n\
             How to call it: `remember content=\"<summary>\" title=\"<short label>\" id=\"mem/<date>/<slug>\"`\n\n\
             Guidelines:\n\
             - Write the content as a self-contained sentence. In 6 months, will it still \
                make sense without the surrounding chat? If not, add context.\n\
             - Use `id=\"mem/2026-04-20/sandbox-port-decision\"` — date + slug, hierarchical.\n\
             - Tag what it's about via `title` (e.g. \"sandbox port decision\", \"FICA domain note\").\n\
             - After saving, tell the user briefly (\"✓ saved that decision\") — don't interrupt the flow.\n\n\
             Before answering ANY question about past context (\"what did we do yesterday?\", \
             \"what's my setup?\", \"did we decide X?\"), FIRST call `search` on the brain. \
             The answer is probably already there.\n\n\
             ## CAPABILITIES\n\n\
             The brain may contain:\n\
             - Source code (Rust, Python, JS, TS, Go, Java, C# — AST-chunked by function/class)\n\
             - SQL/T-SQL (stored procedures, tables, triggers, views, functions — FK, CHECK, refs tags)\n\
             - Memories and notes stored by users or other LLMs\n");

        #[cfg(feature = "docs")]
        caps.push_str("- Documents (PDF, DOCX, TXT, MD — extracted and indexed)\n");

        #[cfg(feature = "ocr")]
        caps.push_str("- Scanned PDFs (OCR via PaddleOCR — image pages converted to text)\n");

        #[cfg(feature = "whisper")]
        caps.push_str("- Audio/video transcripts (MP4, MP3, WAV — speech-to-text)\n");

        #[cfg(feature = "code")]
        caps.push_str("\n## TOOLS\n\n\
             - 'open': Attach this MCP server to a different .said brain (creates it empty if missing). \
               Use this to pick your brain name without restarting Cursor.\n\
             - 'create': Create an empty .said brain file at a new path (use `open` after to attach).\n\
             - 'init': Bulk-ingest a directory into the current brain (SQL, code, docs).\n\
             - 'sync': Reconcile brain with disk — tombstone frames whose source file \
               was deleted. Call this when the user says they deleted a file, moved/renamed \
               files, or asks 'is my brain still in sync with my files?'.\n\
             - 'search': Semantic search across ALL content. Use deep=true for full narrative.\n\
             - 'get': Read exact frame content by doc_id.\n\
             - 'sym': Symbol lookup — proc/table/trigger/class by exact name (sub-millisecond).\n\
             - 'remember': Store text as searchable memory.\n\
             - 'delete': Remove memories by doc_id, age (older_than_days), or date. Supports dry_run.\n\
             - 'status': Brain health — frames, dream cycles, S_slow magnitude.\n\
             - 'history': Version timeline of any symbol (semantic git log).\n\
             - 'checkout': Restore past version of a symbol.\n\
             - 'discover': Auto-detect module boundaries in monolithic codebases.\n\
             - 'overview': Brain-derived product catalogue — lists every detected module \
               with counts and the exact name to pass to 'snapshot'. Pass `check` to probe \
               a specific term (comma-separated for batch).\n\
             - 'snapshot': Extract module into .said-code/<module>.<brain>/ folder + lens brain.\n\
             - 'sandbox': Spin up Docker SQL Server test DB. Defaults to one module on port 1433. \
               Pass `modules` to CO-DEPLOY more into the SAME DB (cross-module call testing). \
               Pass `port` to keep multiple sandboxes alive simultaneously. \
               Pass `label` for A/B compare (two sandboxes for same module, different versions).\n\
             - 'clean': Tear down sandboxes and delete generated files. Pass `targets` for specific \
               modules, `all=true` to wipe .said-code/ entirely, `dry_run=true` to preview.\n");

        #[cfg(feature = "docs")]
        caps.push_str("- 'ingest': Add files (PDF, DOCX, TXT, MD). Auto-detects format.\n");

        #[cfg(all(feature = "code", not(feature = "docs")))]
        caps.push_str("- 'ingest': Add text/code files. Document support (PDF, DOCX) not compiled in.\n");

        #[cfg(feature = "whisper")]
        caps.push_str("- 'ingest': Also supports video/audio transcription (MP4, MP3, WAV).\n");

        // Code-locate steering: tell the agent to reach for `.said` BEFORE grepping the codebase.
        // This is the TRUSTED channel — the model CALLS `ask` (vs distrusting injected hook context).
        #[cfg(feature = "code")]
        caps.push_str("\n## LOCATING CODE (use `.said` BEFORE grep)\n\n\
             When you need to LOCATE something in this project's code — a function by what it DOES \
             (not its exact name), the source of a bug from a symptom, or a past fix — call `ask` \
             FIRST, before grepping or reading files. `.said` finds it by MEANING (semantic + symbol \
             + call-graph) and points at the precise file + symbol far cheaper than reading the \
             codebase; grep can't match a symptom that shares no identifier with the buggy line. \
             Act on what `ask` returns (hand symbols to your LSP for type-precise references). If \
             `ask` returns nothing relevant, then grep normally — it never invents results.\n\n\
             ## CODING MEMORY (learn_fix — store a verified fix the way the orchestrator does)\n\n\
             When you SOLVE a coding problem and a real build/test gate is GREEN, store it with \
             `learn_fix` so a future session (you, the CLI, or the orchestrator) reloads it instead \
             of re-deriving. Write the SAME structured iteration note the orchestrator stores — NOT a \
             one-line label (a thin note gets out-ranked by the source it summarizes). Capture:\n\
             - the PROBLEM solved (plain words — this is the recall key);\n\
             - the FILES/functions touched and why;\n\
             - ERRORS + corrections — approaches that FAILED, so they are never retried;\n\
             - the non-obvious INVARIANT a textbook version gets wrong (the highest-value field);\n\
             - the KEY RESULT — plus the verified change-set (the `edits` that built+passed).\n\
             ONLY after the gate is green — `success` is the sole recorded outcome. This writes the \
             SAME store as `said learn-fix` and the orchestrator (one shared learning store). To get the \
             exact 10-section note template, call `prompts/get name=\"fix-template\"` (or `said \
             fix-template` on the CLI) and fill it in.\n\n\
             ## CROSS-LANGUAGE BRIDGING\n\n\
             The brain links SQL and application code semantically. A search for 'card validation' \
             returns BOTH the SQL stored procedure AND the C# service that calls it.\n\n\
             ## DATA RETENTION\n\n\
             All memories stored forever by default. Use 'delete' with older_than_days or \
             before_date for enterprise retention policies (GDPR, SOX). Deleted frames are \
             tombstoned (preserved in history, removed from search).");

        caps
    };

    let description = if is_module {
        "ISOLATED MODULE brain — boundary-enforced, no cognitive bleed. \
         Only module-exclusive objects are directly accessible."
    } else {
        "Portable brain files — search, ingest, remember. \
         One file, all memory. MTEB WikimQA 1.0, Needle 1.0."
    };

    let server_details = InitializeResult {
        server_info: Implementation {
            name: "said-mcp".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: Some(if is_module {
                "SAID Module Brain (Boundary Enforced)".into()
            } else {
                "SAID Brain MCP Server".into()
            }),
            description: Some(description.into()),
            icons: vec![],
            website_url: None,
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            // Advertise prompts so clients (Cursor / Claude Desktop) surface
            // the `onboard` prompt in their prompt-picker UI.
            prompts: Some(ServerCapabilitiesPrompts { list_changed: None }),
            ..Default::default()
        },
        meta: None,
        instructions: Some(instructions.into()),
        protocol_version: ProtocolVersion::V2025_11_25.into(),
    };

    let transport = StdioTransport::new(TransportOptions::default())?;
    let handler = SaidServerHandler::new(said_path);

    let server: Arc<ServerRuntime> = server_runtime::create_server(McpServerOptions {
        server_details,
        transport,
        handler: handler.to_mcp_server_handler(),
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    if let Err(e) = server.start().await {
        eprintln!("said-mcp error: {}", e);
    }
    Ok(())
}

/// Human-facing help for `said-mcp --help`. This is an MCP stdio server (an AI agent normally spawns
/// it and reads its tools over the protocol), but a person running it by hand deserves to see what it
/// is and what it offers — mirroring the `said` CLI's command menu.
fn print_help() {
    #[cfg(not(feature = "code"))]
    let body = "\
said-mcp — portable brain MCP server (Personal / Free tier)

A single-file personal memory, served over MCP so an AI agent (Claude, Cursor, …) can read and write
it for you. Offline, no cloud, no LLM inside — the agent is the LLM.

USAGE:
    said-mcp --path <brain.said>     Start the server on a brain file (an agent connects over stdio)
    said-mcp --help                  Show this help
    said-mcp --version               Show the version

    The agent steering is built in: on connect the server tells the agent to recall with `ask`
    before answering and save with `remember`. Nothing to paste or configure.

TOOLS THE AGENT CAN CALL (memory-only — this is the free tier):
    ask             Find memories by meaning (the main recall command)
    remember        Save a note / fact / decision / preference as a memory
    get             Read one memory's exact text by its id
    delete          Remove a memory (recoverable from the recycle bin)
    list_concepts   List the [[wikilink]] concepts your memories are linked to
    history         Show the version history of a memory
    checkout        Restore an earlier version of a memory
    status          How many memories the brain holds + its health
    open            Attach the server to a different .said brain file
    create          Create a new, empty brain file
    admin           Recover deleted memories and manage retention

CONNECT (example MCP client config):
    {
      \"mcpServers\": {
        \"said-brain\": {
          \"command\": \"said-mcp\",
          \"args\": [\"--path\", \"my-brain.said\"]
        }
      }
    }

For the terminal equivalent, use the `said` CLI (`said --help`). Same brain file, either way.";

    #[cfg(feature = "code")]
    let body = "\
said-mcp — .said MCP server

USAGE:
    said-mcp --path <brain.said>     Start the server on a brain file (an agent connects over stdio)
    said-mcp --help                  Show this help
    said-mcp --version               Show the version

An AI agent connects over MCP stdio and calls the tools this build advertises (see the agent's
tools/list). For the terminal equivalent use the `said` CLI (`said --help`).";

    println!("{body}");
}
