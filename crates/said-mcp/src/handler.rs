//! MCP server handler â€” routes tool calls to SaidFile methods.

use crate::tools::*;
#[allow(unused_imports)]
use async_trait::async_trait;
use rust_mcp_sdk::{
    mcp_server::ServerHandler,
    schema::{
        schema_utils::CallToolError, CallToolRequestParams, CallToolResult,
        ContentBlock, GetPromptRequestParams, GetPromptResult, ListPromptsResult,
        ListToolsResult, PaginatedRequestParams, Prompt, PromptMessage, Role,
        RpcError, TextContent,
    },
    McpServer,
};
use sca_core::said_file::SaidFile;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct SaidServerHandler {
    brain: Arc<Mutex<SaidFile>>,
    /// The path of the currently-attached brain. Mutable so the `open` tool
    /// can switch to a different file at runtime without a server restart.
    said_path: Mutex<String>,
    /// Brains this MCP server created empty and that remain unused. Tracked
    /// so that when `open` switches to a named brain, we can remove the
    /// auto-created placeholder without asking the user. Any brain the user
    /// actually populates (init / remember / ingest) is removed from this set
    /// so it won't be auto-deleted.
    ephemeral_brains: Mutex<std::collections::HashSet<String>>,
    /// If this is a lens file, the lens metadata for filtering
    lens: Option<sca_core::lens::LensFile>,
}

// =========================================================================
// LSP helper functions â€” used by handle_lsp_def / handle_lsp_refs etc.
// Mirror the CLI helpers in said-cli/src/main.rs so behaviour is identical.
// =========================================================================

/// Whitespace-tolerant SQL keyword-pair matcher. Returns true if `haystack`
/// contains `head` followed by `tail` separated only by ASCII whitespace
/// (spaces, tabs, newlines), optionally with `OR ALTER` between them.
/// `haystack` is expected uppercase. Catches dialect quirks like
/// `CREATE   PROCEDURE` (multi-space), `CREATE\nFUNCTION` (newline-separated),
/// and `CREATE OR ALTER PROCEDURE` in a single check.
fn contains_sql_keyword_pair(haystack: &str, head: &str, tail: &str) -> bool {
    let bytes = haystack.as_bytes();
    let head_b = head.as_bytes();
    let tail_b = tail.as_bytes();
    let or_alter = b"OR ALTER";
    let mut i = 0;
    while i + head_b.len() <= bytes.len() {
        if &bytes[i..i + head_b.len()] == head_b {
            let mut j = i + head_b.len();
            // require at least one whitespace separator
            if j >= bytes.len() || !bytes[j].is_ascii_whitespace() {
                i += 1;
                continue;
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() { j += 1; }
            // optional `OR ALTER` interjection
            if j + or_alter.len() <= bytes.len() && &bytes[j..j + or_alter.len()] == or_alter {
                let k = j + or_alter.len();
                if k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    j = k;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() { j += 1; }
                }
            }
            if j + tail_b.len() <= bytes.len() && &bytes[j..j + tail_b.len()] == tail_b {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(feature = "lsp")]
fn parse_lsp_location(location: &str) -> Result<(String, u32, u32), CallToolError> {
    // Split from the right to handle paths with colons (e.g. C:\...)
    let parts: Vec<&str> = location.rsplitn(3, ':').collect();
    if parts.len() < 3 {
        return Err(CallToolError::from_message(format!(
            "Invalid location '{}'. Expected file:line:col",
            location
        )));
    }
    let col: u32 = parts[0].parse().map_err(|_| {
        CallToolError::from_message(format!("Invalid column: '{}'", parts[0]))
    })?;
    let line: u32 = parts[1].parse().map_err(|_| {
        CallToolError::from_message(format!("Invalid line: '{}'", parts[1]))
    })?;
    let file = parts[2].to_string();
    Ok((file, line, col))
}

#[cfg(feature = "lsp")]
fn detect_lsp_server(file_path: &str) -> &'static str {
    let ext = std::path::Path::new(file_path)
        .extension()
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

/// Build the JSON error payload for an edit failure. If `msg` is itself a JSON
/// object (a structured error from `compute_edit` carrying `valid_anchors`),
/// merge `ok:false` into it so the repair menu surfaces at the top level.
/// Otherwise wrap the plain string as `{ ok:false, error: msg }`.
fn edit_error_payload(msg: &str) -> serde_json::Value {
    if let Ok(serde_json::Value::Object(mut m)) = serde_json::from_str::<serde_json::Value>(msg) {
        if m.contains_key("error") {
            m.insert("ok".into(), serde_json::Value::Bool(false));
            return serde_json::Value::Object(m);
        }
    }
    serde_json::json!({ "ok": false, "error": msg })
}

impl SaidServerHandler {
    /// Snapshot the currently-attached brain path. Cloning a short string
    /// under lock is cheap and releases the mutex immediately.
    fn current_path(&self) -> String {
        self.said_path.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Return the directory containing the currently-attached brain, as an
    /// absolute path when possible. This is the CWD we set on every `said`
    /// subprocess so relative paths (like `.said-code/...`) land next to the
    /// brain â€” NOT wherever Cursor happened to spawn the MCP server (which
    /// is typically `C:\Users\<user>\` on Windows).
    ///
    /// Without this, subprocess writes go to MCP's inherited CWD and the
    /// user sees snapshot tools report success while the output folder
    /// silently appears under their home directory.
    fn brain_dir(&self) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(self.current_path());
        let parent = p.parent().map(|x| x.to_path_buf())
            .filter(|x| !x.as_os_str().is_empty());
        let dir = match parent {
            Some(d) => d,
            None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        };
        // Make absolute WITHOUT canonicalize â€” canonicalize on Windows returns
        // `\\?\C:\...` extended-length paths which confuse downstream string
        // concatenation with "/schema.sql" etc. (mixed / + \ separators, plus
        // some tools don't support \\?\). Join against cwd ourselves.
        if dir.is_absolute() {
            dir
        } else {
            std::env::current_dir()
                .map(|c| c.join(&dir))
                .unwrap_or(dir)
        }
    }

    /// Translate a path the subprocess will write to (usually `.said-code/...`)
    /// into its absolute equivalent based on `brain_dir()`. Used to verify
    /// output on disk after a subprocess completes, regardless of what the
    /// subprocess reported as its relative path.
    fn resolve_in_brain_dir(&self, rel: &str) -> std::path::PathBuf {
        let rp = std::path::PathBuf::from(rel);
        if rp.is_absolute() { return rp; }
        self.brain_dir().join(rp)
    }

    /// Normalize two file paths for same-file comparison (handles mixed
    /// separators and relative vs absolute). Returns true if both refer to
    /// the same filesystem object.
    fn same_file(a: &str, b: &str) -> bool {
        let pa = PathBuf::from(a);
        let pb = PathBuf::from(b);
        let ca = std::fs::canonicalize(&pa).unwrap_or(pa);
        let cb = std::fs::canonicalize(&pb).unwrap_or(pb);
        ca == cb
    }

    /// Text for the `onboard` prompt. Detects whether the attached brain
    /// is still the ephemeral placeholder and tailors the greeting.
    fn onboarding_text(current: &str) -> String {
        let is_placeholder = current.ends_with(".brain.said")
            || Self::is_pristine_brain(current);
        let base = std::path::Path::new(current)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(current);

        if is_placeholder {
            format!(
                "# Welcome to .said\n\
                 \n\
                 You're connected to the **.said** MCP server. Right now it's \
                 attached to `{base}` â€” a temporary placeholder.\n\
                 \n\
                 ## Quick start (2 questions)\n\
                 \n\
                 **1. Pick a name for your brain.** It's one file that holds \
                 everything â€” code, SQL, documents, memories. What should it \
                 be called?\n\
                 \n\
                 Suggestions:\n\
                 - `willie.said` â€” personal / single-project brain\n\
                 - `acme.said` â€” one brain per client\n\
                 - `vivere.said` â€” name it after the codebase you're indexing\n\
                 \n\
                 **2. What do you want to use it for?**\n\
                 \n\
                 - **Portable** â€” notes, journaling, research (start with \
                 `remember` and `search`)\n\
                 - **Enterprise** â€” legacy monolith modernization (start with \
                 `init` on your SQL/code folder, then `overview` + `snapshot`)\n\
                 \n\
                 ## Once you tell me the name, I'll:\n\
                 \n\
                 1. Call `open path=\"<your-name>.said\"` â€” creates the brain \
                 and cleans up the placeholder\n\
                 2. If enterprise: run `init dir=\"<path>\"` to ingest, then \
                 `overview` to show you what's inside\n\
                 3. If portable: hand you the `remember` / `search` commands\n\
                 \n\
                 Type: **\"Use `willie.said` and it's portable\"** or \
                 **\"Create `vivere.said` and init from `G:\\work\\sql`\"** \
                 â€” or anything natural. I'll map it to the right tools."
            )
        } else {
            format!(
                "# Welcome back to .said\n\
                 \n\
                 Attached to: **{base}**\n\
                 \n\
                 Common commands from here:\n\
                 - `status` â€” health + frame count\n\
                 - `overview` â€” what products/modules are in this brain\n\
                 - `search <term>` â€” semantic search\n\
                 - `snapshot <module>` â€” extract a module workspace\n\
                 - `sandbox <module>` â€” spin up a Docker SQL Server test DB\n\
                 - `open <name>.said` â€” switch to a different brain (this \
                 one is kept)\n\
                 \n\
                 Tell me what you're trying to do and I'll pick the right tool."
            )
        }
    }

    /// If `path` refers to a pristine/placeholder brain AND exactly one
    /// populated `.said` lives next to it, return the populated one. Otherwise
    /// return `path` unchanged. Never picks a populated brain when multiple
    /// exist (that would be an ambiguous decision â€” user must pick via `open`).
    fn auto_promote_if_placeholder(path: &str) -> String {
        let p = PathBuf::from(path);
        let file_name = p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let is_reserved_placeholder = file_name == ".brain.said";

        // Only promote when the target is reserved OR pristine/empty. If it's
        // populated OR missing-with-a-real-name, leave it alone â€” the user
        // picked that name on purpose.
        let should_promote = is_reserved_placeholder
            || (p.exists() && Self::is_pristine_brain(path));
        if !should_promote { return path.to_string(); }

        // Scan the parent directory for populated .said files.
        let parent = p.parent().map(|x| x.to_path_buf())
            .filter(|x| !x.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        let mut populated: Vec<(PathBuf, u64)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&parent) {
            for entry in rd.flatten() {
                let ep = entry.path();
                if !ep.is_file() { continue; }
                if ep == p { continue; }
                // Skip dot-prefixed files (`.brain.said` and friends).
                let name = match ep.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                if name.starts_with('.') { continue; }
                if !name.ends_with(".said") { continue; }

                let size = std::fs::metadata(&ep).map(|m| m.len()).unwrap_or(0);
                if size > 32_000 {
                    // Verify it's actually openable (avoid picking corrupt files).
                    if SaidFile::open(&ep).is_ok() {
                        populated.push((ep, size));
                    }
                }
            }
        }

        if populated.len() == 1 {
            return populated[0].0.to_string_lossy().to_string();
        }
        // 0 populated â†’ stay on placeholder (nothing to promote to).
        // >1 populated â†’ ambiguous; user must pick via `open`.
        path.to_string()
    }

    /// Returns populated `.said` files (>32KB, openable, not dot-prefixed) in
    /// the same directory as `path`. Used by `status` to nudge the user when
    /// they're on an empty placeholder but real brains exist alongside.
    fn list_populated_sibling_brains(path: &str) -> Vec<(String, u64)> {
        let p = PathBuf::from(path);
        let parent = p.parent().map(|x| x.to_path_buf())
            .filter(|x| !x.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        let mut found: Vec<(String, u64)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&parent) {
            for entry in rd.flatten() {
                let ep = entry.path();
                if !ep.is_file() { continue; }
                if ep == p { continue; }
                let name = match ep.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                if name.starts_with('.') { continue; }
                if !name.ends_with(".said") { continue; }
                let size = std::fs::metadata(&ep).map(|m| m.len()).unwrap_or(0);
                if size > 32_000 && SaidFile::open(&ep).is_ok() {
                    found.push((name.to_string(), size));
                }
            }
        }
        found.sort_by(|a, b| b.1.cmp(&a.1));  // largest first
        found
    }

    /// Is a brain file "pristine" â€” exists, the small size of a freshly
    /// created (empty) .said, zero frames? Safe to delete without data loss.
    fn is_pristine_brain(path: &str) -> bool {
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => return false,
        };
        if size == 0 || size > 32_000 {
            // zero-byte or populated â€” in neither case is it safe to silently delete.
            return false;
        }
        // Try to open and check frame count.
        match SaidFile::open(path) {
            Ok(b) => b.frames.active_count() == 0,
            Err(_) => false,
        }
    }
}

impl SaidServerHandler {
    pub fn new(explicit_path: Option<String>) -> Self {
        let requested = explicit_path.unwrap_or_else(|| Self::auto_detect_said());

        // Auto-promotion: if the requested path is a PLACEHOLDER (either the
        // reserved name `.brain.said` or an empty brain that looks pristine),
        // AND there's exactly one populated brain in the same directory, quietly
        // attach to that instead. This fixes the common trap where Cursor is
        // configured to start MCP at `.brain.said` but the user already has
        // `vivere.said` / `willie.said` full of real content sitting next to it.
        let path = Self::auto_promote_if_placeholder(&requested);
        if path != requested {
            eprintln!(
                "[brain] Auto-promoted from placeholder '{}' to populated brain '{}'",
                requested, path
            );
        }

        // Track whether the file existed before we tried to open it â€” if not,
        // `open_brain` will create it empty, and we flag it as ephemeral so
        // `open` can clean it up later when the user picks a real brain name.
        let existed_before = Path::new(&path).exists();
        let mut ephemeral: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Check if this is a lens file â€” if so, open the PARENT brain
        if sca_core::lens::LensFile::is_lens(&path) {
            match sca_core::lens::LensFile::open(&path) {
                Ok(lens) => {
                    let parent_path = lens.resolve_parent();
                    let parent_str = parent_path.to_string_lossy().to_string();
                    let brain = Self::open_brain(&parent_str);
                    eprintln!("[lens] Opened parent brain: {} (module: {}, {} frames)",
                        parent_str, lens.module_name, lens.frame_ids.len());
                    return Self {
                        brain: Arc::new(Mutex::new(brain)),
                        said_path: Mutex::new(path),
                        ephemeral_brains: Mutex::new(ephemeral),
                        lens: Some(lens),
                    };
                }
                Err(e) => {
                    eprintln!("[lens] Failed to open lens: {}, falling back to direct open", e);
                }
            }
        }

        let brain = Self::open_brain(&path);
        // If we just auto-created the startup brain (nothing populated it),
        // mark it ephemeral so the first `open` call can tidy it up.
        if !existed_before && Self::is_pristine_brain(&path) {
            ephemeral.insert(path.clone());
        }
        Self {
            brain: Arc::new(Mutex::new(brain)),
            said_path: Mutex::new(path),
            ephemeral_brains: Mutex::new(ephemeral),
            lens: None,
        }
    }

    fn auto_detect_said() -> String {
        // Same logic as said-cli: find single .said in cwd, or use config default
        if let Ok(entries) = std::fs::read_dir(".") {
            let said_files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "said").unwrap_or(false))
                .collect();
            if said_files.len() == 1 {
                return said_files[0].to_string_lossy().to_string();
            }
        }
        // Fallback: create a default brain
        "brain.said".to_string()
    }

    fn open_brain(path: &str) -> SaidFile {
        // Helper â€” load the SCA encoder: embedded model (baked in via the
        // `embed-model` feature) first, then known install paths. Without the
        // embedded fallback, the read path opened a brain with no encoder, so
        // search/ask could not encode the query and SCA semantic ranking was
        // dead (all results scored 0.000). Mirrors the CLI's try_load_encoder.
        fn attach_encoder(brain: &mut SaidFile) {
            if brain.auto_load_encoder() {
                return;
            }
            for p in &[
                "said-lam-static",
                "../said-lam-static",
                "SAID-LAM-private/said-lam-static",
            ] {
                if Path::new(p).exists() {
                    let _ = brain.load_encoder(p);
                    break;
                }
            }
        }

        if Path::new(path).exists() {
            match SaidFile::open(path) {
                Ok(mut brain) => {
                    attach_encoder(&mut brain);
                    brain
                }
                Err(e) => {
                    // File exists but couldn't be opened (bad CRC, corrupt,
                    // tmp-rename crash). Keep the existing file and return an
                    // in-memory empty brain pointing at the same path â€” the
                    // user can inspect/recover manually.
                    eprintln!("[brain] Could not open {}: {}. Using in-memory empty brain.", path, e);
                    SaidFile::create(path)
                }
            }
        } else {
            // No file yet â€” create an empty brain AND persist it to disk so
            // subsequent tools can read from a real file. Without the save
            // call the file stays unwritten until someone calls init/remember,
            // which breaks any tool that checks file size.
            eprintln!("[brain] {} does not exist â€” creating empty brain on disk.", path);
            let mut brain = SaidFile::create(path);
            if let Err(e) = brain.save() {
                eprintln!("[brain] WARNING: could not save empty brain: {}", e);
            }
            attach_encoder(&mut brain);
            brain
        }
    }
}

#[async_trait]
impl ServerHandler for SaidServerHandler {
    async fn handle_list_tools_request(
        &self,
        _params: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            meta: None,
            next_cursor: None,
            tools: SaidTools::tools(),
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<CallToolResult, CallToolError> {
        let tool = SaidTools::try_from(params).map_err(CallToolError::new)?;

        match tool {
            SaidTools::SearchTool(t) => self.handle_search(t),
            SaidTools::AskTool(t) => self.handle_ask(t),
            SaidTools::GetTool(t) => self.handle_get(t),
            SaidTools::IngestTool(t) => self.handle_ingest(t),
            SaidTools::RememberTool(t) => self.handle_remember(t),
            SaidTools::StatusTool(_) => self.handle_status(),
            SaidTools::SymTool(t) => self.handle_sym(t),
            SaidTools::HistoryTool(t) => self.handle_history(t),
            SaidTools::CheckoutTool(t) => self.handle_checkout(t),
            SaidTools::EditTool(t) => self.handle_edit(t),
            SaidTools::EditBatchTool(t) => self.handle_edit_batch(t),
            SaidTools::DeleteTool(t) => self.handle_delete(t),
            SaidTools::DiscoverTool(_) => self.handle_discover(),
            SaidTools::OpenTool(t) => self.handle_open(t),
            SaidTools::CreateTool(t) => self.handle_create(t),
            SaidTools::InitTool(t) => self.handle_init(t),
            SaidTools::SyncTool(t) => self.handle_sync(t),
            SaidTools::JournalTool(t) => self.handle_journal(t),
            SaidTools::OverviewTool(t) => self.handle_overview(t),
            SaidTools::SnapshotTool(t) => self.handle_snapshot(t),
            SaidTools::SandboxTool(t) => self.handle_sandbox(t),
            SaidTools::CleanTool(t) => self.handle_clean(t),
            SaidTools::SessionEndTool(t) => self.handle_session_end(t),
            SaidTools::ToolCompletionTool(t) => self.handle_tool_completion(t),
            SaidTools::SalienceTool(t) => self.handle_salience(t),
            SaidTools::DreamTool(t) => self.handle_dream(t),
            SaidTools::AdminTool(t) => self.handle_admin(t),
            SaidTools::LspDefTool(t) => self.handle_lsp_def(t),
            SaidTools::LspRefsTool(t) => self.handle_lsp_refs(t),
            SaidTools::LspHoverTool(t) => self.handle_lsp_hover(t),
            SaidTools::LspSymbolsTool(t) => self.handle_lsp_symbols(t),
            SaidTools::RecallFixTool(t) => self.handle_recall_fix(t),
            SaidTools::LearnFixTool(t) => self.handle_learn_fix(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgeListTool(t) => self.handle_forge_list(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgeGetTool(t) => self.handle_forge_get(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgeStatusTool(t) => self.handle_forge_status(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgeLoadTool(t) => self.handle_forge_load(t).await,
            #[cfg(feature = "forge")]
            SaidTools::ForgeRunTool(t) => self.handle_forge_run(t).await,
            #[cfg(feature = "forge")]
            SaidTools::ForgeResetTool(t) => self.handle_forge_reset(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgeInitTool(t) => self.handle_forge_init(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgePlanQuestionsTool(t) => self.handle_forge_plan_questions(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgePlanApplyTool(t) => self.handle_forge_plan_apply(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgeSyncTool(t) => self.handle_forge_sync(t),
            #[cfg(feature = "forge")]
            SaidTools::ForgeGapsTool(t) => self.handle_forge_gaps(t),
        }
    }

    // â”€â”€ MCP prompts â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Two first-class prompts:
    //   `onboard`  â€” guided setup for first-time MCP attach.
    //   `answerer` â€” the canonical .said agent system prompt, sourced
    //                from the `said-prompts` crate so MCP clients
    //                (Claude Desktop, Cursor, etc.) load the same
    //                instructions the WASM browser agent uses.
    async fn handle_list_prompts_request(
        &self,
        _params: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListPromptsResult, RpcError> {
        Ok(ListPromptsResult {
            meta: None,
            next_cursor: None,
            prompts: vec![
                Prompt {
                    name: "onboard".to_string(),
                    title: Some("Welcome â€” Quick Start".to_string()),
                    description: Some(
                        "A short guided setup for .said: pick a brain name, \
                         ingest your codebase, then explore modules. Use this \
                         when attaching the MCP server for the first time."
                            .to_string(),
                    ),
                    arguments: vec![],
                    icons: vec![],
                    meta: None,
                },
                Prompt {
                    name: "answerer".to_string(),
                    title: Some(".said Answerer Agent".to_string()),
                    description: Some(
                        "Canonical system prompt for the .said agent â€” reads \
                         brain content and answers with citations. Aligned \
                         with Anthropic Claude Code production prompts. \
                         Single source of truth (said-prompts crate)."
                            .to_string(),
                    ),
                    arguments: vec![],
                    icons: vec![],
                    meta: None,
                },
            ],
        })
    }

    async fn handle_get_prompt_request(
        &self,
        params: GetPromptRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<GetPromptResult, RpcError> {
        match params.name.as_str() {
            "onboard" => Ok(GetPromptResult {
                description: Some(
                    "Guided onboarding for the .said MCP server".to_string(),
                ),
                meta: None,
                messages: vec![PromptMessage {
                    role: Role::Assistant,
                    content: ContentBlock::TextContent(
                        TextContent::new(
                            Self::onboarding_text(&self.current_path()),
                            None,
                            None,
                        ),
                    ),
                }],
            }),
            "answerer" => {
                // Pull live brain context: file list + total active memories.
                // The handler holds the current brain path; deriving full
                // multi-brain context here would require additional state,
                // so we report the single open brain (or empty).
                let path_str = self.current_path();
                let files: Vec<String> = if path_str.is_empty() {
                    Vec::new()
                } else {
                    std::path::Path::new(&path_str)
                        .file_name()
                        .map(|f| vec![f.to_string_lossy().into_owned()])
                        .unwrap_or_default()
                };
                let ctx = said_prompts::agents::answerer::Context {
                    files,
                    total_docs: 0,
                };
                let prompt = said_prompts::assemble(said_prompts::Role::Answerer, &ctx);
                Ok(GetPromptResult {
                    description: Some(
                        "Canonical .said answerer system prompt".to_string(),
                    ),
                    meta: None,
                    messages: vec![PromptMessage {
                        role: Role::Assistant,
                        content: ContentBlock::TextContent(
                            TextContent::new(prompt, None, None),
                        ),
                    }],
                })
            }
            other => Err(RpcError::invalid_params()
                .with_message(format!("Unknown prompt: '{}'", other))),
        }
    }
}

impl SaidServerHandler {
    fn handle_search(&self, t: SearchTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        let top_k = if t.deep.unwrap_or(false) { 500 } else { 50 };
        // Decision 2 â€” parse optional pillar filter. Unknown names are ignored;
        // if nothing valid was passed we treat it as "no filter" (all pillars).
        let pillar_set: Option<std::collections::HashSet<sca_core::frames::Pillar>> =
            t.pillar.as_ref().and_then(|raw| {
                let mut set = std::collections::HashSet::new();
                for name in raw.split(',') {
                    let token = name.trim().to_lowercase();
                    let p = match token.as_str() {
                        "episodic" => Some(sca_core::frames::Pillar::Episodic),
                        "semantic" => Some(sca_core::frames::Pillar::Semantic),
                        "procedural" => Some(sca_core::frames::Pillar::Procedural),
                        "external" => Some(sca_core::frames::Pillar::External),
                        "code" => Some(sca_core::frames::Pillar::Code),
                        "memory" => Some(sca_core::frames::Pillar::Memory),
                        _ => None,
                    };
                    if let Some(p) = p { set.insert(p); }
                }
                if set.is_empty() { None } else { Some(set) }
            });

        let all_results = brain.recall_by_pillar(&t.query, top_k, pillar_set.as_ref());

        // If lens is active, filter results to only module frames
        // Uses the module:tag on frames in the parent brain (set during snapshot)
        let results: Vec<_> = if let Some(ref lens) = self.lens {
            let tag = &lens.filter_tag; // "module:card"
            all_results.into_iter()
                .filter(|r| {
                    // Check if this frame has the module tag in the parent brain
                    brain.frames.get_meta(&r.doc_id)
                        .map(|m| m.tags.iter().any(|t| t == tag))
                        .unwrap_or(false)
                    // Fallback: check lens frame_ids set
                    || lens.contains(&r.doc_id)
                })
                .take(if t.deep.unwrap_or(false) { 100 } else { 10 })
                .collect()
        } else {
            all_results.into_iter()
                .take(if t.deep.unwrap_or(false) { 100 } else { 10 })
                .collect()
        };

        if results.is_empty() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("No results found for: {}", t.query),
            )]));
        }

        let mut output = String::new();
        for (i, r) in results.iter().enumerate() {
            output.push_str(&format!(
                "{}. [score={:.3}] {}\n{}\n\n",
                i + 1,
                r.score,
                r.doc_id,
                r.content.chars().take(500).collect::<String>()
            ));
        }

        // Auto-dream now fires inside recall_by_pillar (core) — no manual trigger
        // here. We still persist the brain state it evolved.
        let _ = brain.save_brain_only();

        Ok(CallToolResult::text_content(vec![TextContent::from(output)]))
    }

    fn handle_ask(&self, t: AskTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let top = t.top.map(|v| v as usize).unwrap_or(10);
        let deep = t.deep.unwrap_or(false);

        // Tag-scope detection: same logic as CLI cmd_ask. Let a query like
        // "version 4 ..." narrow the candidate pool to frames tagged version:4.
        let scope_doc_ids: Option<std::collections::HashSet<String>> =
            if let Some((ns, val)) = sca_core::recall::detect_scope_tag(&t.query) {
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
            } else { None };

        // THE SHARED CALL â€” same function the CLI uses. CLI and MCP return
        // byte-identical result sets (modulo formatting) for any query.
        let (kept, keywords) = sca_core::ask::ask(
            &mut brain, &t.query, top, deep, scope_doc_ids.as_ref(),
        );

        if kept.is_empty() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("Ask: \"{}\"  (no confident match from sym/grep/SCA engines)", t.query),
            )]));
        }

        let mut output = format!(
            "Ask: \"{}\"  ({} results, keywords: {})\n\n",
            t.query, kept.len(), keywords.join(", "),
        );
        for (i, r) in kept.iter().enumerate() {
            let loc = r.location.as_deref().unwrap_or("");
            let preview: String = r.content.chars().take(500).collect();
            output.push_str(&format!(
                "{}. [{:.2}][{}] {} {}\n{}\n\n",
                i + 1,
                r.confidence,
                r.kind,
                r.doc_id,
                if loc.is_empty() { String::new() } else { format!("({})", loc) },
                preview,
            ));
        }

        // Auto-dream now fires inside sca_core::ask::ask (core) — no duplicate trigger
        // here. We still persist the brain-state it evolved.
        let _ = brain.save_brain_only();

        Ok(CallToolResult::text_content(vec![TextContent::from(output)]))
    }

    fn handle_get(&self, t: GetTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        match brain.get(&t.doc_id) {
            Some(content) => Ok(CallToolResult::text_content(vec![TextContent::from(content)])),
            None => Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("Document not found: {}", t.doc_id),
            )])),
        }
    }

    #[allow(unused_mut)]
    fn handle_ingest(&self, t: IngestTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Enterprise pointer mode â€” no content extraction, no blob. Each file
        // becomes a searchable External frame holding only URI + summary.
        // Same shape as CLI `said ingest --pointer`; see remember_as_external_pointer.
        if t.pointer.unwrap_or(false) {
            use std::path::Path;
            let path = Path::new(&t.path);
            if !path.exists() {
                return Err(CallToolError::from_message(format!("not found: {}", t.path)));
            }

            // Build the list of files to register. Walk dirs shallow â€” no
            // gitignore filter at MCP level (the CLI's recursive walk is the
            // full-feature entry point; MCP gets the simpler file-by-file API).
            let files: Vec<std::path::PathBuf> = if path.is_file() {
                vec![path.to_path_buf()]
            } else if path.is_dir() {
                let mut out = Vec::new();
                if let Ok(entries) = std::fs::read_dir(path) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.is_file() { out.push(p); }
                    }
                }
                out
            } else {
                return Err(CallToolError::from_message(
                    format!("not a file or directory: {}", t.path)));
            };

            let mut n_frames = 0usize;
            for f in &files {
                let uri = f.canonicalize().ok()
                    .map(|p| format!("file://{}", p.display().to_string().replace('\\', "/")))
                    .unwrap_or_else(|| f.display().to_string());
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                let mime = f.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase());
                let summary_text = t.summary.clone().unwrap_or_else(|| {
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

            brain.build_index().map_err(|e| CallToolError::from_message(e))?;
            brain.save().map_err(|e| CallToolError::from_message(e))?;
            drop(brain);
            self.mark_populated();

            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!(
                    "Pointer ingest complete.\nMode: enterprise (no blobs embedded)\nFiles: {}\nFrames: {}",
                    files.len(), n_frames,
                ),
            )]));
        }

        // Mode guard: Enterprise brains refuse content-embedding ingests.
        // Callers must set pointer=true (handled above) or switch to portable.
        brain.ensure_content_ingest_allowed()
            .map_err(|e| CallToolError::from_message(e))?;

        #[cfg(feature = "docs")]
        {
            let path = t.path.clone();
            let report = sca_core::document_ingest::ingest_document(
                &mut brain,
                &path,
                |_, _, _| {},
            ).map_err(|e| CallToolError::from_message(e))?;

            let _ = brain.build_index();
            brain.compact();
            brain.save().map_err(|e| CallToolError::from_message(e))?;
            drop(brain);
            self.mark_populated();

            Ok(CallToolResult::text_content(vec![TextContent::from(
                format!(
                    "Ingested: {}\nFormat: {}\nSegments: {}\nFrames stored: {}",
                    report.source_path,
                    report.format,
                    report.segments_extracted,
                    report.frames_stored,
                ),
            )]))
        }

        #[cfg(not(feature = "docs"))]
        {
            Err(CallToolError::from_message(
                "Document ingestion disabled â€” rebuild said-mcp with --features docs".to_string(),
            ))
        }
    }

    fn handle_remember(&self, t: RememberTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Decision 3 â€” parse optional pillar; default to Episodic. Unknown
        // pillar names fall back silently to Episodic (same spirit as
        // Decision 2's search tool).
        let pillar = t.pillar.as_deref().map(str::trim).map(str::to_lowercase);
        let pillar_enum = match pillar.as_deref() {
            Some("semantic") => sca_core::frames::Pillar::Semantic,
            Some("procedural") => sca_core::frames::Pillar::Procedural,
            Some("external") => sca_core::frames::Pillar::External,
            Some("code") => sca_core::frames::Pillar::Code,
            Some("memory") => sca_core::frames::Pillar::Memory,
            // Both None (legacy call) and Some("episodic") route here.
            _ => sca_core::frames::Pillar::Episodic,
        };

        let extra_tags = t.tags.unwrap_or_default();

        // Route through `remember_with_salience` so every MCP `remember` call
        // gets both Decision 4's salience scoring AND step 8's Surprise /
        // reconsolidation tagging. Lexical correction markers + semantic
        // contradiction are detected automatically; no new tool parameter
        // needed. The salience.tags + surprise tags flow through into
        // FrameMeta.tags alongside any caller-provided extras.
        let (frame_id, scored) = brain.remember_with_salience(
            t.id.as_deref(),
            &t.content,
            t.title.as_deref(),
            pillar_enum,
            extra_tags,
        );

        let _ = brain.build_index();
        brain.save().map_err(|e| CallToolError::from_message(e))?;
        let frame_count = brain.frames.active_count();
        // Pull tags off the just-written frame so the response can surface
        // surprise detection to the caller (agents using MCP can react to
        // contradiction events immediately).
        let doc_id_written = brain.frames.get_all_frames()
            .iter().rev().find(|m| m.id == frame_id)
            .map(|m| m.doc_id.clone());
        let written_tags: Vec<String> = doc_id_written.as_ref()
            .and_then(|d| brain.frames.get_meta(d))
            .map(|m| m.tags.clone())
            .unwrap_or_default();
        drop(brain);
        // Brain now has real content â€” retire any ephemeral tag so `open`
        // doesn't delete it later.
        self.mark_populated();

        let pillar_label = match pillar_enum {
            sca_core::frames::Pillar::Episodic => "episodic",
            sca_core::frames::Pillar::Semantic => "semantic",
            sca_core::frames::Pillar::Procedural => "procedural",
            sca_core::frames::Pillar::External => "external",
            sca_core::frames::Pillar::Code => "code",
            sca_core::frames::Pillar::Memory => "memory",
            sca_core::frames::Pillar::Document => "document",
        };

        // Build a one-line Surprise / salience note. Silent on benign frames
        // so the default response stays short; flags contradictions + updates
        // so the caller's agent can react ("I noticed you're overriding a
        // prior fact â€” preserved both as a conflict").
        let mut notes: Vec<String> = Vec::new();
        notes.push(format!("salience={} ({})", scored.score, scored.band.tag()));
        if written_tags.iter().any(|t| t == "reconsolidation:contradicts") {
            let prior = written_tags.iter()
                .find_map(|t| t.strip_prefix("contradicts:"))
                .unwrap_or("unknown");
            notes.push(format!("âš  contradicts prior frame `{}`", prior));
        } else if written_tags.iter().any(|t| t == "reconsolidation:update") {
            let prior = written_tags.iter()
                .find_map(|t| t.strip_prefix("updates:"))
                .unwrap_or("unknown");
            notes.push(format!("â†» updates a prior memory `{}`", prior));
        }
        let notes_line = notes.join(" Â· ");

        Ok(CallToolResult::text_content(vec![TextContent::from(
            format!(
                "âœ“ Saved to brain (memory #{}, pillar={}). {}\n\nBrain now has {} memories. \
                 This memory is searchable â€” future `search` calls can find it.",
                frame_id, pillar_label, notes_line, frame_count
            ),
        )]))
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Decision 3 â€” episodic writer hooks
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn handle_session_end(&self, t: SessionEndTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        let mut tags: Vec<String> = t.tags.unwrap_or_default();
        tags.push("event:session_end".to_string());
        if let Some(sid) = t.session_id.as_deref() {
            let s = sid.trim();
            if !s.is_empty() {
                tags.push(format!("session:{}", s));
            }
        }

        // Build a stable, readable doc_id so later `history` / `search` can
        // find the episode marker. If the caller supplied a session_id we
        // use it verbatim; otherwise the helper auto-generates `ep_N`.
        let doc_id_owned = t.session_id.as_ref().map(|sid| {
            let s = sid.trim();
            if s.is_empty() {
                None
            } else {
                Some(format!("ep_session_{}", s))
            }
        }).flatten();

        let title = Some("session_end");

        let frame_id = brain.remember_with_pillar(
            doc_id_owned.as_deref(),
            &t.summary,
            title,
            sca_core::frames::Pillar::Episodic,
            tags,
        );

        let _ = brain.build_index();
        brain.save().map_err(|e| CallToolError::from_message(e))?;
        let frame_count = brain.frames.active_count();
        drop(brain);
        self.mark_populated();

        Ok(CallToolResult::text_content(vec![TextContent::from(
            format!(
                "âœ“ session_end logged (memory #{}). Brain has {} memories. \
                 Dream consolidation will pick this up at next cycle.",
                frame_id, frame_count
            ),
        )]))
    }

    fn handle_tool_completion(
        &self,
        t: ToolCompletionTool,
    ) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        let mut tags: Vec<String> = t.tags.unwrap_or_default();
        tags.push("event:tool_completion".to_string());
        tags.push(format!("tool:{}", t.tool.trim()));
        if let Some(st) = t.status.as_deref() {
            let s = st.trim().to_lowercase();
            if !s.is_empty() {
                tags.push(format!("status:{}", s));
            }
        }

        // Body: tool name + args summary (if any) + result. Keeps the frame
        // self-describing for the dream-function procedural distillation.
        let mut body = String::new();
        body.push_str(&format!("tool: {}\n", t.tool.trim()));
        if let Some(args) = t.args_summary.as_deref() {
            let trimmed = args.trim();
            if !trimmed.is_empty() {
                body.push_str(&format!("args: {}\n", trimmed));
            }
        }
        if let Some(st) = t.status.as_deref() {
            body.push_str(&format!("status: {}\n", st.trim()));
        }
        body.push_str("result: ");
        body.push_str(&t.result);

        let title = Some(format!("tool_completion:{}", t.tool.trim()));

        let frame_id = brain.remember_with_pillar(
            None,
            &body,
            title.as_deref(),
            sca_core::frames::Pillar::Episodic,
            tags,
        );

        let _ = brain.build_index();
        brain.save().map_err(|e| CallToolError::from_message(e))?;
        let frame_count = brain.frames.active_count();
        drop(brain);
        self.mark_populated();

        Ok(CallToolResult::text_content(vec![TextContent::from(
            format!(
                "âœ“ tool_completion logged (memory #{}). Brain has {} memories. \
                 Recurring tool+args+result patterns will distil to Procedural \
                 pillar at next dream cycle.",
                frame_id, frame_count
            ),
        )]))
    }

    fn handle_salience(&self, t: SalienceTool) -> Result<CallToolResult, CallToolError> {
        // Read-only â€” no brain lock needed, pure scoring. But we still want
        // the pillar parse to match the write-path's conventions.
        let pillar_enum = match t.pillar.as_deref().map(str::trim).map(str::to_lowercase).as_deref() {
            Some("semantic") => sca_core::frames::Pillar::Semantic,
            Some("procedural") => sca_core::frames::Pillar::Procedural,
            Some("external") => sca_core::frames::Pillar::External,
            Some("code") => sca_core::frames::Pillar::Code,
            Some("memory") => sca_core::frames::Pillar::Memory,
            _ => sca_core::frames::Pillar::Episodic,
        };

        let scored = sca_core::salience::score_turn(&t.content, pillar_enum);
        let recommendation = match scored.band {
            sca_core::salience::SalienceBand::Low =>
                "Recommendation: do NOT call `remember`. Likely chit-chat or a bare question.",
            sca_core::salience::SalienceBand::Medium =>
                "Recommendation: `remember` if context warrants; normal retrieval weight.",
            sca_core::salience::SalienceBand::High =>
                "Recommendation: `remember` with this pillar. High reconsolidation value â€” preserve.",
        };

        let tag_list = scored.tags.join(", ");
        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "salience={} band={} tags=[{}]\n{}",
            scored.score,
            scored.band.tag(),
            tag_list,
            recommendation
        ))]))
    }

    fn handle_dream(&self, t: DreamTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Build DreamParams from tool args (all optional with sane defaults).
        let mut params = sca_core::dream::DreamParams::default();
        if let Some(th) = t.cluster_threshold {
            // Clamp to [0.0, 1.0] â€” nonsense values should fall back.
            if th.is_finite() && (0.0..=1.0).contains(&th) {
                params.cluster_threshold = th;
            }
        }
        if let Some(mn) = t.min_cluster_size {
            if mn >= 2 {
                params.min_cluster_size = mn as usize;
            }
        }
        if let Some(mc) = t.max_candidates {
            if mc > 0 {
                params.max_candidates = mc as usize;
            }
        }

        // Cycle number â€” caller pin, or derived from existing count of
        // `dream_cycle:*` Semantic frames (an idempotent fallback â€” no need
        // to persist a counter in BRAN for v1).
        let cycle = t.cycle.unwrap_or_else(|| {
            let mut max_seen: u32 = 0;
            for f in brain.frames.get_all_frames() {
                for tag in &f.tags {
                    if let Some(rest) = tag.strip_prefix("dream_cycle:") {
                        if let Ok(n) = rest.parse::<u32>() {
                            if n > max_seen {
                                max_seen = n;
                            }
                        }
                    }
                }
            }
            max_seen + 1
        });

        let report = brain.run_dream_content(cycle, &params);

        let _ = brain.build_index();
        brain.save().map_err(|e| CallToolError::from_message(e))?;
        drop(brain);
        self.mark_populated();

        let created_preview: String = if report.semantic_frames_created.is_empty() {
            "none".to_string()
        } else {
            report
                .semantic_frames_created
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
                + if report.semantic_frames_created.len() > 5 {
                    ", ..."
                } else {
                    ""
                }
        };

        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "âœ“ dream cycle {} complete ({} ms)\n\
             candidates_examined   = {}\n\
             clusters_formed       = {}\n\
             semantic_frames       = {}\n\
             episodic_frames_tagged = {}\n\
             created: {}",
            report.cycle,
            report.elapsed_ms,
            report.candidates,
            report.clusters_formed,
            report.semantic_frames_created.len(),
            report.frames_marked,
            created_preview,
        ))]))
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // LSP tools â€” wraps SaidFile::lsp_definition / lsp_references / lsp_hover
    // / lsp_workspace_symbol. Results are cached as frames in the brain so
    // future `ask_fused` retrieval finds them without a fresh LSP roundtrip.
    //
    // Gated on `feature = "lsp"`. When the feature is off, the handler
    // returns a structured error instead of failing the whole MCP server.
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    fn handle_lsp_def(&self, t: LspDefTool) -> Result<CallToolResult, CallToolError> {
        #[cfg(feature = "lsp")]
        {
            let (file, line, col) = parse_lsp_location(&t.location)?;
            let mut brain = self.brain.lock().map_err(|e| {
                CallToolError::from_message(format!("brain lock: {}", e))
            })?;
            let server = detect_lsp_server(&file);
            brain.enable_lsp(server, ".").map_err(CallToolError::from_message)?;
            let result = brain.lsp_definition(&file, line, col).map_err(CallToolError::from_message)?;
            brain.save().map_err(CallToolError::from_message)?;
            let body = if result.is_empty() {
                format!("No definition found for {}", t.location)
            } else { result };
            Ok(CallToolResult::text_content(vec![TextContent::from(body)]))
        }
        #[cfg(not(feature = "lsp"))]
        {
            let _ = t;
            Ok(CallToolResult::text_content(vec![TextContent::from(
                "lsp_def: said-mcp was built without the `lsp` feature. \
                 Rebuild with `cargo build -p said-mcp --features lsp` to enable.".to_string()
            )]))
        }
    }

    fn handle_lsp_refs(&self, t: LspRefsTool) -> Result<CallToolResult, CallToolError> {
        #[cfg(feature = "lsp")]
        {
            let (file, line, col) = parse_lsp_location(&t.location)?;
            let mut brain = self.brain.lock().map_err(|e| {
                CallToolError::from_message(format!("brain lock: {}", e))
            })?;
            let server = detect_lsp_server(&file);
            brain.enable_lsp(server, ".").map_err(CallToolError::from_message)?;
            let result = brain.lsp_references(&file, line, col).map_err(CallToolError::from_message)?;
            brain.save().map_err(CallToolError::from_message)?;
            let body = if result.is_empty() {
                format!("No references found for {}", t.location)
            } else { result };
            Ok(CallToolResult::text_content(vec![TextContent::from(body)]))
        }
        #[cfg(not(feature = "lsp"))]
        {
            let _ = t;
            Ok(CallToolResult::text_content(vec![TextContent::from(
                "lsp_refs: said-mcp was built without the `lsp` feature.".to_string()
            )]))
        }
    }

    fn handle_lsp_hover(&self, t: LspHoverTool) -> Result<CallToolResult, CallToolError> {
        #[cfg(feature = "lsp")]
        {
            let (file, line, col) = parse_lsp_location(&t.location)?;
            let mut brain = self.brain.lock().map_err(|e| {
                CallToolError::from_message(format!("brain lock: {}", e))
            })?;
            let server = detect_lsp_server(&file);
            brain.enable_lsp(server, ".").map_err(CallToolError::from_message)?;
            let result = brain.lsp_hover(&file, line, col).map_err(CallToolError::from_message)?;
            let body = if result.is_empty() {
                format!("No hover info for {}", t.location)
            } else { result };
            Ok(CallToolResult::text_content(vec![TextContent::from(body)]))
        }
        #[cfg(not(feature = "lsp"))]
        {
            let _ = t;
            Ok(CallToolResult::text_content(vec![TextContent::from(
                "lsp_hover: said-mcp was built without the `lsp` feature.".to_string()
            )]))
        }
    }

    fn handle_lsp_symbols(&self, t: LspSymbolsTool) -> Result<CallToolResult, CallToolError> {
        #[cfg(feature = "lsp")]
        {
            let mut brain = self.brain.lock().map_err(|e| {
                CallToolError::from_message(format!("brain lock: {}", e))
            })?;
            // No file to detect from â€” default to rust-analyzer (workspace-level
            // search). Caller can override via env var SCA_LSP_SERVER.
            let server = std::env::var("SCA_LSP_SERVER").ok();
            let server_str = server.as_deref().unwrap_or("rust-analyzer");
            brain.enable_lsp(server_str, ".").map_err(CallToolError::from_message)?;
            let result = brain.lsp_workspace_symbol(&t.query).map_err(CallToolError::from_message)?;
            brain.save().map_err(CallToolError::from_message)?;
            let body = if result.is_empty() {
                format!("No symbols found for '{}'", t.query)
            } else { result };
            Ok(CallToolResult::text_content(vec![TextContent::from(body)]))
        }
        #[cfg(not(feature = "lsp"))]
        {
            let _ = t;
            Ok(CallToolResult::text_content(vec![TextContent::from(
                "lsp_symbols: said-mcp was built without the `lsp` feature.".to_string()
            )]))
        }
    }

    // ── CODING MEMORY ───────────────────────────────────────────────────────
    // recall_fix / learn_fix call the SAME sca_core helpers as the CLI and the
    // orchestrator, so the MCP agent shares ONE learning store with them.

    fn handle_recall_fix(&self, t: RecallFixTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let min = t.min_score.unwrap_or(0.45);
        match sca_core::ask::recall_coding_fix(&mut brain, &t.problem, min) {
            Some(hit) => {
                let label = brain.frames.get_meta(&hit.doc_id)
                    .and_then(|m| m.tags.iter().find(|t| t.starts_with("pr:")).cloned())
                    .unwrap_or_else(|| "-".into());
                let body = format!(
                    "Fix ({:.2}) {}  provenance={}\n\n{}\n\n## Verified change-set\n{}",
                    hit.score, hit.doc_id, label, hit.note, hit.edits_json,
                );
                Ok(CallToolResult::text_content(vec![TextContent::from(body)]))
            }
            None => Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                "No known fix above score {:.2} for \"{}\" — drive your own LLM, then store \
                 the verified result with learn_fix.",
                min, t.problem,
            ))])),
        }
    }

    fn handle_learn_fix(&self, t: LearnFixTool) -> Result<CallToolResult, CallToolError> {
        // Validate the change-set is JSON before storing (same guard as the CLI).
        let edits = t.edits.trim_start_matches('\u{feff}').trim();
        if serde_json::from_str::<serde_json::Value>(edits).is_err() {
            return Err(CallToolError::from_message(
                "learn_fix: `edits` is not valid JSON (expected the change-set array)".to_string(),
            ));
        }
        // Assemble the human note from the optional fields (FILES/ERRORS/LEARNINGS).
        let mut note = String::new();
        if let Some(f) = t.files.as_deref() { if !f.trim().is_empty() { note.push_str(&format!("FILES: {}\n", f.trim())); } }
        if let Some(e) = t.errors.as_deref() { if !e.trim().is_empty() { note.push_str(&format!("ERRORS: {}\n", e.trim())); } }
        if let Some(l) = t.learnings.as_deref() { if !l.trim().is_empty() { note.push_str(&format!("LEARNINGS: {}\n", l.trim())); } }
        note.push_str("RESULT: success — built+passed");

        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        // The ONE shared writer: blake3 id, native Procedural pillar, byte-identical
        // to `said learn-fix` and said-orchestration::learn.
        let doc_id = sca_core::ask::learn_coding_fix(
            &mut brain, &t.problem, &note, edits, t.label.as_deref(),
        );
        brain.save().map_err(CallToolError::from_message)?;
        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "Learned fix {} (provenance: {}). Stored in the Procedural pillar; future \
             recall_fix / orchestrator runs can reuse it.",
            doc_id, t.label.as_deref().unwrap_or("-"),
        ))]))
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Admin tool â€” parity with `said admin <action>` CLI subcommand family.
    // Same semantics: legal holds block retention sweeps, restores demote the
    // current Active head, who-deleted surfaces attribution tags. Shipped as
    // a single tool with an `action` field so agents don't have to memorize a
    // separate tool name per operation.
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    fn handle_admin(&self, t: AdminTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        let action = t.action.trim().to_lowercase();
        match action.as_str() {
            "list-tombstones" | "list" => {
                let records = brain.admin_tombstones();
                let filter = t.like.as_deref().map(str::to_lowercase);
                let rows: Vec<&sca_core::frames::FrameMeta> = records.into_iter()
                    .filter(|m| match &filter {
                        Some(n) => m.doc_id.to_lowercase().contains(n.as_str()),
                        None => true,
                    })
                    .collect();
                if rows.is_empty() {
                    return Ok(CallToolResult::text_content(vec![TextContent::from(
                        "No tombstoned frames.".to_string(),
                    )]));
                }
                let mut out = format!("Tombstoned frames ({}):\n", rows.len());
                for m in rows {
                    let hold: Vec<&str> = m.tags.iter()
                        .filter(|t| t.starts_with("legal_hold:"))
                        .map(|s| s.as_str()).collect();
                    let hold_s = if hold.is_empty() { String::new() }
                        else { format!(" [{}]", hold.join(",")) };
                    let superseded = m.superseded_by
                        .map(|id| format!(" superseded_by=#{}", id))
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "  {} (frame #{}, {} bytes, created_at={}){}{}\n",
                        m.doc_id, m.id, m.uncompressed_len, m.created_at, superseded, hold_s,
                    ));
                }
                Ok(CallToolResult::text_content(vec![TextContent::from(out)]))
            }
            "restore" => {
                let doc_id = t.doc_id.as_deref().ok_or_else(||
                    CallToolError::from_message("restore requires `doc_id`".to_string()))?;
                let (restored, displaced) = brain.admin_restore(doc_id)
                    .map_err(|e| CallToolError::from_message(e))?;
                brain.save().map_err(|e| CallToolError::from_message(e))?;
                let extra = displaced
                    .map(|id| format!("\n  Previous active head (frame #{}) demoted to tombstone.", id))
                    .unwrap_or_default();
                Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                    "âœ“ Restored doc_id '{}' as frame #{}.{}",
                    doc_id, restored, extra,
                ))]))
            }
            "who-deleted" | "lineage" => {
                let doc_id = t.doc_id.as_deref().ok_or_else(||
                    CallToolError::from_message("who-deleted requires `doc_id`".to_string()))?;
                let lineage = brain.lineage(doc_id);
                if lineage.is_empty() {
                    return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                        "No frames found for doc_id '{}'.", doc_id,
                    ))]));
                }
                let mut out = format!("Deletion / lineage trail for '{}':\n", doc_id);
                for m in lineage {
                    let status = match m.status {
                        sca_core::frames::FrameStatus::Tombstone => "tombstoned",
                        sca_core::frames::FrameStatus::Deleted => "deleted",
                        sca_core::frames::FrameStatus::Active => "active",
                    };
                    let superseded = m.superseded_by
                        .map(|id| format!(" â†’ superseded by #{}", id))
                        .unwrap_or_default();
                    let attrib: String = m.tags.iter()
                        .filter(|s| s.starts_with("user_id:") || s.starts_with("session:")
                            || s.starts_with("deleted_by:") || s.starts_with("actor:"))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" ");
                    let attrib = if attrib.is_empty() { String::new() }
                        else { format!("  [{}]", attrib) };
                    out.push_str(&format!(
                        "  #{} [{}] created_at={}{}{}\n",
                        m.id, status, m.created_at, superseded, attrib,
                    ));
                }
                Ok(CallToolResult::text_content(vec![TextContent::from(out)]))
            }
            "legal-hold-add" | "hold-add" => {
                let doc_id = t.doc_id.as_deref().ok_or_else(||
                    CallToolError::from_message("legal-hold-add requires `doc_id`".to_string()))?;
                let case = t.case.as_deref().ok_or_else(||
                    CallToolError::from_message("legal-hold-add requires `case`".to_string()))?;
                let n = brain.admin_legal_hold_add(doc_id, case);
                brain.save().map_err(|e| CallToolError::from_message(e))?;
                Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                    "âœ“ Placed legal hold '{}' on {} frame(s) for doc_id '{}'.",
                    case, n, doc_id,
                ))]))
            }
            "legal-hold-release" | "hold-release" => {
                let doc_id = t.doc_id.as_deref().ok_or_else(||
                    CallToolError::from_message("legal-hold-release requires `doc_id`".to_string()))?;
                let case = t.case.as_deref().ok_or_else(||
                    CallToolError::from_message("legal-hold-release requires `case`".to_string()))?;
                let n = brain.admin_legal_hold_release(doc_id, case);
                brain.save().map_err(|e| CallToolError::from_message(e))?;
                Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                    "âœ“ Released legal hold '{}' from {} frame(s) for doc_id '{}'.",
                    case, n, doc_id,
                ))]))
            }
            "audit" | "audit-log" => {
                let log = brain.audit();
                match log.verify() {
                    Ok(()) => {},
                    Err(e) => return Err(CallToolError::from_message(
                        format!("âœ— AUDT chain broken: {}", e))),
                }
                let filter_actor = t.doc_id.as_deref();  // reuse doc_id field
                let filter_kind = t.case.as_deref();     // reuse case field
                let entries: Vec<&sca_core::audit::AuditEntry> = log.entries().iter()
                    .filter(|e| filter_actor.map(|n| e.actor.contains(n)).unwrap_or(true))
                    .filter(|e| filter_kind.map(|k| e.kind == k).unwrap_or(true))
                    .collect();
                let mut out = format!("Audit log ({} of {} entries, chain verified):\n",
                    entries.len(), log.len());
                for e in entries.iter().take(200) {
                    out.push_str(&format!(
                        "  #{} [{}] {} actor={} target={} {}\n",
                        e.seq, e.timestamp, e.kind, e.actor, e.target, e.detail,
                    ));
                }
                if entries.len() > 200 {
                    out.push_str(&format!("  ... ({} more entries)\n", entries.len() - 200));
                }
                return Ok(CallToolResult::text_content(vec![TextContent::from(out)]));
            }
            "retention-sweep" | "sweep" => {
                let older_than_days = t.older_than_days.unwrap_or(365);
                let keep_per_doc = t.keep_per_doc.unwrap_or(1) as usize;
                let cutoff_ts = {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs()).unwrap_or(0);
                    now.saturating_sub(older_than_days.saturating_mul(86_400))
                };
                let records: Vec<(String, u64, u64)> = brain.admin_tombstones().iter()
                    .filter(|m| m.status == sca_core::frames::FrameStatus::Tombstone)
                    .filter(|m| !m.tags.iter().any(|t| t.starts_with("legal_hold:") || t.as_str() == "legal_hold"))
                    .filter(|m| m.created_at < cutoff_ts)
                    .map(|m| (m.doc_id.clone(), m.created_at, m.id))
                    .collect();
                use std::collections::HashMap;
                let mut by_doc: HashMap<String, Vec<(u64, u64)>> = HashMap::new();
                for (d, ts, fid) in records { by_doc.entry(d).or_default().push((ts, fid)); }
                let mut dropped = 0usize;
                for (_did, mut v) in by_doc {
                    v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
                    for (_ts, fid) in v.into_iter().skip(keep_per_doc) {
                        if brain.mark_frame_deleted(fid) { dropped += 1; }
                    }
                }
                brain.save().map_err(|e| CallToolError::from_message(e))?;
                Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                    "âœ“ Retention sweep dropped {} tombstones older than {} days (kept {} per doc).\n\
                     Legal holds honored â€” no held frame was touched.\n\
                     Run `compact` to physically reclaim the freed bytes.",
                    dropped, older_than_days, keep_per_doc,
                ))]))
            }
            other => Err(CallToolError::from_message(format!(
                "Unknown admin action '{}'. Valid: list-tombstones, restore, \
                 who-deleted, legal-hold-add, legal-hold-release, retention-sweep.",
                other,
            ))),
        }
    }

    fn handle_sync(&self, t: SyncTool) -> Result<CallToolResult, CallToolError> {
        let dry = t.dry_run.unwrap_or(false);
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Collect every active frame that carries a `source:<path>` tag.
        // Sources can come from `init` (the ingested folder's root) or from
        // `ingest` (the file path). We care about file-level tombstoning, so
        // we look for tag patterns that reference a concrete file OR walk the
        // doc_id if it looks like a relative path under a known source root.
        let doc_ids: Vec<String> = brain.frames.active_doc_ids().iter().map(|s| s.to_string()).collect();

        // Build map: file_part â†’ doc_ids_for_that_file
        // A doc_id like "sqlMasterGccGlobal/src/â€¦/foo.sql::NAME::kind:line"
        // has file_part = everything before the first "::".
        let mut file_to_dids: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        let mut source_roots: std::collections::HashSet<String> = std::collections::HashSet::new();

        for did in &doc_ids {
            let file_part = if let Some(pos) = did.find("::") {
                did[..pos].to_string()
            } else { did.clone() };
            file_to_dids.entry(file_part).or_default().push(did.clone());

            // Harvest source: tags from any frame to know which roots to resolve
            // relative file_parts against.
            if let Some(meta) = brain.frames.get_meta(did) {
                for tag in &meta.tags {
                    if let Some(root) = tag.strip_prefix("source:") {
                        source_roots.insert(root.to_string());
                    }
                }
            }
        }

        // For each unique file_part, decide if the source still exists.
        // A file_part is "resolved" if we find it at:
        //   1. the absolute path itself (file_part is already absolute), or
        //   2. <root>/<file_part> for any known source root.
        let mut orphaned_files: Vec<String> = Vec::new();
        let mut live_count = 0usize;
        for file_part in file_to_dids.keys() {
            let as_path = std::path::PathBuf::from(file_part);
            let exists = if as_path.is_absolute() {
                as_path.is_file()
            } else {
                source_roots.iter().any(|root| {
                    std::path::PathBuf::from(root).join(file_part).is_file()
                })
            };
            if exists { live_count += 1; } else {
                orphaned_files.push(file_part.clone());
            }
        }
        orphaned_files.sort();

        let orphaned_frame_count: usize = orphaned_files.iter()
            .map(|f| file_to_dids.get(f).map(|v| v.len()).unwrap_or(0))
            .sum();

        let mut tombstoned = 0usize;
        if !dry {
            for file_part in &orphaned_files {
                if let Some(dids) = file_to_dids.get(file_part) {
                    for did in dids {
                        if brain.tombstone_frame(did) {
                            tombstoned += 1;
                        }
                    }
                }
            }
            if tombstoned > 0 {
                brain.save().map_err(|e| CallToolError::from_message(e))?;
            }
        }

        let total_files = file_to_dids.len();

        // Build user-friendly report
        let preview: Vec<String> = orphaned_files.iter().take(10)
            .map(|f| format!("  â€¢ {}", f)).collect();
        let preview_text = if preview.is_empty() {
            "  (none)".to_string()
        } else {
            let extra = if orphaned_files.len() > 10 {
                format!("\n  ... and {} more", orphaned_files.len() - 10)
            } else { String::new() };
            format!("{}{}", preview.join("\n"), extra)
        };

        let mode = if dry { "(dry-run â€” nothing changed)" } else { "(executed)" };
        let action = if dry {
            "would tombstone".to_string()
        } else {
            format!("tombstoned {} frames across", tombstoned)
        };

        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
"âœ“ Sync complete {}

Scanned:              {} source files referenced by {} active frames
Sources still on disk: {}
Sources that are gone: {}  ({} frames affected)

Orphaned sources:
{}

Action: {} {} orphaned source files.

Tip: orphaned frames are TOMBSTONED, not hard-deleted. They're removed from
search results but preserved in the lineage history â€” reachable via the
`history` tool if you ever need to see what was there. To purge tombstones
permanently, run `said compact --drop-history --all` from a terminal.",
            mode, total_files, doc_ids.len(),
            live_count,
            orphaned_files.len(), orphaned_frame_count,
            preview_text,
            action, orphaned_files.len()
        ))]))
    }

    fn handle_journal(&self, t: JournalTool) -> Result<CallToolResult, CallToolError> {
        // Predictable doc_id: mem/YYYY-MM-DD/<topic>. Date is machine-local
        // but consistent with user's timezone (they're in front of the tool).
        let date = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let secs = SystemTime::now().duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0);
            // Cheap manual YYYY-MM-DD (days since epoch, no chrono dep needed).
            // Good enough for doc_id uniqueness; civil-date precision is fine.
            let days = secs / 86400;
            let (y, m, d) = days_to_ymd(days as i64);
            format!("{:04}-{:02}-{:02}", y, m, d)
        };

        // Sanitize topic so it's a safe path component.
        let topic: String = t.topic.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect();
        let topic = topic.trim_matches('-').to_string();
        let topic = if topic.is_empty() { "session".to_string() } else { topic };

        let doc_id = format!("mem/{}/{}", date, topic);
        let title = format!("Journal: {} ({})", t.topic, date);

        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let frame_id = brain.remember_as(&doc_id, &t.summary, Some(&title));
        brain.add_tag(&doc_id, "kind:journal");
        brain.add_tag(&doc_id, &format!("date:{}", date));
        let _ = brain.build_index();
        brain.save().map_err(|e| CallToolError::from_message(e))?;
        drop(brain);
        self.mark_populated();

        Ok(CallToolResult::text_content(vec![TextContent::from(
            format!(
                "âœ“ Journal entry saved.\n\n\
                 Doc ID:   {}\n\
                 Title:    {}\n\
                 Frame:    #{}\n\
                 Tags:     kind:journal, date:{}\n\
                 \n\
                 Find it again later with:\n\
                 â€¢ search query=\"{}\"\n\
                 â€¢ get doc_id=\"{}\"",
                doc_id, title, frame_id, date, t.topic, doc_id
            ),
        )]))
    }

    fn handle_status(&self) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Detect drift between in-memory state and on-disk file. Something
        // (a `create` call, an external `said init`, a manual rm) may have
        // rewritten the file since we last mmap'd it. If the on-disk size
        // disagrees with what we hold, reload before reporting.
        let said_path = self.current_path();
        let disk_size = std::fs::metadata(&said_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let mem_size = brain.stats().file_size as u64;
        let mut reload_note = String::new();
        if disk_size != mem_size {
            match sca_core::said_file::SaidFile::open(&said_path) {
                Ok(fresh) => {
                    *brain = fresh;
                    reload_note = format!(
                        "\n(reloaded from disk: in-memory {} bytes, disk {} bytes â€” on-disk wins)",
                        mem_size, disk_size
                    );
                }
                Err(e) => {
                    reload_note = format!(
                        "\nWARNING: on-disk file changed ({} bytes) but reload failed: {}",
                        disk_size, e
                    );
                }
            }
        }

        let s = brain.stats();

        let lens_info = if let Some(ref lens) = self.lens {
            format!("\nModule: {} (lens â†’ {})\nModule memories: {}\n",
                lens.module_name, lens.parent_path, lens.frame_ids.len())
        } else {
            String::new()
        };

        // If the attached brain is empty but there are populated .said files
        // sitting in the same directory, surface them â€” the user almost
        // certainly meant to work with one of those.
        let other_brains = if s.active_frames == 0 {
            Self::list_populated_sibling_brains(&said_path)
        } else {
            Vec::new()
        };

        // Headline tells the user immediately whether the brain is empty,
        // small, or populated â€” and what they can do from here.
        let headline = if s.active_frames == 0 {
            if !other_brains.is_empty() {
                let mut msg = String::from(
                    "âš  Brain is EMPTY â€” but there are populated brains nearby:\n\n"
                );
                for b in &other_brains {
                    msg.push_str(&format!("  â€¢ {} ({:.1} MB)\n", b.0, b.1 as f64 / 1_048_576.0));
                }
                msg.push_str(
                    "\nYou probably want to attach to one of those. Run:\n\
                     \n\
                     â€¢ open path=\"<name>.said\"    â€” switch to the real brain\n\
                     \n\
                     â€¦or if you want to stay empty and ingest fresh:\n\
                     \n\
                     â€¢ init dir=\"<path>\"          â€” bulk-ingest a whole folder\n\
                     â€¢ remember content=\"â€¦\"        â€” store a single note/memory\n"
                );
                msg
            } else {
                "Brain is EMPTY â€” nothing ingested yet.\n\
                 \n\
                 Next steps:\n\
                 â€¢ init dir=\"<path>\"          â€” bulk-ingest a whole folder\n\
                 â€¢ remember content=\"â€¦\"        â€” store a single note/memory\n\
                 â€¢ open path=\"<name>.said\"     â€” switch to a different brain\n"
                    .to_string()
            }
        } else {
            "Brain is POPULATED and ready to query.\n\
             \n\
             Next steps:\n\
             â€¢ overview                    â€” list detected modules/products\n\
             â€¢ search \"<query>\"            â€” semantic search\n\
             â€¢ sym <name>                  â€” exact symbol lookup\n\
             â€¢ snapshot <module>           â€” extract a module workspace\n"
                .to_string()
        };

        let mode_line = match brain.mode() {
            sca_core::said_file::BrainMode::Portable =>
                "Mode:          portable (embeds full content; USB-offline friendly)",
            sca_core::said_file::BrainMode::Enterprise =>
                "Mode:          ENTERPRISE (pointer-only; content-embedding ingests REFUSED)",
        };

        let output = format!(
            "{}\n\
             â”€â”€â”€ Brain details â”€â”€â”€\n\
             File:          {}\n\
             {}\n\
             {}Memories:      {}  (everything stored in this brain)\n\
             Size on disk:  {} bytes ({:.1} MB)\n\
             Search index:  {}\n\
             Symbols:       {} named functions/classes/tables\n\
             Queries run:   {} (brain learns from usage)\n\
             Dream cycles:  {}   (memory consolidation events){}",
            headline,
            said_path,
            mode_line,
            lens_info,
            s.active_frames,
            s.file_size,
            s.file_size as f64 / 1_048_576.0,
            if s.trigram_present { "present (fast grep available)" } else { "absent (grep will be slower)" },
            s.symbol_count,
            s.brain_queries,
            s.brain_cycles,
            reload_note,
        );

        Ok(CallToolResult::text_content(vec![TextContent::from(output)]))
    }

    fn handle_sym(&self, t: SymTool) -> Result<CallToolResult, CallToolError> {
        let brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        let results = brain.sym(&t.name, 20);

        if results.is_empty() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("No symbol found: {}", t.name),
            )]));
        }

        let mut output = String::new();
        for (i, r) in results.iter().enumerate() {
            output.push_str(&format!(
                "{}. {} {} @ {}:{}-{}\n",
                i + 1, r.kind, r.name, r.doc_id, r.start_line, r.end_line
            ));
        }

        Ok(CallToolResult::text_content(vec![TextContent::from(output)]))
    }

    /// Resolve + apply + syntax-verify ONE edit, returning the new file content
    /// and a summary â€” WITHOUT writing. Shared by `handle_edit` (writes one) and
    /// `handle_edit_batch` (computes all, writes all-or-nothing). Errors carry a
    /// human-readable reason; the caller never writes on Err.
    fn compute_edit(
        &self,
        file: &str,
        mode: &str,
        symbol: Option<&str>,
        anchor: Option<&str>,
        content: Option<&str>,
        allow_large: bool,
    ) -> Result<(String, serde_json::Value), String> {
        use sca_core::edit::{self, EditOp};

        edit::is_safe_relative_path(file)?;

        let is_delete = mode == "delete-symbol";
        let new_text = match (content, is_delete) {
            (Some(c), _) => c.to_string(),
            (None, true) => String::new(),
            (None, false) => return Err("missing 'content'".into()),
        };

        let file_content = std::fs::read_to_string(file)
            .map_err(|e| format!("read {}: {}", file, e))?;

        // Resolve a symbol â†’ (start,end), scoped to file, with drift check +
        // authoritative-content span correction (matches the CLI path).
        let resolve_sym = |name: &str| -> Result<(usize, usize), String> {
            let mut brain = self.brain.lock().map_err(|e| format!("brain lock: {}", e))?;
            let results = brain.sym(name, 50);
            let cands: Vec<edit::SymCandidate> = results.iter().map(|r| edit::SymCandidate {
                doc_id: r.doc_id.clone(),
                name: r.name.clone(),
                start_line: r.start_line as usize,
                end_line: r.end_line as usize,
            }).collect();
            let (start, end) = edit::resolve_symbol_in_file(&cands, file)?;
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
                        corrected_end = hi;
                    }
                }
            }
            Ok((start, corrected_end))
        };
        let want_symbol = || symbol.map(|s| s.to_string()).ok_or_else(|| format!("mode '{}' requires 'symbol'", mode));
        let want_anchor = || anchor.map(|s| s.to_string()).ok_or_else(|| format!("mode '{}' requires 'anchor'", mode));

        let op: EditOp = match mode {
            "insert-after-symbol" => {
                let (_, end) = resolve_sym(&want_symbol()?)?;
                EditOp::InsertAfterLine { line: end, text: new_text }
            }
            "insert-before-symbol" => {
                let (start, _) = resolve_sym(&want_symbol()?)?;
                EditOp::InsertBeforeLine { line: start, text: new_text }
            }
            "replace-symbol" => {
                let (start, end) = resolve_sym(&want_symbol()?)?;
                edit::check_span(end - start + 1, edit::DEFAULT_MAX_SPAN, allow_large)?;
                EditOp::ReplaceLines { start, end, text: new_text }
            }
            "delete-symbol" => {
                let (start, end) = resolve_sym(&want_symbol()?)?;
                edit::check_span(end - start + 1, edit::DEFAULT_MAX_SPAN, allow_large)?;
                EditOp::DeleteLines { start, end }
            }
            // Scope-aware: insert just before the named scope's closing brace,
            // so a new member lands at the right (class) scope, never nested.
            // Auto-indent the member to match sibling indentation.
            "append-into-symbol" => {
                let (start, end) = resolve_sym(&want_symbol()?)?;
                let disk_lines: Vec<&str> = file_content.lines().collect();
                let body_indent = disk_lines.get(start)
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
                let a = want_anchor()?;
                let line = edit::resolve_text_anchor(&file_content, &a)?;
                EditOp::InsertAfterLine { line, text: new_text }
            }
            "insert-before-text" => {
                let a = want_anchor()?;
                let line = edit::resolve_text_anchor(&file_content, &a)?;
                EditOp::InsertBeforeLine { line, text: new_text }
            }
            "replace-text" => {
                EditOp::ReplaceSubstring { needle: want_anchor()?, replacement: new_text }
            }
            "insert-after-context" => {
                let a = want_anchor()?;
                let line = edit::resolve_context_anchor(&file_content, &a)? + a.matches('\n').count();
                EditOp::InsertAfterLine { line, text: new_text }
            }
            "insert-before-context" => {
                let a = want_anchor()?;
                let line = edit::resolve_context_anchor(&file_content, &a)?;
                EditOp::InsertBeforeLine { line, text: new_text }
            }
            "replace-context" => {
                let a = want_anchor()?;
                edit::resolve_context_anchor(&file_content, &a)?;
                EditOp::ReplaceSubstring { needle: a, replacement: new_text }
            }
            other => return Err(format!("unknown mode: {}", other)),
        };

        let result = edit::apply_edit(&file_content, &op)?;

        #[cfg(feature = "code")]
        {
            let ext = Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or("");
            if let Err(e) = edit::verify_syntax(&result.content, ext) {
                // Attach a structured, model-agnostic repair menu (copy-paste
                // ready said-edit moves) computed live from the AST at the
                // landing line. Encoded as JSON in the error so callers can
                // parse `valid_anchors` for one-shot correction.
                let suggestions = sca_core::code_search::suggest_anchors(
                    &file_content, ext, result.applied_at_line);
                let valid: Vec<serde_json::Value> = suggestions.iter().map(|s| serde_json::json!({
                    "mode": s.mode, "symbol": s.symbol, "line": s.line, "kind": s.kind, "note": s.note,
                })).collect();
                return Err(serde_json::json!({
                    "error": format!("{} â€” edit rejected, file unchanged", e),
                    "valid_anchors": valid,
                }).to_string());
            }
        }

        let summary = serde_json::json!({
            "file": file,
            "mode": mode,
            "anchor": symbol.or(anchor).unwrap_or(""),
            "applied_at_line": result.applied_at_line,
            "lines_added": result.lines_added,
            "lines_removed": result.lines_removed,
        });
        Ok((result.content, summary))
    }

    /// Atomic write (temp + rename) for an edited file.
    fn atomic_write_edit(file: &str, content: &str) -> Result<(), String> {
        let target = Path::new(file);
        let stem = target.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tmp = match target.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(d) => d.join(format!(".{}.said-edit.tmp", stem)),
            None => PathBuf::from(format!(".{}.said-edit.tmp", stem)),
        };
        std::fs::write(&tmp, content.as_bytes())
            .map_err(|e| format!("write temp {}: {}", tmp.display(), e))?;
        std::fs::rename(&tmp, target).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("rename to {}: {}", target.display(), e)
        })
    }

    /// Surgical anchored edit â€” the same core the `said edit` CLI uses
    /// (`sca_core::edit`). No whole-file-rewrite path exists, so an autonomous
    /// caller cannot delete the rest of a file.
    fn handle_edit(&self, t: EditTool) -> Result<CallToolResult, CallToolError> {
        let ok_json = |msg: serde_json::Value| {
            Ok(CallToolResult::text_content(vec![TextContent::from(msg.to_string())]))
        };
        let err_json = |msg: String| {
            Ok(CallToolResult::text_content(vec![TextContent::from(edit_error_payload(&msg).to_string())]))
        };

        // explain: pre-validate only â€” return valid_anchors without editing.
        #[cfg(feature = "code")]
        if t.explain {
            if let Err(e) = sca_core::edit::is_safe_relative_path(&t.file) { return err_json(e); }
            let fc = match std::fs::read_to_string(&t.file) {
                Ok(s) => s, Err(e) => return err_json(format!("read {}: {}", t.file, e)),
            };
            let ext = Path::new(&t.file).extension().and_then(|e| e.to_str()).unwrap_or("");
            let line = if let Some(ref name) = t.symbol {
                let brain = self.brain.lock().map_err(|e| CallToolError::from_message(format!("brain lock: {}", e)))?;
                brain.sym(name, 50).iter()
                    .find(|r| sca_core::edit::paths_equal(r.doc_id.split("::").next().unwrap_or(""), &t.file))
                    .map(|r| r.start_line as usize).unwrap_or(1)
            } else if let Some(ref a) = t.anchor {
                sca_core::edit::resolve_text_anchor(&fc, a).unwrap_or(1)
            } else { 1 };
            let suggestions = sca_core::code_search::suggest_anchors(&fc, ext, line);
            let valid: Vec<serde_json::Value> = suggestions.iter().map(|s| serde_json::json!({
                "mode": s.mode, "symbol": s.symbol, "line": s.line, "kind": s.kind, "note": s.note,
            })).collect();
            return ok_json(serde_json::json!({
                "ok": true, "explain": true, "file": t.file, "at_line": line, "valid_anchors": valid,
            }));
        }

        let (new_content, mut summary) = match self.compute_edit(
            &t.file, &t.mode, t.symbol.as_deref(), t.anchor.as_deref(),
            t.content.as_deref(), t.allow_large,
        ) {
            Ok(r) => r,
            Err(e) => return err_json(e),
        };

        if !t.dry_run {
            if let Err(e) = Self::atomic_write_edit(&t.file, &new_content) {
                return err_json(e);
            }
        }

        if let serde_json::Value::Object(ref mut m) = summary {
            m.insert("ok".into(), serde_json::Value::Bool(true));
            m.insert("dry_run".into(), serde_json::Value::Bool(t.dry_run));
        }
        ok_json(summary)
    }

    /// Transactional multi-edit â€” apply a SET of edits all-or-nothing. Every
    /// edit is resolved + applied + syntax-verified in memory first; only if
    /// ALL succeed are the files written. If any fails, nothing is written, so
    /// you can never get a half-applied change set on disk (the GroqCycle gap).
    fn handle_edit_batch(&self, t: EditBatchTool) -> Result<CallToolResult, CallToolError> {
        let err_json = |msg: String| {
            Ok(CallToolResult::text_content(vec![TextContent::from(edit_error_payload(&msg).to_string())]))
        };
        if t.edits.is_empty() {
            return err_json("no edits provided".into());
        }
        // Phase 1: compute every edit in memory. For multiple edits to the SAME
        // file we thread the running content so later edits see earlier ones.
        let mut pending: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut summaries: Vec<serde_json::Value> = Vec::new();
        for (i, e) in t.edits.iter().enumerate() {
            // If this file was already edited in this batch, the on-disk read in
            // compute_edit would miss prior edits â€” so for the common one-edit-
            // per-file case this is exact; multi-edit-same-file is applied in
            // order against disk and we warn rather than silently misorder.
            if pending.contains_key(&e.file) {
                return err_json(format!(
                    "edit {} targets {} which already has a pending edit in this batch; \
                     send one edit per file per batch (or order them via separate calls)",
                    i + 1, e.file
                ));
            }
            match self.compute_edit(&e.file, &e.mode, e.symbol.as_deref(), e.anchor.as_deref(),
                e.content.as_deref(), e.allow_large) {
                Ok((new_content, summary)) => {
                    pending.insert(e.file.clone(), new_content);
                    summaries.push(summary);
                }
                Err(msg) => {
                    // Preserve a structured error (with valid_anchors) if compute_edit
                    // produced one, adding batch context; else wrap the plain string.
                    let mut payload = edit_error_payload(&msg);
                    if let serde_json::Value::Object(ref mut m) = payload {
                        m.insert("failed_edit".into(), serde_json::json!({
                            "index": i + 1, "of": t.edits.len(), "file": e.file,
                        }));
                        m.insert("note".into(), serde_json::Value::String(
                            "NO files written (transactional)".into()));
                    }
                    return Ok(CallToolResult::text_content(vec![TextContent::from(payload.to_string())]));
                }
            }
        }
        // Phase 2: all computed OK â†’ write them all.
        if !t.dry_run {
            for (file, content) in &pending {
                if let Err(e) = Self::atomic_write_edit(file, content) {
                    // Best-effort: a write failure mid-way is the one case we
                    // can't fully roll back, so report exactly what was written.
                    return err_json(format!(
                        "write failed for {} after others may have been written: {} \
                         (re-run; edits are idempotent for anchors that still match)",
                        file, e
                    ));
                }
            }
        }
        Ok(CallToolResult::text_content(vec![TextContent::from(serde_json::json!({
            "ok": true,
            "edits": summaries,
            "files_changed": pending.len(),
            "dry_run": t.dry_run,
        }).to_string())]))
    }

    fn handle_delete(&self, t: DeleteTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Mode 1: Delete specific frame by doc_id
        if let Some(ref doc_id) = t.doc_id {
            if t.dry_run.unwrap_or(false) {
                return Ok(CallToolResult::text_content(vec![TextContent::from(
                    format!("[DRY RUN] Would delete: {}", doc_id),
                )]));
            }
            if brain.tombstone_frame(doc_id) {
                brain.save().map_err(|e| CallToolError::from_message(e))?;
                return Ok(CallToolResult::text_content(vec![TextContent::from(
                    format!("Deleted: {} (tombstoned â€” preserved in history)", doc_id),
                )]));
            } else {
                return Ok(CallToolResult::text_content(vec![TextContent::from(
                    format!("Not found: {}", doc_id),
                )]));
            }
        }

        // Mode 2: Time-based deletion
        let cutoff_secs: Option<u64> = if let Some(days) = t.older_than_days {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            Some(now - (days as u64 * 86400))
        } else if let Some(ref date_str) = t.before_date {
            // Parse YYYY-MM-DD â†’ approximate Unix timestamp
            let parts: Vec<&str> = date_str.split('-').collect();
            if parts.len() == 3 {
                let year: i64 = parts[0].parse().unwrap_or(2026);
                let month: i64 = parts[1].parse().unwrap_or(1);
                let day: i64 = parts[2].parse().unwrap_or(1);
                // Rough calculation: days since epoch
                let days_since_epoch = (year - 1970) * 365 + (month - 1) * 30 + day;
                Some((days_since_epoch * 86400) as u64)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(cutoff) = cutoff_secs {
            let all_frames = brain.frames.get_all_frames();
            let mut to_delete: Vec<String> = Vec::new();

            for meta in &all_frames {
                if meta.status != sca_core::frames::FrameStatus::Active {
                    continue;
                }
                if meta.created_at < cutoff {
                    // Check tag filter if specified
                    if let Some(ref tag) = t.tag_filter {
                        if !meta.tags.iter().any(|t| t == tag) {
                            continue;
                        }
                    }
                    to_delete.push(meta.doc_id.clone());
                }
            }

            if t.dry_run.unwrap_or(false) {
                let mut output = format!("[DRY RUN] Would delete {} frames:\n", to_delete.len());
                for did in to_delete.iter().take(20) {
                    output.push_str(&format!("  {}\n", did));
                }
                if to_delete.len() > 20 {
                    output.push_str(&format!("  ... and {} more\n", to_delete.len() - 20));
                }
                return Ok(CallToolResult::text_content(vec![TextContent::from(output)]));
            }

            let mut deleted = 0;
            for did in &to_delete {
                if brain.tombstone_frame(did) {
                    deleted += 1;
                }
            }

            if deleted > 0 {
                brain.save().map_err(|e| CallToolError::from_message(e))?;
            }

            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("Deleted {} frames (tombstoned â€” preserved in history)", deleted),
            )]));
        }

        Ok(CallToolResult::text_content(vec![TextContent::from(
            "No deletion criteria specified. Use doc_id, older_than_days, or before_date.".to_string(),
        )]))
    }

    fn handle_discover(&self) -> Result<CallToolResult, CallToolError> {
        let brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Count object types from symbols
        let mut tables = 0u32;
        let mut procs = 0u32;
        let mut triggers = 0u32;
        let mut views = 0u32;
        let mut functions = 0u32;

        let doc_ids = brain.frames.active_doc_ids();
        for did in &doc_ids {
            let title = brain.frames.get_meta(did)
                .and_then(|m| m.title.clone())
                .unwrap_or_default()
                .to_lowercase();
            if title.contains("(table)") || title.contains("create_table") { tables += 1; }
            else if title.contains("(proc)") || title.contains("create_procedure") { procs += 1; }
            else if title.contains("(trigger)") || title.contains("create_trigger") { triggers += 1; }
            else if title.contains("(view)") || title.contains("create_view") { views += 1; }
            else if title.contains("(function)") || title.contains("create_function") { functions += 1; }
        }

        let total = tables + procs + triggers + views + functions;
        let output = format!(
            "Module Discovery\n\
             Total objects: {} ({} tables, {} procs, {} triggers, {} views, {} functions)\n\
             Active frames: {}\n\n\
             For full module boundary analysis with FK graph clustering and \
             hub table modernization strategy, run: said discover (CLI)\n\n\
             Quick summary available via search:\n\
             - Use search with 'card management' to find card-related objects\n\
             - Use search with 'billing fees' to find billing objects\n\
             - Use sym to look up specific object names",
            total, tables, procs, triggers, views, functions,
            doc_ids.len()
        );

        Ok(CallToolResult::text_content(vec![TextContent::from(output)]))
    }

    fn handle_snapshot(&self, t: SnapshotTool) -> Result<CallToolResult, CallToolError> {
        // Snapshot needs to write files to disk â€” run via CLI subprocess
        let said_path_owned = self.current_path();
        let said_path: &str = &said_path_owned;
        let module = &t.module;

        // Run the CLI in the brain's directory â€” otherwise relative output
        // paths like `.said-code/...` would land wherever Cursor started the
        // MCP server (typically the user's home dir), not next to the brain.
        let brain_dir = self.brain_dir();
        let output = std::process::Command::new("said")
            .args(["snapshot", module, "--path", said_path, "--json"])
            .current_dir(&brain_dir)
            .output();

        match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);

                if !result.status.success() {
                    return Ok(CallToolResult::text_content(vec![TextContent::from(
                        format!("âœ— Snapshot failed for module '{}'.\n\nError output:\n{}",
                            module, stderr.trim()),
                    )]));
                }

                // Parse the JSON the CLI returned. If parsing fails, fall back
                // to raw output â€” but NEVER fabricate. We also GROUND-TRUTH
                // every number by re-checking the filesystem: the LLM gets a
                // "Verified on disk" block it cannot paraphrase.
                let json_line = stdout.lines()
                    .find(|l| l.trim_start().starts_with('{'))
                    .unwrap_or("{}");
                let parsed: serde_json::Value = serde_json::from_str(json_line)
                    .unwrap_or(serde_json::Value::Object(Default::default()));

                let output_dir = parsed.get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let files_copied = parsed.get("files_copied").and_then(|v| v.as_u64()).unwrap_or(0);
                let tables = parsed.get("exclusive_tables").and_then(|v| v.as_u64()).unwrap_or(0);
                let procs = parsed.get("exclusive_procs").and_then(|v| v.as_u64()).unwrap_or(0);
                let triggers = parsed.get("exclusive_triggers").and_then(|v| v.as_u64()).unwrap_or(0);
                let views = parsed.get("exclusive_views").and_then(|v| v.as_u64()).unwrap_or(0);
                let functions = parsed.get("exclusive_functions").and_then(|v| v.as_u64()).unwrap_or(0);
                let shared = parsed.get("shared_hub_tables").and_then(|v| v.as_u64()).unwrap_or(0);
                let frames = parsed.get("frames_in_brain").and_then(|v| v.as_u64()).unwrap_or(0);

                // Ground-truth verification: the CLI reports a RELATIVE path
                // (e.g. `.said-code/card.vivere`). Resolve it against the
                // brain's directory so we're checking the right place.
                let resolved = if output_dir.is_empty() {
                    std::path::PathBuf::new()
                } else {
                    self.resolve_in_brain_dir(&output_dir)
                };
                let folder_exists = resolved.is_dir();
                let actual_files = if folder_exists {
                    walkdir_count(resolved.join("Exclusive"))
                } else { 0 };
                let boundary_exists = folder_exists
                    && resolved.join("BOUNDARY.md").is_file();
                let lens_exists = folder_exists && {
                    let stem = Path::new(said_path).file_stem()
                        .and_then(|s| s.to_str()).unwrap_or("brain");
                    let lens_name = format!("{}.{}.said", module.to_lowercase(), stem);
                    resolved.join(&lens_name).is_file()
                };
                let resolved_str = resolved.to_string_lossy().to_string();

                let verification = if !folder_exists {
                    format!(
                        "âš  DISK CHECK FAILED: output folder does NOT exist.\n\
                         \n\
                         Reported relative path: {}\n\
                         Resolved absolute path: {}\n\
                         \n\
                         The snapshot tool reported success but nothing was \
                         written at that location. This is a REAL FAILURE. Do \
                         NOT tell the user the snapshot succeeded.",
                        output_dir, resolved_str
                    )
                } else {
                    format!(
"â”€â”€â”€ Ground-truth (verified on disk) â”€â”€â”€
Relative path:     {}
Absolute path:     {}  [exists: âœ“]
BOUNDARY.md:       {}
Lens brain file:   {}
Files in Exclusive/ (actual count): {}
",
                        output_dir,
                        resolved_str,
                        if boundary_exists { "âœ“ present" } else { "âœ— MISSING" },
                        if lens_exists { "âœ“ present" } else { "âœ— MISSING" },
                        actual_files,
                    )
                };

                let msg = format!(
"âœ“ Module '{}' extracted.

â”€â”€â”€ From the snapshot tool (JSON) â”€â”€â”€
Exclusive tables:    {}
Exclusive procs:     {}
Exclusive triggers:  {}
Exclusive views:     {}
Exclusive functions: {}
Shared hub tables:   {}
Files copied:        {}
Brain frames:        {}
Output folder:       {}

{}
â”€â”€â”€ What to do next â”€â”€â”€
â€¢ sandbox module=\"{}\"            â€” spin up a live test DB for this module
â€¢ clean targets=[\"{}\"]           â€” tear down this workspace
â€¢ open path=\"{}/{}.{}.said\"  â€” attach to the module's lens brain for focused search

STRICT RULE FOR THE LLM: report the EXACT numbers above. Do not paraphrase, \
round, or replace any value with one from memory or prior runs. If any \
field above shows 0, that is the truth â€” say 0, not a past value.
",
                    module,
                    tables, procs, triggers, views, functions, shared, files_copied, frames,
                    output_dir,
                    verification,
                    module, module,
                    output_dir,
                    module.to_lowercase(),
                    Path::new(said_path).file_stem().and_then(|s| s.to_str()).unwrap_or("brain"),
                );
                Ok(CallToolResult::text_content(vec![TextContent::from(msg)]))
            }
            Err(e) => {
                Ok(CallToolResult::text_content(vec![TextContent::from(
                    format!("âœ— Could not run snapshot (said CLI not in PATH): {}\n\
                             Run manually from a terminal: said snapshot {} --path {}",
                        e, module, said_path),
                )]))
            }
        }
    }

    fn handle_open(&self, t: OpenTool) -> Result<CallToolResult, CallToolError> {
        // Normalize the path â€” Windows users often paste forward- or back-slash
        // paths interchangeably. We keep it as-is on disk but canonicalize for
        // the same-file check below.
        let new_path = t.path.clone();
        let new_pathbuf = std::path::PathBuf::from(&new_path);

        // Create the file if it doesn't exist â€” this is the whole point of
        // `open`: "attach to this brain, creating it if needed".
        let was_created = !new_pathbuf.exists();
        if was_created {
            // Make sure the parent directory exists.
            if let Some(parent) = new_pathbuf.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                            "Could not create parent directory {}: {}", parent.display(), e
                        ))]));
                    }
                }
            }
            let mut sf = sca_core::said_file::SaidFile::create(&new_path);
            if let Err(e) = sf.save() {
                return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                    "Could not create empty brain at {}: {}", new_path, e
                ))]));
            }
        }

        // Open the (possibly freshly-created) file and swap it in.
        let fresh = match sca_core::said_file::SaidFile::open(&new_path) {
            Ok(mut b) => {
                // Embedded encoder first, then known install paths â€” same as open_brain.
                if !b.auto_load_encoder() {
                    for p in &[
                        "said-lam-static",
                        "../said-lam-static",
                        "SAID-LAM-private/said-lam-static",
                    ] {
                        if std::path::Path::new(p).exists() {
                            let _ = b.load_encoder(p);
                            break;
                        }
                    }
                }
                b
            }
            Err(e) => {
                return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                    "Could not open {}: {}\n\
                     If the file is corrupt, call `create path='{}'` to overwrite.",
                    new_path, e, new_path
                ))]));
            }
        };

        let old_path = self.current_path();
        let size = std::fs::metadata(&new_path).map(|m| m.len()).unwrap_or(0);
        let active_frames = fresh.frames.active_count();
        let switching_away = !Self::same_file(&old_path, &new_path);

        // Atomically swap path and brain. Take BOTH mutexes and hold them
        // across both writes so a concurrent tool call (MCP handlers can
        // run on separate tokio tasks) can't observe one updated without
        // the other. Lock order: brain then path, always â€” matches every
        // other code path in this file so we don't deadlock with them.
        {
            let mut brain_guard = self.brain.lock().map_err(|e| {
                CallToolError::from_message(format!("brain lock: {}", e))
            })?;
            let mut path_guard = self.said_path.lock().map_err(|e| {
                CallToolError::from_message(format!("path lock: {}", e))
            })?;
            *brain_guard = fresh;
            *path_guard = new_path.clone();
        }

        // â”€â”€ Ephemeral bookkeeping â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // If the old path was a placeholder we auto-created and never
        // populated, remove it now so the user isn't left with a stray
        // vivere.said next to their real willie.said.
        let mut cleanup_note = String::new();
        if switching_away {
            let was_ephemeral = {
                let eph = self.ephemeral_brains.lock().ok();
                eph.as_ref()
                    .map(|s| s.iter().any(|p| Self::same_file(p, &old_path)))
                    .unwrap_or(false)
            };
            if was_ephemeral && Self::is_pristine_brain(&old_path) {
                match std::fs::remove_file(&old_path) {
                    Ok(_) => {
                        cleanup_note = format!(
                            "\nRemoved empty placeholder brain: {}",
                            old_path
                        );
                        if let Ok(mut eph) = self.ephemeral_brains.lock() {
                            eph.retain(|p| !Self::same_file(p, &old_path));
                        }
                    }
                    Err(e) => {
                        cleanup_note = format!(
                            "\n(could not remove placeholder {}: {} â€” safe to delete manually)",
                            old_path, e
                        );
                    }
                }
            }
        }

        // If the NEW brain was just created empty by us, track it as
        // ephemeral until the user populates it. `init` / `remember` /
        // `ingest` should call `mark_populated()` to retire the flag.
        if was_created {
            if let Ok(mut eph) = self.ephemeral_brains.lock() {
                eph.insert(new_path.clone());
            }
        }

        let (headline, next_steps) = if was_created {
            (
                format!("âœ“ Created new empty brain: {} ({} bytes)", new_path, size),
                "This brain has no content yet. To populate it:\n\
                 \n\
                 â€¢ init dir=\"<path>\"            â€” bulk-ingest a code/SQL/docs folder (best for monoliths)\n\
                 â€¢ ingest path=\"<file>\"         â€” add a single file (PDF, DOCX, MP4, etc.)\n\
                 â€¢ remember content=\"â€¦\"         â€” store a note or memory directly\n\
                 \n\
                 Example: `init dir=\"G:\\\\work\\\\my-project\\\\src\"`"
                    .to_string(),
            )
        } else {
            (
                format!("âœ“ Attached to existing brain: {}", new_path),
                format!(
                    "Size: {} bytes  â€¢  Active frames: {}\n\
                     \n\
                     Ready to query:\n\
                     \n\
                     â€¢ overview                     â€” what's in this brain?\n\
                     â€¢ search \"<query>\"             â€” semantic search\n\
                     â€¢ sym <name>                   â€” look up a function/class/table by name\n\
                     â€¢ snapshot <module>            â€” extract a module workspace\n\
                     â€¢ sandbox <module>             â€” spin up a live SQL test DB",
                    size, active_frames
                ),
            )
        };

        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "{}\n\
             \n\
             {}{}\n\
             \n\
             (Previous brain: {})",
            headline, next_steps, cleanup_note, old_path
        ))]))
    }

    /// Mark the currently-attached brain as "populated" â€” removes it from the
    /// ephemeral set so a later `open` won't delete it. Called by tools that
    /// add content (init, remember, ingest).
    fn mark_populated(&self) {
        let current = self.current_path();
        if let Ok(mut eph) = self.ephemeral_brains.lock() {
            eph.retain(|p| !Self::same_file(p, &current));
        }
    }

    fn handle_create(&self, t: CreateTool) -> Result<CallToolResult, CallToolError> {
        // Safety: determine whether the target path refers to the SAME file
        // the MCP server is already attached to. Compare canonical forms so
        // "vivere.said" vs "G:\development\SAID-ECHO\vivere.said" vs
        // "G:/development/SAID-ECHO/vivere.said" all resolve to the same thing.
        let target_raw = std::path::PathBuf::from(&t.file);
        let live_path = self.current_path();
        let live_raw = std::path::PathBuf::from(&live_path);

        let target_canon = std::fs::canonicalize(&target_raw)
            .or_else(|_| target_raw.parent()
                .map(|p| std::fs::canonicalize(p)
                    .map(|c| c.join(target_raw.file_name().unwrap_or_default())))
                .unwrap_or(Ok(target_raw.clone())))
            .unwrap_or(target_raw.clone());
        let live_canon = std::fs::canonicalize(&live_raw).unwrap_or(live_raw.clone());

        let same_file = target_canon == live_canon;

        // If the file exists and is populated, require explicit overwrite.
        // A brain with actual content (> ~1 KB of data + >0 frames) should
        // never be silently wiped by a "create" call.
        if target_raw.exists() {
            let size = std::fs::metadata(&target_raw).map(|m| m.len()).unwrap_or(0);
            // A freshly-created brain is ~19-20 KB with no frames. Anything
            // noticeably larger is populated and shouldn't be overwritten
            // without the `overwrite` flag.
            if size > 32_000 {
                return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                    "Refusing to overwrite {} â€” it's {} bytes, which looks populated.\n\
                     \n\
                     If this is intentional, call with `overwrite: true` (when supported) \
                     or delete the file manually first.\n\
                     \n\
                     {}",
                    t.file, size,
                    if same_file {
                        format!("NOTE: this IS the brain this MCP server is attached to. \
                                 Overwriting it while MCP is running will desynchronize \
                                 in-memory state from disk.")
                    } else {
                        String::new()
                    }
                ))]));
            }
        }

        // Write the empty brain. Mode is set ONCE at creation and is
        // immutable â€” Portable and Enterprise are licensed separately, and
        // there is no later mode-switch command. Default: Portable.
        let chosen_mode = t.mode.as_deref()
            .and_then(sca_core::said_file::BrainMode::parse)
            .unwrap_or(sca_core::said_file::BrainMode::Portable);
        let mut sf = sca_core::said_file::SaidFile::create_with_mode(&t.file, chosen_mode);
        if let Err(e) = sf.save() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                "Could not write {}: {}", t.file, e
            ))]));
        }
        let size = std::fs::metadata(&t.file).map(|m| m.len()).unwrap_or(0);

        // If we just wrote the MCP's live brain, reload so subsequent tool
        // calls see the empty brain instead of the stale in-memory state.
        let mut reload_note = String::new();
        if same_file {
            if let Ok(mut brain) = self.brain.lock() {
                match sca_core::said_file::SaidFile::open(&live_path) {
                    Ok(fresh) => {
                        *brain = fresh;
                        reload_note = "\nMCP in-memory state reloaded from the new empty brain.".into();
                    }
                    Err(e) => {
                        reload_note = format!("\nWARNING: could not reload in-memory brain: {}", e);
                    }
                }
            }
        }

        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "Created empty brain: {} ({} bytes){}\n\
             \n\
             Next steps:\n\
             - Call `open path='{}'` to attach this MCP server to the new brain, then\n\
             - Call `init` with a source directory to bulk-ingest\n\
             - Or call `remember` / `ingest` to add content one piece at a time{}",
            t.file, size, reload_note, t.file,
            if same_file { "" } else {
                "\n\nNOTE: this MCP server is still attached to a different brain. \
                 Use `open path='<newfile>'` to switch to the new brain without restarting."
            }
        ))]))
    }

    fn handle_init(&self, t: InitTool) -> Result<CallToolResult, CallToolError> {
        // `said init` opens the brain, walks the dir, and writes results back.
        // The MCP server already holds this brain mmap'd â€” release our mutex so
        // the subprocess can take the file lock.
        let said_path = self.current_path();
        // Use absolute paths so the subprocess doesn't depend on our CWD.
        let abs_brain_path = std::fs::canonicalize(&said_path)
            .unwrap_or_else(|_| std::path::PathBuf::from(&said_path))
            .to_string_lossy().to_string();
        let mut args: Vec<String> = vec![
            "init".into(), t.dir.clone(),
            "--path".into(), abs_brain_path,
        ];
        if t.incremental.unwrap_or(false) {
            args.push("--incremental".into());
        }

        let own_exe = std::env::current_exe().ok();
        let sibling = own_exe.as_ref().and_then(|p| p.parent())
            .map(|d| d.join(if cfg!(windows) { "said.exe" } else { "said" }));
        let cli_paths: Vec<std::ffi::OsString> = std::iter::once(std::ffi::OsString::from("said"))
            .chain(sibling.map(|p| p.into_os_string()))
            .collect();

        // Run subprocess in the brain's directory â€” snapshot/.said-code output
        // lands there instead of inheriting Cursor's CWD.
        let brain_dir = self.brain_dir();
        for exe in &cli_paths {
            let output = std::process::Command::new(exe)
                .args(&args)
                .current_dir(&brain_dir)
                .output();
            if let Ok(r) = output {
                let stdout = String::from_utf8_lossy(&r.stdout);
                let stderr = String::from_utf8_lossy(&r.stderr);
                let raw = if stderr.is_empty() { stdout.to_string() } else {
                    format!("{}\n{}", stdout, stderr)
                };

                if r.status.success() {
                    // Reload mmap so subsequent calls see fresh frames, and
                    // clear the ephemeral flag so the brain survives future
                    // `open` calls.
                    let (frames, size) = {
                        let mut frames = 0u64;
                        let mut size = 0u64;
                        if let Ok(mut brain) = self.brain.lock() {
                            if let Ok(fresh) = sca_core::said_file::SaidFile::open(&said_path) {
                                frames = fresh.stats().active_frames as u64;
                                size = fresh.stats().file_size as u64;
                                *brain = fresh;
                            }
                        }
                        (frames, size)
                    };
                    self.mark_populated();

                    // A friendly, guided response. The user shouldn't have to
                    // figure out "what now?" â€” we tell them the three most
                    // useful next steps directly.
                    let size_mb = size as f64 / 1_048_576.0;
                    let msg = format!(
"âœ“ Ingest complete â€” brain populated.

Brain:      {}
Source:     {}
Frames:     {}
Size:       {:.1} MB

Your brain is ready. Try any of these next:

  1. overview              â†’ see what modules / products are detected
  2. search \"â€¦\"            â†’ semantic search across everything
  3. sym <name>            â†’ look up an exact function / table / class
  4. snapshot <module>     â†’ extract a module into its own workspace
  5. sandbox <module>      â†’ spin up a live test database (SQL brains only)

Example: ask me \"overview\" or \"search for card validation\" and I'll run it.
",
                        said_path, t.dir, frames, size_mb
                    );
                    return Ok(CallToolResult::text_content(vec![TextContent::from(msg)]));
                } else {
                    // Surface the CLI's actual error.
                    let msg = format!(
"âœ— Ingest failed.

Source:  {}
Brain:   {}

Error output:
{}

Common fixes:
  - Check the directory path exists and is readable
  - For monoliths, point at the top-level folder (we walk subdirectories)
  - If the brain is locked, another MCP session may have it open
",
                        t.dir, said_path, raw.trim()
                    );
                    return Ok(CallToolResult::text_content(vec![TextContent::from(msg)]));
                }
            }
        }
        Ok(CallToolResult::text_content(vec![TextContent::from(
            "Could not run `said init` â€” the said CLI binary wasn't found.\n\
             \n\
             The CLI must be on PATH or installed next to said-mcp.exe. If you're \
             in development, rebuild with: cargo build -p said-cli --features code --release"
                .to_string()
        )]))
    }

    fn handle_overview(&self, t: OverviewTool) -> Result<CallToolResult, CallToolError> {
        // Run `said overview` â€” reusing CLI keeps catalogue-derivation in one place.
        let abs_path = std::fs::canonicalize(self.current_path())
            .unwrap_or_else(|_| std::path::PathBuf::from(self.current_path()))
            .to_string_lossy().to_string();
        let mut args: Vec<String> = vec!["overview".into(), "--path".into(), abs_path];
        if let Some(check) = &t.check {
            args.push("--check".into());
            args.push(check.clone());
        }

        let own_exe = std::env::current_exe().ok();
        let sibling = own_exe.as_ref().and_then(|p| p.parent())
            .map(|d| d.join(if cfg!(windows) { "said.exe" } else { "said" }));
        let cli_paths: Vec<std::ffi::OsString> = std::iter::once(std::ffi::OsString::from("said"))
            .chain(sibling.map(|p| p.into_os_string()))
            .collect();
        let brain_dir = self.brain_dir();

        for exe in &cli_paths {
            let output = std::process::Command::new(exe)
                .args(&args)
                .current_dir(&brain_dir)
                .output();
            if let Ok(r) = output {
                let stdout = String::from_utf8_lossy(&r.stdout);
                let stderr = String::from_utf8_lossy(&r.stderr);
                // Filter out the encoder-loaded banner that isn't useful in MCP output.
                let clean: String = stdout.lines()
                    .filter(|l| !l.contains("[SCA] Static encoder auto-loaded"))
                    .collect::<Vec<_>>().join("\n");
                let combined = if stderr.is_empty() { clean } else {
                    format!("{}\n{}", clean, stderr)
                };
                return Ok(CallToolResult::text_content(vec![TextContent::from(combined)]));
            }
        }
        Ok(CallToolResult::text_content(vec![TextContent::from(
            "Could not run `said overview` â€” said CLI not on PATH or next to said-mcp.exe."
                .to_string()
        )]))
    }

    fn handle_clean(&self, t: CleanTool) -> Result<CallToolResult, CallToolError> {
        // Figure out whether this clean call might delete the currently-
        // attached brain file. If so, we MUST drop our mmap handle before
        // invoking `said clean` (Windows can't delete an open mmap) and
        // swap in a fresh empty brain afterwards so subsequent tool calls
        // don't see ghost content from the old in-memory state.
        let current_path = self.current_path();
        let targets_contain_brain = t.targets.as_ref()
            .map(|v| v.iter().any(|s| s.ends_with(".said") || s.ends_with(".SAID")))
            .unwrap_or(false);
        let will_touch_attached_brain = t.all.unwrap_or(false) || {
            // If any target, as a path, points at the attached brain, we
            // need to drop the mmap too.
            if let Some(targets) = &t.targets {
                targets.iter().any(|s| {
                    (s.ends_with(".said") || s.ends_with(".SAID"))
                        && Self::same_file(s, &current_path)
                })
            } else { false }
        };

        // Drop the mmap now so the file handle is released before `said
        // clean` tries to remove the file. We replace the brain with an
        // in-memory-only empty one pointed at the SAME path â€” if `clean`
        // deletes the file we'll re-save it as a placeholder afterwards.
        let dropped = if will_touch_attached_brain || targets_contain_brain {
            if let Ok(mut brain) = self.brain.lock() {
                *brain = sca_core::said_file::SaidFile::create(&current_path);
                true
            } else { false }
        } else { false };

        let mut args: Vec<String> = vec!["clean".into()];
        if let Some(targets) = &t.targets {
            for tgt in targets { args.push(tgt.clone()); }
        }
        if t.all.unwrap_or(false) { args.push("--all".into()); }
        if t.containers_only.unwrap_or(false) { args.push("--containers-only".into()); }
        if t.dry_run.unwrap_or(false) { args.push("--dry-run".into()); }
        // Forward --path so `clean --all` knows which brain is attached.
        args.push("--path".into());
        let abs_brain = std::fs::canonicalize(&current_path)
            .unwrap_or_else(|_| std::path::PathBuf::from(&current_path))
            .to_string_lossy().to_string();
        args.push(abs_brain);

        let own_exe = std::env::current_exe().ok();
        let sibling = own_exe.as_ref().and_then(|p| p.parent())
            .map(|d| d.join(if cfg!(windows) { "said.exe" } else { "said" }));
        let cli_paths: Vec<std::ffi::OsString> = std::iter::once(std::ffi::OsString::from("said"))
            .chain(sibling.map(|p| p.into_os_string()))
            .collect();
        let brain_dir = self.brain_dir();

        let mut response: Option<String> = None;
        for exe in &cli_paths {
            let output = std::process::Command::new(exe)
                .args(&args)
                .current_dir(&brain_dir)
                .output();
            if let Ok(r) = output {
                let stdout = String::from_utf8_lossy(&r.stdout);
                let stderr = String::from_utf8_lossy(&r.stderr);
                let combined = if stderr.is_empty() { stdout.to_string() } else {
                    format!("{}\n{}", stdout, stderr)
                };
                response = Some(combined);
                break;
            }
        }

        // After `said clean` ran, reconcile the in-memory brain with disk.
        if dropped && !t.dry_run.unwrap_or(false) {
            let mut reload_note = String::new();
            if !std::path::Path::new(&current_path).exists() {
                // File was deleted. Write an empty placeholder back so the
                // MCP stays attachable. Mark it ephemeral so the next `open`
                // can clean it up if the user switches brains.
                let mut fresh = sca_core::said_file::SaidFile::create(&current_path);
                if fresh.save().is_ok() {
                    if let Ok(mut brain) = self.brain.lock() { *brain = fresh; }
                    if let Ok(mut eph) = self.ephemeral_brains.lock() {
                        eph.insert(current_path.clone());
                    }
                    reload_note = format!(
                        "\n\nBrain file was deleted. MCP is now attached to a fresh \
                         empty brain at {}. Use `open path='...'` to switch to a real brain.",
                        current_path
                    );
                }
            } else {
                // File still exists â€” reload it (could have been shrunk /
                // modified by whatever clean did).
                if let Ok(fresh) = sca_core::said_file::SaidFile::open(&current_path) {
                    if let Ok(mut brain) = self.brain.lock() { *brain = fresh; }
                    reload_note = format!(
                        "\n\nMCP in-memory brain reloaded from disk: {}",
                        current_path
                    );
                }
            }
            if let Some(ref mut r) = response {
                r.push_str(&reload_note);
            }
        }

        match response {
            Some(r) => Ok(CallToolResult::text_content(vec![TextContent::from(r)])),
            None => Ok(CallToolResult::text_content(vec![TextContent::from(
                "Could not run `said clean` â€” the said CLI is not on PATH or next to said-mcp.exe. \
                 From your shell: `said clean` (optionally with module names or --all)."
                    .to_string()
            )])),
        }
    }

    fn handle_sandbox(&self, t: SandboxTool) -> Result<CallToolResult, CallToolError> {
        // Re-open brain from disk to get latest data
        // (snapshot may have modified the file since MCP started)
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Reload brain from disk if the file was modified externally
        let said_path_owned = self.current_path();
        if let Ok(fresh) = sca_core::said_file::SaidFile::open(&said_path_owned) {
            *brain = fresh;
            // Embedded encoder first, then known install paths.
            if !brain.auto_load_encoder() {
                for p in &["said-lam-static", "../said-lam-static", "SAID-LAM-private/said-lam-static"] {
                    if Path::new(p).exists() {
                        let _ = brain.load_encoder(p);
                        break;
                    }
                }
            }
        }

        let module = &t.module;
        let module_lower = module.to_lowercase();
        let said_stem = Path::new(&said_path_owned)
            .file_stem().and_then(|s| s.to_str()).unwrap_or("brain");

        // Collect the full list of modules this sandbox should contain.
        // `module` is always included; `modules` adds co-deployed modules for
        // cross-module interaction testing (e.g. card + billing + fee together).
        let mut all_modules: Vec<String> = vec![module_lower.clone()];
        if let Some(ref extras) = t.modules {
            for m in extras {
                let m_lc = m.to_lowercase();
                if !all_modules.contains(&m_lc) { all_modules.push(m_lc); }
            }
        }
        let is_multi = all_modules.len() > 1;

        // Workspace root: .said-code/<module>.<brain> next to the attached
        // BRAIN (never relative to whatever cwd Cursor spawned us in â€” that
        // used to dump workspaces into the user's home directory). We make
        // the path absolute by rooting it at `brain_dir()`.
        let brain_root = self.brain_dir();
        let workspace_root = brain_root.join(".said-code");
        let combo_label = all_modules.join("+");
        let scoped_label = match &t.label {
            Some(s) if !s.is_empty() => format!("{}-{}", combo_label, s),
            _ => combo_label.clone(),
        };
        let base_dir_pb = workspace_root.join(format!("{}.{}", scoped_label, said_stem));
        let sandbox_dir_pb = base_dir_pb.join("sandbox");
        let sandbox_dir = sandbox_dir_pb.to_string_lossy().to_string();

        // Per-sandbox Docker identity â€” default 1433 but user can override.
        // Different ports let `card`, `billing`, `card+billing` all run at once.
        let port: u16 = t.port.unwrap_or(1433);
        let container_name = format!("said-sbx-{}-{}", scoped_label.replace('+', "-"), port);

        // Build the keyword set that decides which PROCs/TRIGGERS belong to
        // this sandbox. Generic: expand every label segment into word-boundary
        // forms (`_X_`, `X_`, `_X`). Works for any monolith without hardcoding.
        let mut module_keywords: Vec<String> = Vec::new();
        for label in &all_modules {
            module_keywords.push(label.clone());
            for seg in label.split(|c: char| c == '-' || c == '_') {
                if seg.len() < 2 { continue; }
                let upper = seg.to_uppercase();
                module_keywords.push(format!("_{}_", upper).to_lowercase());
                module_keywords.push(format!("{}_", upper).to_lowercase());
                module_keywords.push(format!("_{}", upper).to_lowercase());
            }
        }
        // Build a set of client tags the requested modules might match.
        // Per multi-client convention, frames carry `client:<Name>` tags
        // when ingested under `1-ground-truth/<Name>/...`. If the user
        // says `said sandbox TXN`, anything tagged `client:TXN` belongs
        // here regardless of how the doc_id is structured.
        let module_client_tags: Vec<String> = all_modules.iter()
            .map(|m| format!("client:{}", m).to_lowercase())
            .collect();
        let matches_module = |did_lower: &str, title_lower: &str, tags: &[String]| -> bool {
            // Strict client-tag match wins when present (multi-client mode).
            if !module_client_tags.is_empty() {
                let any_client_tagged = tags.iter()
                    .any(|t| t.to_lowercase().starts_with("client:"));
                if any_client_tagged {
                    return tags.iter().any(|t| {
                        let tl = t.to_lowercase();
                        module_client_tags.iter().any(|mc| &tl == mc)
                    });
                }
            }
            // Fall back to legacy substring match for single-client
            // workspaces (no client: tag on any frame).
            module_keywords.iter().any(|kw| did_lower.contains(kw) || title_lower.contains(kw))
        };

        // Check if snapshot(s) were run. For multi-module, any one existing is
        // enough to proceed â€” full schema comes from parent brain regardless.
        let has_snapshot = all_modules.iter().any(|m| {
            let p = workspace_root.join(format!("{}.{}", m, said_stem));
            p.join("Exclusive").exists() || p.join("BOUNDARY.md").exists()
        }) || base_dir_pb.join("Exclusive").exists();

        // Create sandbox directory
        let _ = std::fs::create_dir_all(&sandbox_dir_pb);

        // ALL objects from BRAIN â€” complete schema for FK/function dependencies
        // Module-specific filtering happens at the proc/trigger level
        let mut functions: Vec<(String, String, Vec<String>)> = Vec::new(); // (name, content, deps)
        let mut tables: Vec<(String, String, String, Vec<String>)> = Vec::new();
        let mut procs: Vec<(String, String)> = Vec::new();
        let mut triggers: Vec<(String, String)> = Vec::new();
        let mut views: Vec<(String, String)> = Vec::new();
        let mut seed_parts: Vec<String> = Vec::new();
        // MERGE-style lookup-data scripts (sqlMasterDataTxnGlobal layout):
        // a flat folder of `<table>.sql` files that disable triggers,
        // toggle IDENTITY_INSERT, MERGE INTO [lookups].<table>, then
        // re-enable triggers. Shipped to docker as `03-data.sql` so
        // SQL Server runs them after schema (01) and INSERT seed (02).
        // MERGE files indexed by (manifest_scope, table_name).
        // `manifest_scope` is the directory path containing the
        // data-index.json that owns this MERGE â€” typically the client
        // folder under 1-ground-truth/. Multi-client workspaces have
        // multiple manifests; this keeps each MERGE matched to its own.
        let mut data_by_table: std::collections::BTreeMap<(String, String), String> =
            std::collections::BTreeMap::new();
        // Per-manifest dependency order: scope_prefix â†’ table list.
        // Multiple data-index.json files coexist (one per client repo).
        let mut data_index_orders: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();

        let doc_ids: Vec<String> = brain.frames.active_doc_ids().into_iter()
            .map(|s| s.to_string()).collect();

        for did in &doc_ids {
            let meta = brain.frames.get_meta(did);
            let title = meta.as_ref()
                .and_then(|m| m.title.clone()).unwrap_or_default().to_lowercase();
            let frame_tags: Vec<String> = meta.as_ref()
                .map(|m| m.tags.clone()).unwrap_or_default();
            let did_lower = did.to_lowercase();
            let is_module = matches_module(&did_lower, &title, &frame_tags);

            // Multi-client gate for non-proc objects. When the user
            // requested a specific client (any frame in the brain has a
            // `client:` tag), restrict ALL emitted objects (tables /
            // functions / triggers / views / MERGE data) to ones tagged
            // with the requested client. Otherwise objects from sibling
            // clients ride along into the wrong sandbox.
            if !module_client_tags.is_empty() {
                let any_client_tagged = frame_tags.iter()
                    .any(|t| t.to_lowercase().starts_with("client:"));
                if any_client_tagged {
                    let belongs = frame_tags.iter().any(|t| {
                        let tl = t.to_lowercase();
                        module_client_tags.iter().any(|mc| &tl == mc)
                    });
                    if !belongs {
                        continue;
                    }
                }
            }

            if let Some(content) = brain.get(did) {
                let clean = content.trim_start_matches('\u{FEFF}').to_string();
                let upper = clean.to_uppercase();

                // ALL tables (needed for FK references). Matches two title
                // conventions:
                //   - `(table)` / `create_table` â€” AST-chunked titles from
                //     `said add-dir` ingest.
                //   - forge-sync titles are path-based and don't carry these
                //     markers; fall back to a content sniff. A frame is a
                //     table file if it lives under `<schema>/Tables/...` AND
                //     the content contains `CREATE TABLE`. Without this
                //     branch, the sandbox builder emits 0 tables because
                //     forge-sync'd schemas never match the marker check.
                let is_forge_sync_table = did.starts_with("forge-sync:")
                    && did.contains("/Tables/")
                    && contains_sql_keyword_pair(&upper, "CREATE", "TABLE");
                if title.contains("create_table") || title.contains("(table)") || is_forge_sync_table {
                    // Normalize table name. Two doc_id conventions:
                    //   - AST-chunked:  `path::NAME::kind:line` â€” take the
                    //                   NAME segment between the `::`.
                    //   - forge-sync:   `forge-sync:1-ground-truth/.../Tables/<file>.sql`
                    //                   (no `::`) â€” extract the file stem
                    //                   `<file>` (e.g. `ana_Acc_No_Alloc`).
                    // The earlier code path (`rsplit('.').next()`) returned
                    // the extension `"sql"` for every forge-sync table,
                    // collapsing all 363 tables into one bucket after dedup.
                    let tname = if let Some(pos) = did.find("::") {
                        let after_path = &did[pos+2..];
                        let chunk_name = after_path.split("::").next().unwrap_or(after_path);
                        let no_brackets = chunk_name.replace(['[', ']'], "");
                        no_brackets.rsplit('.').next().unwrap_or(&no_brackets).to_uppercase()
                    } else {
                        // forge-sync path â€” file stem of the SQL file
                        let last_seg = did.rsplit('/').next().unwrap_or(did);
                        let stem = last_seg.rsplit('.').nth(1).unwrap_or(last_seg);
                        stem.to_uppercase()
                    };

                    // FK refs from tags (format: "fk:TARGET_TABLE" or "fk:schema.TARGET")
                    let tags = brain.frames.get_meta(did)
                        .map(|m| m.tags.clone()).unwrap_or_default();
                    let fk_refs: Vec<String> = tags.iter()
                        .filter(|t| t.starts_with("fk:"))
                        .map(|t| {
                            let raw = t[3..].replace(['[', ']'], "");
                            raw.rsplit('.').next().unwrap_or(&raw).to_uppercase()
                        }).collect();

                    // Also extract FK refs from content (catches inline REFERENCES)
                    let content_fks: Vec<String> = clean.lines()
                        .filter(|l| l.to_uppercase().contains("REFERENCES"))
                        .filter_map(|l| {
                            let u = l.to_uppercase();
                            u.find("REFERENCES").map(|pos| {
                                let after = u[pos+10..].trim();
                                let ident = after.split(&['(', ' ', '\n', '\t'][..]).next().unwrap_or("");
                                let no_b = ident.replace(['[', ']'], "");
                                no_b.rsplit('.').next().unwrap_or(&no_b).trim().to_string()
                            }).filter(|s| !s.is_empty())
                        }).collect();

                    let mut all_fks = fk_refs;
                    all_fks.extend(content_fks);
                    all_fks.sort();
                    all_fks.dedup();

                    tables.push((tname, did.clone(), clean, all_fks));

                // ALL functions (tables need them for DEFAULT constraints).
                // Forge-sync fallback mirrors the table branch above â€”
                // path-pattern + content sniff for functions whose titles
                // are paths rather than `(function)` markers.
                } else if title.contains("create_function")
                    || title.contains("(function)")
                    || (did.starts_with("forge-sync:")
                        && did.contains("/Functions/")
                        && contains_sql_keyword_pair(&upper, "CREATE", "FUNCTION"))
                {
                    // Placeholder â€” deps computed in a second pass once we know ALL function names
                    functions.push((did.clone(), clean, Vec::<String>::new()));

                // ALL triggers (from brain â€” includes those embedded in table files).
                // Forge-sync fallback: triggers usually live in the same file
                // as their parent table (Tables/<x>.sql) â€” handled by the
                // table branch above. Standalone trigger files use `Triggers/`.
                } else if title.contains("create_trigger")
                    || title.contains("(trigger)")
                    || (did.starts_with("forge-sync:")
                        && did.contains("/Triggers/")
                        && contains_sql_keyword_pair(&upper, "CREATE", "TRIGGER"))
                {
                    triggers.push((did.clone(), clean));

                // Module-specific procs only
                } else if (title.contains("create_procedure") || title.contains("(proc)")) && is_module {
                    procs.push((did.clone(), clean));

                // forge-sync frames are whole-file (no AST chunking), so
                // their titles are paths rather than `(proc)` / `create_
                // procedure` markers. Detect proc files by path pattern
                // + content sniff. Without this branch, hand-written
                // procs added to `1-ground-truth/` after `forge sync`
                // never reach the snapshot â€” only the running container
                // sees them via `forge docs --verify-against-sandbox`'s
                // auto-deploy. The path pattern is the dt convention
                // (`<schema>/Stored Procedures/<file>.sql`); the content
                // sniff guarantees we only pick up actual procs (not
                // tables, triggers, etc. that happen to live nearby).
                } else if did.starts_with("forge-sync:")
                    && did.contains("/Stored Procedures/")
                    && contains_sql_keyword_pair(&upper, "CREATE", "PROCEDURE")
                    && is_module
                {
                    procs.push((did.clone(), clean));

                // ALL views. Forge-sync fallback covers `Views/<x>.sql`.
                } else if title.contains("create_view")
                    || title.contains("(view)")
                    || (did.starts_with("forge-sync:")
                        && did.contains("/Views/")
                        && contains_sql_keyword_pair(&upper, "CREATE", "VIEW"))
                {
                    views.push((did.clone(), clean));

                // MERGE-style lookup-data (sqlMasterDataTxnGlobal flat
                // `Data/<table>.sql` layout). Classified BEFORE the
                // INSERT branch because some MERGE files also contain
                // INSERT statements as part of the seed payload.
                // Keyed by table name (the file stem) so we can later
                // re-order by data-index.json.
                } else if upper.contains("MERGE INTO") {
                    // Extract the lookup table name from the MERGE-data
                    // frame doc_id. dt convention: filename matches the
                    // target table (e.g. `cps_Cardholder_Profile_Status.sql`).
                    let path_only = did.split("::").next().unwrap_or(did);
                    let last_seg = path_only.rsplit('/').next().unwrap_or(path_only);
                    let stem = last_seg.rsplit('.').nth(1).unwrap_or(last_seg);
                    let table_key = stem.to_lowercase();
                    // Compute manifest scope: drop the filename + any
                    // trailing `/Data` segment, then climb to the parent
                    // dir whose data-index.json should own this file.
                    // Example doc_id:
                    //   1-ground-truth/TXN/sqlMasterDataTxnGlobal/src/Data/cps_*.sql
                    //   â†’ scope = `1-ground-truth/TXN/sqlMasterDataTxnGlobal/src`
                    let scope = {
                        let dir = path_only.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                        // Drop trailing `/Data` so the scope matches the
                        // dir holding `data-index.json` (one level up).
                        let dir_lower = dir.to_lowercase();
                        if dir_lower.ends_with("/data") {
                            dir[..dir.len() - 5].to_string()
                        } else {
                            dir.to_string()
                        }
                    };

                    // Strip per-file `WITH CHECK CHECK CONSTRAINT ALL`
                    // lines so they don't re-validate against still-empty
                    // FK targets. Also strip `ALTER TABLE â€¦ DISABLE/ENABLE
                    // TRIGGER <Name>` lines that name specific triggers â€”
                    // when a trigger fails to deploy (16 source-bug errors
                    // in schema.sql), these calls error with Msg 4920 and
                    // abort the batch before MERGE runs. We disable+enable
                    // ALL triggers via the per-file prologue/epilogue
                    // instead, which is `IF EXISTS`-guarded.
                    let rewritten: String = clean
                        .lines()
                        .filter(|l| {
                            let u = l.to_uppercase();
                            if u.contains("WITH CHECK CHECK CONSTRAINT ALL") {
                                return false;
                            }
                            // Drop named-trigger ALTERs but keep
                            // `ALTER TABLE â€¦ NOCHECK CONSTRAINT ALL`.
                            let is_named_trigger_alter =
                                (u.contains("DISABLE TRIGGER") || u.contains("ENABLE TRIGGER"))
                                && !u.contains(" ALL");
                            !is_named_trigger_alter
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    // Store the cleaned MERGE body for per-file emission.
                    // Each file gets its own sqlcmd invocation in run.sh
                    // so session-state (IDENTITY_INSERT, NOCHECK, etc.)
                    // can't leak from one MERGE to the next. Keyed by
                    // (scope, table) so two clients' MERGEs for the
                    // same lookup table can co-exist in one brain.
                    data_by_table.insert(
                        (scope, table_key),
                        format!("-- {}\n{}\n", did, rewritten),
                    );

                // Seed data (INSERT statements)
                } else if upper.contains("INSERT INTO") || upper.starts_with("INSERT ") {
                    seed_parts.push(format!("-- {}\n{}\nGO\n", did, clean));
                }
            }

            // Parse data-index.json if present. Each manifest scopes to
            // its own client folder â€” keyed by the dir holding the .json
            // so siblings (e.g. TXN/.../src/data-index.json and
            // Vivere/.../src/data-index.json) don't interleave.
            if did.to_lowercase().ends_with("data-index.json") {
                if let Some(content) = brain.get(did) {
                    let clean = content.trim_start_matches('\u{FEFF}');
                    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(clean) {
                        let path_only = did.split("::").next().unwrap_or(did);
                        let scope = path_only
                            .rsplit_once('/')
                            .map(|(p, _)| p.to_string())
                            .unwrap_or_default();
                        let mut order: Vec<String> = Vec::new();
                        for entry in arr {
                            if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
                                order.push(name.to_lowercase());
                            }
                        }
                        if !order.is_empty() {
                            data_index_orders.insert(scope, order);
                        }
                    }
                }
            }
        }

        // Build the set of ALL function names (normalized â€” no schema prefix).
        // Generic: works for any naming convention (F_*, fn_*, CamelCase, etc.)
        // Doc_id layout is `path::NAME::kind:line` (kind:line appended by init
        // to keep doc_ids unique when a file has multiple chunks sharing a name,
        // e.g. CREATE TABLE + ALTER TABLE on the same object). We want NAME.
        let normalize_name = |did: &str| -> String {
            // forge-sync frames are whole-file (no `::` chunking); the
            // identifier we want is the file stem. Without this branch
            // every forge-sync `.sql` proc normalises to "SQL" (the
            // file extension), causing every proc beyond the first to
            // collide-and-skip in the `emitted_procs` dedup set.
            if did.starts_with("forge-sync:") && did.ends_with(".sql") && !did.contains("::") {
                let after_proto = &did["forge-sync:".len()..];
                let last_seg = after_proto.rsplit('/').next().unwrap_or(after_proto);
                let stem = last_seg.strip_suffix(".sql").unwrap_or(last_seg);
                return stem.to_uppercase();
            }
            // Split off the path prefix, then take the first component after â€”
            // which is the chunk name.
            let after_path = if let Some(pos) = did.find("::") {
                &did[pos+2..]
            } else { did };
            let chunk_name = after_path.split("::").next().unwrap_or(after_path);
            let no_brackets = chunk_name.replace(['[', ']'], "");
            let core = no_brackets.rsplit('.').next().unwrap_or(&no_brackets);
            core.to_uppercase()
        };

        let func_names: std::collections::HashSet<String> = functions.iter()
            .map(|(did, _, _)| normalize_name(did))
            .collect();

        // Compute deps generically: for each function, find which OTHER known function
        // names appear as whole-word references in its body. Ignore references within
        // string literals and comments (best-effort) and self-refs.
        let strip_sql_noise = |src: &str| -> String {
            let mut out = String::with_capacity(src.len());
            let bytes = src.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                // Line comment
                if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i+1] == b'-' {
                    while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                    continue;
                }
                // Block comment (nested)
                if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i+1] == b'*' {
                    let mut depth = 1; i += 2;
                    while i + 1 < bytes.len() && depth > 0 {
                        if bytes[i] == b'/' && bytes[i+1] == b'*' { depth += 1; i += 2; }
                        else if bytes[i] == b'*' && bytes[i+1] == b'/' { depth -= 1; i += 2; }
                        else { i += 1; }
                    }
                    continue;
                }
                // String literal
                if bytes[i] == b'\'' {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == b'\'' {
                            if i + 1 < bytes.len() && bytes[i+1] == b'\'' { i += 2; continue; }
                            i += 1; break;
                        }
                        i += 1;
                    }
                    continue;
                }
                out.push(bytes[i] as char);
                i += 1;
            }
            out.to_uppercase()
        };

        // Compute functionâ†’function dependencies (which other functions each
        // function calls). SQL Server's deferred name resolution covers most
        // table references inside function bodies, so we keep all functions
        // in one pre-table bucket.
        let functions: Vec<(String, String, Vec<String>)> = functions.into_iter()
            .map(|(did, content, _)| {
                let self_name = normalize_name(&did);
                let stripped = strip_sql_noise(&content);
                let mut deps: Vec<String> = Vec::new();
                for token in stripped.split(|c: char| {
                    !(c.is_ascii_alphanumeric() || c == '_')
                }) {
                    if token.len() >= 2 && func_names.contains(token) && token != self_name {
                        deps.push(token.to_string());
                    }
                }
                deps.sort();
                deps.dedup();
                (did, content, deps)
            })
            .collect();

        let mut ordered_functions: Vec<(String, String)> = Vec::new();
        let mut func_placed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut func_remaining = functions;

        let max_rounds = func_remaining.len().max(50);
        for _round in 0..max_rounds {
            if func_remaining.is_empty() { break; }
            let prev = func_remaining.len();
            let mut still = Vec::new();
            for (did, content, deps) in func_remaining {
                let name = normalize_name(&did);
                let deps_ok = deps.iter().all(|d| {
                    func_placed.contains(d) || !func_names.contains(d) || *d == name
                });
                if deps_ok {
                    func_placed.insert(name);
                    ordered_functions.push((did, content));
                } else {
                    still.push((did, content, deps));
                }
            }
            if still.len() == prev {
                for (did, content, _) in still { ordered_functions.push((did, content)); }
                break;
            }
            func_remaining = still;
        }

        // Topological sort: tables with no FK deps first, then tables
        // referencing only already-created tables. Repeat until all placed.
        let table_names: std::collections::HashSet<String> = tables.iter()
            .map(|(name, _, _, _)| name.clone()).collect();

        let mut ordered_tables: Vec<(String, String, String)> = Vec::new(); // (name, did, content)
        let mut placed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut remaining: Vec<(String, String, String, Vec<String>)> = tables;

        // Multiple passes â€” each pass places tables whose FK deps are satisfied
        for _round in 0..50 {
            if remaining.is_empty() { break; }
            let prev_count = remaining.len();
            let mut still_remaining = Vec::new();
            for (name, did, content, fk_refs) in remaining {
                let deps_satisfied = fk_refs.iter().all(|dep| {
                    placed.contains(dep) || !table_names.contains(dep) || *dep == name
                });
                if deps_satisfied {
                    placed.insert(name.clone());
                    ordered_tables.push((name, did, content));
                } else {
                    still_remaining.push((name, did, content, fk_refs));
                }
            }
            if still_remaining.len() == prev_count {
                // Circular deps â€” just dump the rest
                for (name, did, content, _) in still_remaining {
                    ordered_tables.push((name, did, content));
                }
                break;
            }
            remaining = still_remaining;
        }

        // Generic dependency sort for views/triggers/procs.
        // Any SQL object may reference functions, tables, or other views.
        // We already know func_names + table_names. Build view_names too.
        let view_names: std::collections::HashSet<String> = views.iter()
            .map(|(did, _)| normalize_name(did))
            .collect();

        // Universe of all objects that MUST be created before dependents.
        // (functions and views are the ones that bind at create time in SQL Server;
        // tables are already topologically sorted above)
        let resolvable: std::collections::HashSet<String> = func_names
            .union(&view_names).cloned().collect::<std::collections::HashSet<_>>()
            .union(&table_names).cloned().collect();

        let compute_refs = |src: &str, self_name: &str| -> Vec<String> {
            let stripped = strip_sql_noise(src);
            let mut refs: Vec<String> = Vec::new();
            for token in stripped.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                if token.len() >= 2 && resolvable.contains(token) && token != self_name {
                    refs.push(token.to_string());
                }
            }
            refs.sort();
            refs.dedup();
            refs
        };

        // Sort views by dependency (view can reference functions + other views).
        // Functions are already placed above; this sort resolves inter-view refs.
        let mut ordered_views: Vec<(String, String)> = Vec::new();
        let mut view_placed: std::collections::HashSet<String> = func_placed.clone();
        // Tables will exist before views, so seed with table names.
        for n in &table_names { view_placed.insert(n.clone()); }

        let mut view_remaining: Vec<(String, String, Vec<String>)> = views.into_iter()
            .map(|(did, content)| {
                let name = normalize_name(&did);
                let deps = compute_refs(&content, &name);
                (did, content, deps)
            }).collect();

        let max_view_rounds = view_remaining.len().max(20);
        for _round in 0..max_view_rounds {
            if view_remaining.is_empty() { break; }
            let prev = view_remaining.len();
            let mut still = Vec::new();
            for (did, content, deps) in view_remaining {
                let name = normalize_name(&did);
                let deps_ok = deps.iter().all(|d| {
                    view_placed.contains(d) || !resolvable.contains(d) || *d == name
                });
                if deps_ok {
                    view_placed.insert(name);
                    ordered_views.push((did, content));
                } else {
                    still.push((did, content, deps));
                }
            }
            if still.len() == prev {
                for (did, content, _) in still { ordered_views.push((did, content)); }
                break;
            }
            view_remaining = still;
        }

        // Procs & triggers â€” SQL Server uses deferred name resolution inside
        // procedure bodies, so strict ordering isn't required for correctness.
        // We still sort them to minimize warnings on systems that bind eagerly.
        let sort_by_refs = |items: Vec<(String, String)>| -> Vec<(String, String)> {
            let mut placed_local = view_placed.clone();
            let mut out: Vec<(String, String)> = Vec::new();
            let mut rem: Vec<(String, String, Vec<String>)> = items.into_iter()
                .map(|(d, c)| {
                    let name = normalize_name(&d);
                    let deps = compute_refs(&c, &name);
                    (d, c, deps)
                }).collect();
            let max_r = rem.len().max(20);
            for _ in 0..max_r {
                if rem.is_empty() { break; }
                let prev = rem.len();
                let mut still = Vec::new();
                for (d, c, deps) in rem {
                    let name = normalize_name(&d);
                    let deps_ok = deps.iter().all(|x| {
                        placed_local.contains(x) || !resolvable.contains(x) || *x == name
                    });
                    if deps_ok { placed_local.insert(name); out.push((d, c)); }
                    else { still.push((d, c, deps)); }
                }
                if still.len() == prev {
                    for (d, c, _) in still { out.push((d, c)); }
                    break;
                }
                rem = still;
            }
            out
        };

        let procs = sort_by_refs(procs);
        let triggers = sort_by_refs(triggers);

        let func_count = ordered_functions.len();
        let table_count = ordered_tables.len();
        let proc_count = procs.len();
        let trigger_count = triggers.len();
        let view_count = ordered_views.len();

        // â”€â”€â”€ DEPENDENCY-CLOSURE CHECK â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // Walk every proc / function / trigger / view body and harvest
        // every cross-object reference (`EXEC <obj>`, `<schema>.<fn>(`,
        // `JOIN <table>`, `FROM <table>`). Cross-reference against the
        // set of objects we're about to deploy. Anything referenced but
        // not deployed is a "missing dep" â€” surface it loudly so the
        // operator knows the live sandbox WILL fail at runtime when a
        // proc tries to call something that isn't there.
        //
        // This is a deploy-time alternative to the runtime divergence
        // shown by `said forge test` (which only finds the gap when a
        // proc actually executes a call to the missing object).
        let mut deployed_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let push_name = |s: &str, set: &mut std::collections::HashSet<String>| {
            // Bare name (no schema), uppercased, brackets stripped.
            let no_b = s.replace(['[', ']'], "");
            let last = no_b.rsplit('.').next().unwrap_or(&no_b);
            if !last.is_empty() {
                set.insert(last.to_uppercase());
            }
        };
        for (name, _, _) in &ordered_tables {
            push_name(name, &mut deployed_names);
        }
        for (did, _) in &ordered_functions {
            push_name(&normalize_name(did), &mut deployed_names);
        }
        for (did, _) in &ordered_views {
            push_name(&normalize_name(did), &mut deployed_names);
        }
        for (did, _) in &procs {
            push_name(&normalize_name(did), &mut deployed_names);
        }
        for (did, _) in &triggers {
            push_name(&normalize_name(did), &mut deployed_names);
        }

        // SQL Server built-ins + dynamic-SQL pseudo-targets we should
        // NEVER report as missing. Lowercase / case-insensitive match.
        let sql_builtins: std::collections::HashSet<&str> = [
            "getdate", "getutcdate", "sysdatetime", "sysutcdatetime",
            "newid", "newsequentialid", "isnull", "coalesce", "nullif",
            "cast", "convert", "try_cast", "try_convert", "parse",
            "try_parse", "format", "str", "ascii", "char", "unicode",
            "nchar", "len", "datalength", "lower", "upper", "ltrim",
            "rtrim", "trim", "left", "right", "substring", "replace",
            "replicate", "reverse", "stuff", "patindex", "charindex",
            "abs", "ceiling", "floor", "round", "sqrt", "power", "sign",
            "rand", "pi", "exp", "log", "log10",
            "concat", "concat_ws", "quotename", "space", "soundex",
            "difference", "translate", "hashbytes", "binary_checksum",
            "checksum", "bytecount",
            "dateadd", "datediff", "datepart", "datename", "day", "month",
            "year", "eomonth", "isdate", "todatetimeoffset", "switchoffset",
            "object_id", "object_name", "schema_id", "schema_name",
            "user_id", "user_name", "suser_name", "suser_id",
            "db_id", "db_name", "host_name", "host_id", "scope_identity",
            "ident_current", "identity",
            "sum", "avg", "min", "max", "count", "count_big",
            "string_agg", "string_split", "string_escape",
            "row_number", "rank", "dense_rank", "ntile", "lag", "lead",
            "first_value", "last_value", "percent_rank",
            "openjson", "openxml", "openrowset", "openquery",
            "json_value", "json_query", "json_modify", "isjson",
            "error_number", "error_message", "error_severity",
            "error_state", "error_line", "error_procedure",
            "raiserror", "throw",
            "@@identity", "@@rowcount", "@@error", "@@trancount",
            "@@servername", "@@version", "@@language",
            // Pseudo-tables.
            "inserted", "deleted",
            // Common system schemas.
            "sys", "information_schema",
        ].iter().copied().collect();

        let strip_sql_noise_for_refs = |src: &str| -> String {
            // Re-use the same comment / string-literal stripper logic
            // we use for function-dep sorting so identifier matching
            // doesn't fire on `'... EXEC ...'` inside a string.
            let mut out = String::with_capacity(src.len());
            let bytes = src.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() {
                if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    i = (i + 2).min(bytes.len());
                    out.push(' ');
                    continue;
                }
                if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                if bytes[i] == b'\'' {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == b'\'' {
                            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                                i += 2;
                                continue;
                            }
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                    out.push(' ');
                    continue;
                }
                out.push(bytes[i] as char);
                i += 1;
            }
            out
        };

        // Harvest cross-object references from every deployed body.
        // `(missing_object, [list_of_callers])`.
        let mut missing_deps: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();

        let pluck_ident_after = |tail: &str| -> String {
            let tail = tail.trim_start();
            let mut out = String::new();
            for ch in tail.chars() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '[' || ch == ']' {
                    out.push(ch);
                } else {
                    break;
                }
            }
            out
        };

        let object_markers: &[&str] = &[
            "EXEC ", "EXECUTE ", "FROM ", "JOIN ", "INTO ",
            "UPDATE ", "DELETE FROM ", "MERGE INTO ",
        ];

        let scan_body = |caller_label: &str, body: &str,
                             missing: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<String>>| {
            let stripped = strip_sql_noise_for_refs(body);
            let upper = stripped.to_uppercase();
            // Statement-marker references (EXEC, JOIN, FROM, â€¦).
            for marker in object_markers {
                let mut from = 0usize;
                while let Some(rel) = upper[from..].find(marker) {
                    let pos = from + rel + marker.len();
                    let ident = pluck_ident_after(&stripped[pos..]);
                    from = pos;
                    if ident.is_empty() {
                        continue;
                    }
                    let no_b = ident.replace(['[', ']'], "");
                    let last = no_b.rsplit('.').next().unwrap_or(&no_b);
                    let last_upper = last.to_uppercase();
                    if last_upper.len() < 3 {
                        continue;
                    }
                    if sql_builtins.contains(last_upper.to_lowercase().as_str()) {
                        continue;
                    }
                    if deployed_names.contains(&last_upper) {
                        continue;
                    }
                    // Skip SQL keywords masquerading as identifiers.
                    if matches!(last_upper.as_str(),
                        // SQL keywords masquerading as identifiers.
                        "WITH" | "WHERE" | "GROUP" | "ORDER" | "HAVING" | "UNION"
                        | "INNER" | "LEFT" | "RIGHT" | "OUTER" | "CROSS"
                        | "TABLE" | "VIEW" | "PROCEDURE" | "FUNCTION" | "TRIGGER"
                        | "AS" | "ON" | "AND" | "OR" | "NOT" | "NULL" | "IS" | "IN"
                        | "EXISTS" | "BETWEEN" | "LIKE" | "TOP" | "DISTINCT"
                        | "BEGIN" | "END" | "RETURN" | "DECLARE" | "SET" | "VALUES"
                        | "CASE" | "WHEN" | "THEN" | "ELSE" | "WHILE" | "BREAK"
                        | "CONTINUE" | "TRY" | "CATCH" | "ROLLBACK" | "COMMIT"
                        | "TRAN" | "TRANSACTION" | "OUTPUT" | "READONLY" | "DEFAULT"
                        // SQL type names that show up after `INTO @var TABLE(... TYPE)`.
                        | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "BIT" | "DECIMAL"
                        | "NUMERIC" | "FLOAT" | "REAL" | "MONEY" | "SMALLMONEY" | "DATE"
                        | "TIME" | "DATETIME" | "DATETIME2" | "DATETIMEOFFSET"
                        | "SMALLDATETIME" | "VARCHAR" | "NVARCHAR" | "CHARACTER"
                        | "VARYING" | "TEXT" | "NTEXT" | "BINARY" | "VARBINARY" | "IMAGE"
                        | "UNIQUEIDENTIFIER" | "XML" | "JSON" | "TIMESTAMP" | "ROWVERSION"
                        | "HIERARCHYID" | "GEOMETRY" | "GEOGRAPHY" | "SQL_VARIANT" | "CHAR"
                        // SQL Server system catalog tables/views â€” always present at runtime.
                        | "OBJECTS" | "COLUMNS" | "TABLES" | "INDEXES" | "PARAMETERS"
                        | "TRIGGERS" | "VIEWS" | "PROCEDURES" | "TYPES" | "SCHEMAS"
                        | "PARTITIONS" | "FOREIGN_KEYS" | "FOREIGN_KEY_COLUMNS"
                        | "INDEX_COLUMNS" | "CHECK_CONSTRAINTS" | "DEFAULT_CONSTRAINTS"
                        | "KEY_CONSTRAINTS" | "EXTENDED_PROPERTIES"
                        | "DATABASE_PERMISSIONS" | "DATABASE_PRINCIPALS" | "DATABASE_ROLES"
                        | "SERVER_PRINCIPALS" | "SERVER_ROLES" | "SQL_LOGINS"
                        | "KEY_COLUMN_USAGE" | "TABLE_CONSTRAINTS" | "REFERENTIAL_CONSTRAINTS"
                        | "ROUTINES" | "ROUTINE_PARAMETERS" | "DM_EXEC_REQUESTS"
                        | "DM_EXEC_SESSIONS" | "DM_EXEC_CONNECTIONS"
                        | "DM_TRAN_ACTIVE_TRANSACTIONS" | "DM_TRAN_DATABASE_TRANSACTIONS"
                        | "SYSPROCESSES" | "SYSUSERS" | "SYSOBJECTS" | "SYSCOLUMNS"
                        | "SP_EXECUTESQL" | "SP_RENAME" | "SP_HELPTEXT"
                        | "SP_ADDROLEMEMBER" | "SP_DROPROLEMEMBER" | "SP_DBCMPTLEVEL") {
                        continue;
                    }
                    missing.entry(last_upper)
                        .or_insert_with(std::collections::BTreeSet::new)
                        .insert(caller_label.to_string());
                }
            }
            // Function-call references: `<name>(`.
            let bytes = stripped.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() {
                if bytes[i] == b'(' {
                    let end = i;
                    let mut start = i;
                    while start > 0 {
                        let c = bytes[start - 1];
                        if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c == b'[' || c == b']' {
                            start -= 1;
                        } else {
                            break;
                        }
                    }
                    if end > start {
                        let ident = &stripped[start..end];
                        let no_b = ident.replace(['[', ']'], "");
                        let last = no_b.rsplit('.').next().unwrap_or(&no_b);
                        let last_upper = last.to_uppercase();
                        if last_upper.len() >= 4
                            && !deployed_names.contains(&last_upper)
                            && !sql_builtins.contains(last_upper.to_lowercase().as_str())
                            // Skip self-references: a proc body can mention
                            // itself in comments / recursive call paths.
                            && last_upper != caller_label.to_uppercase()
                            // Heuristic: dt UDF naming.
                            && (last_upper.starts_with("F_")
                                || last_upper.starts_with("FN_")
                                || last_upper.starts_with("UDF_")
                                || last_upper.starts_with("P_DTE_")
                                || last_upper.starts_with("P_TXN_"))
                        {
                            missing.entry(last_upper)
                                .or_insert_with(std::collections::BTreeSet::new)
                                .insert(caller_label.to_string());
                        }
                    }
                }
                i += 1;
            }
        };

        for (did, content) in &procs {
            scan_body(&normalize_name(did), content, &mut missing_deps);
        }
        for (did, content) in &triggers {
            scan_body(&normalize_name(did), content, &mut missing_deps);
        }
        for (did, content) in &ordered_functions {
            scan_body(&normalize_name(did), content, &mut missing_deps);
        }
        for (did, content) in &ordered_views {
            scan_body(&normalize_name(did), content, &mut missing_deps);
        }

        // Render the report. Always written to a sibling file even
        // when empty, so an absent file means "deploy never ran" and
        // an empty file means "deploy ran, no missing deps".
        let dep_report_lines: Vec<String> = missing_deps
            .iter()
            .map(|(name, callers)| {
                let preview: Vec<&str> = callers.iter().take(5).map(|s| s.as_str()).collect();
                let extra = if callers.len() > 5 {
                    format!(" (+ {} more)", callers.len() - 5)
                } else {
                    String::new()
                };
                format!("  {} â† {}{}", name, preview.join(", "), extra)
            })
            .collect();
        let dep_report = if missing_deps.is_empty() {
            "-- No missing dependencies â€” every cross-object reference resolves to a deployed object.\n".to_string()
        } else {
            format!(
                "-- âš  {} cross-object reference{} unresolved in the deploy set.\n\
                 -- These will fail at proc execution time (e.g. `Could not find\n\
                 -- stored procedure 'X'`). Add the missing object's source file\n\
                 -- to 1-ground-truth/<client>/ so it gets ingested + deployed.\n\
                 --\n\
                 -- Missing object â† caller(s):\n\
                 {}\n",
                missing_deps.len(),
                if missing_deps.len() == 1 { "" } else { "s" },
                dep_report_lines.join("\n"),
            )
        };
        let dep_report_path = sandbox_dir_pb.join("missing-deps.txt");
        let _ = std::fs::write(&dep_report_path, &dep_report);

        // Build schema.sql in correct order: functions â†’ tables â†’ views â†’ procs â†’ triggers
        let mut schema = format!(
            "-- =============================================\n\
             -- {} Module Sandbox Schema\n\
             -- Auto-generated by said sandbox\n\
             -- Functions: {}, Tables: {} (FK-ordered), Views: {}, Procs: {}, Triggers: {}\n\
             -- =============================================\n\
             SET QUOTED_IDENTIFIER ON;\n\
             GO\n\
             SET ANSI_NULLS ON;\n\
             GO\n\
             SET ANSI_PADDING ON;\n\
             GO\n\
             SET CONCAT_NULL_YIELDS_NULL ON;\n\
             GO\n\n",
            module, func_count, table_count, view_count, proc_count, trigger_count
        );

        // CREATE SCHEMA â€” every non-dbo schema referenced by any table /
        // proc / function / view / trigger needs to exist before its
        // objects do, otherwise CREATE TABLE [lookups].[â€¦] fails with
        // Msg 2714. Scan all collected object content for schema-qualified
        // identifiers and emit IF SCHEMA_ID(...) IS NULL CREATE SCHEMA
        // for each unique non-dbo schema seen.
        {
            let mut schemas: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            let mut scan = |body: &str, marker: &str| {
                let upper = body.to_uppercase();
                let mut from = 0usize;
                while let Some(rel) = upper[from..].find(marker) {
                    let pos = from + rel + marker.len();
                    let tail = body[pos..].trim_start();
                    let mut ident = String::new();
                    for ch in tail.chars() {
                        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '[' || ch == ']' {
                            ident.push(ch);
                        } else {
                            break;
                        }
                    }
                    let no_brackets = ident.replace(['[', ']'], "");
                    if let Some(dot) = no_brackets.find('.') {
                        let schema_part = &no_brackets[..dot];
                        if !schema_part.is_empty()
                            && !schema_part.eq_ignore_ascii_case("dbo")
                            && !schema_part.eq_ignore_ascii_case("sys")
                        {
                            schemas.insert(schema_part.to_string());
                        }
                    }
                    from = pos;
                }
            };
            for (_, _, content) in &ordered_tables {
                scan(content, "CREATE TABLE ");
            }
            for (_, content) in &ordered_functions {
                scan(content, "CREATE FUNCTION ");
                scan(content, "CREATE OR ALTER FUNCTION ");
            }
            for (_, content) in &ordered_views {
                scan(content, "CREATE VIEW ");
                scan(content, "CREATE OR ALTER VIEW ");
            }
            for (_, content) in &procs {
                scan(content, "CREATE PROCEDURE ");
                scan(content, "CREATE OR ALTER PROCEDURE ");
                scan(content, "CREATE PROC ");
            }
            for (_, content) in &triggers {
                scan(content, "CREATE TRIGGER ");
                scan(content, "CREATE OR ALTER TRIGGER ");
            }
            // MERGE-style data scripts may target schema-qualified
            // tables (e.g. TXN uses `[lookups].*`). Inspect each MERGE
            // body for its actual target schema and add it. Vivere uses
            // bare table names so nothing extra gets added there.
            for body in data_by_table.values() {
                let upper = body.to_uppercase();
                if let Some(pos) = upper.find("MERGE INTO ") {
                    let after = &body[pos + "MERGE INTO ".len()..].trim_start();
                    let mut ident = String::new();
                    for ch in after.chars() {
                        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '[' || ch == ']' {
                            ident.push(ch);
                        } else {
                            break;
                        }
                    }
                    let no_brackets = ident.replace(['[', ']'], "");
                    if let Some(dot) = no_brackets.find('.') {
                        let s = &no_brackets[..dot];
                        if !s.is_empty()
                            && !s.eq_ignore_ascii_case("dbo")
                            && !s.eq_ignore_ascii_case("sys")
                        {
                            schemas.insert(s.to_string());
                        }
                    }
                }
            }
            if !schemas.is_empty() {
                schema.push_str("-- â•â•â• SCHEMAS (idempotent CREATE) â•â•â•\n");
                for s in &schemas {
                    schema.push_str(&format!(
                        "IF SCHEMA_ID('{name}') IS NULL EXEC('CREATE SCHEMA [{name}]');\nGO\n",
                        name = s
                    ));
                }
                schema.push('\n');
            }
        }

        // De-dup emitted objects by normalized name per object kind. A monolith
        // sometimes has two source files for the same object (e.g. "foo.sql" +
        // "foo_1.sql"), and emitting both causes Msg 2714 "already exists".
        // Each kind has its own namespace in SQL Server (table vs function vs
        // view etc.), so we track them separately.
        let mut emitted_functions: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut emitted_tables: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut emitted_views: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut emitted_procs: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut emitted_triggers: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut emitted_fk_names: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Functions FIRST (dependency-sorted) â€” tables need them for DEFAULT
        // constraints. SQL Server's deferred name resolution lets function
        // bodies reference tables that don't yet exist.
        if !ordered_functions.is_empty() {
            schema.push_str("-- â•â•â• FUNCTIONS (dependency-sorted, loaded first) â•â•â•\n\n");
            for (did, content) in &ordered_functions {
                let name = normalize_name(did);
                if !emitted_functions.insert(name) { continue; }
                schema.push_str(&format!("-- {}\n{}\nGO\n\n", did, content));
            }
        }

        // PASS 1: Tables WITHOUT FK constraints (avoids circular dependency errors)
        schema.push_str("-- â•â•â• PASS 1: TABLES (without FK constraints) â•â•â•\n\n");
        let mut fk_alters: Vec<String> = Vec::new();

        for (name, did, content) in &ordered_tables {
            if !emitted_tables.insert(name.clone()) { continue; }
            // Strip CONSTRAINT ... FOREIGN KEY lines from CREATE TABLE
            let mut clean_lines: Vec<&str> = Vec::new();
            let mut fk_lines: Vec<String> = Vec::new();
            let table_name_for_alter = {
                // Extract [dbo].[table_name] from CREATE TABLE line
                let first_line = content.lines().next().unwrap_or("");
                let upper = first_line.to_uppercase();
                if let Some(pos) = upper.find("CREATE TABLE") {
                    let after = &first_line[pos + 12..].trim();
                    let end = after.find('(').unwrap_or(after.len());
                    after[..end].trim().to_string()
                } else {
                    String::new()
                }
            };

            for line in content.lines() {
                let trimmed = line.trim().to_uppercase();
                if trimmed.contains("FOREIGN KEY") && trimmed.contains("REFERENCES") {
                    // Extract the full constraint for ALTER TABLE
                    let constraint_line = line.trim().trim_end_matches(',');
                    if !table_name_for_alter.is_empty() {
                        fk_lines.push(format!(
                            "ALTER TABLE {} ADD {};\nGO\n",
                            table_name_for_alter, constraint_line
                        ));
                    }
                } else if trimmed.starts_with("CONSTRAINT") && trimmed.contains("FOREIGN KEY") {
                    let constraint_line = line.trim().trim_end_matches(',');
                    if !table_name_for_alter.is_empty() {
                        fk_lines.push(format!(
                            "ALTER TABLE {} ADD {};\nGO\n",
                            table_name_for_alter, constraint_line
                        ));
                    }
                } else {
                    clean_lines.push(line);
                }
            }

            // Remove trailing comma from last column before closing paren
            let clean_content = clean_lines.join("\n");
            schema.push_str(&format!("-- {}\n{}\nGO\n\n", did, clean_content));
            fk_alters.extend(fk_lines);
        }

        // PASS 2: Add FK constraints via ALTER TABLE (deduped by constraint name)
        if !fk_alters.is_empty() {
            schema.push_str(&format!(
                "-- â•â•â• PASS 2: FOREIGN KEY CONSTRAINTS â•â•â•\n\n"
            ));
            let mut deduped = 0usize;
            for alter in &fk_alters {
                // Extract "CONSTRAINT [name]" or "CONSTRAINT name" from the ALTER
                let upper = alter.to_uppercase();
                let fk_name = if let Some(pos) = upper.find("CONSTRAINT") {
                    let after = &alter[pos + 10..].trim_start();
                    let tok = after.split(&[' ', '\n', '\t', '('][..]).next().unwrap_or("");
                    tok.replace(['[', ']'], "").to_uppercase()
                } else {
                    String::new()
                };
                if !fk_name.is_empty() && !emitted_fk_names.insert(fk_name) {
                    continue;
                }
                schema.push_str(alter);
                schema.push('\n');
                deduped += 1;
            }
            schema.push_str(&format!("-- ({} constraints emitted)\n\n", deduped));
        }

        // POST-TABLE RE-CREATE for inline table-valued functions (iTVF).
        // iTVFs have the form `RETURNS TABLE AS RETURN (...)` and SQL Server
        // binds their body strictly at CREATE time â€” they will have failed
        // in the pre-table emission above if they reference base tables.
        // Re-emit them via CREATE OR ALTER now that tables exist.
        let itvf_retries: Vec<&(String, String)> = ordered_functions.iter()
            .filter(|(_, content)| {
                // Match `RETURNS TABLE` followed by `AS` then `RETURN(` with
                // arbitrary whitespace / newlines between â€” this is the iTVF
                // shape. Multi-statement TVFs use `RETURNS @var TABLE(...)`
                // and don't bind strictly, so exclude them.
                let u = content.to_uppercase();
                let norm: String = u.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
                let collapsed: String = norm.split_whitespace().collect::<Vec<_>>().join(" ");
                collapsed.contains("RETURNS TABLE AS RETURN")
                    || collapsed.contains("RETURNS TABLE AS RETURN(")
            })
            .collect();
        if !itvf_retries.is_empty() {
            schema.push_str("-- â•â•â• POST-TABLE FUNCTION RECREATE (inline TVFs) â•â•â•\n\n");
            for (did, content) in &itvf_retries {
                // Swap leading CREATE FUNCTION â†’ CREATE OR ALTER FUNCTION so
                // this idempotently fixes any body that failed to bind before.
                let upper = content.to_uppercase();
                let rewritten = if let Some(pos) = upper.find("CREATE FUNCTION") {
                    let mut s = String::with_capacity(content.len() + 16);
                    s.push_str(&content[..pos]);
                    s.push_str("CREATE OR ALTER FUNCTION");
                    s.push_str(&content[pos + "CREATE FUNCTION".len()..]);
                    s
                } else {
                    (*content).clone()
                };
                schema.push_str(&format!("-- {}\n{}\nGO\n\n", did, rewritten));
            }
        }

        if !ordered_views.is_empty() {
            schema.push_str("-- â•â•â• VIEWS (dependency-sorted) â•â•â•\n\n");
            for (did, content) in &ordered_views {
                let name = normalize_name(did);
                if !emitted_views.insert(name) { continue; }
                schema.push_str(&format!("-- {}\n{}\nGO\n\n", did, content));
            }
        }

        if !procs.is_empty() {
            schema.push_str("-- â•â•â• STORED PROCEDURES â•â•â•\n\n");
            for (did, content) in &procs {
                let name = normalize_name(did);
                if !emitted_procs.insert(name) { continue; }
                schema.push_str(&format!("-- {}\n{}\nGO\n\n", did, content));
            }
        }

        if !triggers.is_empty() {
            schema.push_str("-- â•â•â• TRIGGERS â•â•â•\n\n");
            for (did, content) in &triggers {
                let name = normalize_name(did);
                if !emitted_triggers.insert(name) { continue; }
                schema.push_str(&format!("-- {}\n{}\nGO\n\n", did, content));
            }
        }

        let schema_path = sandbox_dir_pb.join("schema.sql");
        if let Err(e) = std::fs::write(&schema_path, &schema) {
            return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                "âœ— Failed to write {} ({}).\n\n\
                 Common causes: the parent directory is read-only, disk full, \
                 or the path contains invalid characters for this filesystem.",
                schema_path.display(), e
            ))]));
        }

        // Write seed-data.sql â€” also scan frame content for INSERT statements
        // that might be embedded in table definition files
        for (_, _, content) in &ordered_tables {
            // Look for INSERT lines within table files
            for line in content.lines() {
                let line_upper = line.trim().to_uppercase();
                if line_upper.starts_with("INSERT INTO") || line_upper.starts_with("INSERT ") {
                    // Collect multi-line INSERT (until semicolon or empty line)
                    seed_parts.push(format!("{}\nGO\n", line.trim()));
                }
            }
        }

        if !seed_parts.is_empty() {
            let seed_content = format!(
                "-- Seed data for {} module ({} statements)\n\
                 -- Auto-generated by said sandbox\n\n{}",
                module, seed_parts.len(), seed_parts.join("\n")
            );
            let _ = std::fs::write(sandbox_dir_pb.join("seed-data.sql"), &seed_content);
        } else {
            // Create empty file so docker volume mount doesn't fail
            let _ = std::fs::write(sandbox_dir_pb.join("seed-data.sql"),
                "-- No seed data found\n");
        }

        // Emit each MERGE block to its own file under
        // sandbox/data/<NN>_<table>.sql so run.sh can run each via a
        // separate sqlcmd invocation. Fresh connections mean fresh
        // session state â€” no IDENTITY_INSERT poisoning between files.
        // The `<NN>` prefix preserves data-index.json dependency order
        // through alphabetical sqlcmd-driven shell loop iteration.
        let data_dir = sandbox_dir_pb.join("data");
        let _ = std::fs::create_dir_all(&data_dir);
        // Wipe any previous run's files (file count may have shrunk).
        if let Ok(entries) = std::fs::read_dir(&data_dir) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }

        let mut ordered_files: Vec<(String, String)> = Vec::new(); // (filename, body)
        let mut emitted: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        // Group MERGE entries by their best-matching manifest scope.
        // Longest-prefix-match wins so a MERGE under
        // `1-ground-truth/TXN/sqlMasterDataTxnGlobal/src/Data/<x>.sql`
        // matches the manifest at
        // `1-ground-truth/TXN/sqlMasterDataTxnGlobal/src/data-index.json`.
        let pick_scope = |entry_scope: &str| -> Option<String> {
            data_index_orders
                .keys()
                .filter(|k| entry_scope.starts_with(k.as_str()))
                .max_by_key(|k| k.len())
                .cloned()
        };
        // Stable client-grouping: emit each manifest's MERGE files in
        // its own block, manifests sorted alphabetically by scope so
        // the order is deterministic across runs.
        for (manifest_scope, order) in &data_index_orders {
            // Pass 1 (this manifest): files in declared order.
            for name in order {
                let key = (manifest_scope.clone(), name.clone());
                if let Some(body) = data_by_table.get(&key) {
                    ordered_files.push((name.clone(), body.clone()));
                    emitted.insert(key);
                }
            }
            // Pass 2 (this manifest): MERGE files that landed in this
            // scope but weren't named in the manifest. Alphabetical.
            for ((scope, name), body) in &data_by_table {
                if scope != manifest_scope {
                    continue;
                }
                if pick_scope(scope).as_deref() != Some(manifest_scope.as_str()) {
                    continue;
                }
                let key = (scope.clone(), name.clone());
                if !emitted.contains(&key) {
                    ordered_files.push((name.clone(), body.clone()));
                    emitted.insert(key);
                }
            }
        }
        // Pass 3: orphans (no manifest matched). Alphabetical.
        for ((scope, name), body) in &data_by_table {
            let key = (scope.clone(), name.clone());
            if emitted.contains(&key) {
                continue;
            }
            ordered_files.push((name.clone(), body.clone()));
        }

        // Per-file prologue + epilogue:
        //   - Disable FK + triggers on the target table only (so cross-
        //     table FK checks don't reject unseeded targets).
        //   - Strip per-file WITH CHECK CHECK CONSTRAINT lines (already
        //     done at frame collection).
        //   - Re-enable triggers (best-effort) at end. FK re-validation
        //     happens in 04-data-postcheck.sql after all MERGEs run.
        for (i, (table_name, body)) in ordered_files.iter().enumerate() {
            // Parse the actual MERGE target from the body so we don't
            // hardcode `[lookups].[X]` â€” Vivere uses bare table names,
            // TXN uses `[lookups].*`. Falls back to `[<filename>]` if
            // we can't find the explicit target.
            let target_tbl = {
                let upper = body.to_uppercase();
                upper.find("MERGE INTO ").and_then(|pos| {
                    let after = &body[pos + "MERGE INTO ".len()..].trim_start();
                    let mut ident = String::new();
                    for ch in after.chars() {
                        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '[' || ch == ']' {
                            ident.push(ch);
                        } else {
                            break;
                        }
                    }
                    if ident.is_empty() { None } else { Some(ident) }
                })
                .unwrap_or_else(|| format!("[{}]", table_name))
            };
            let file_body = format!(
                "-- Auto-generated by said sandbox: MERGE for {tbl}\n\
                 SET NOCOUNT ON\n\
                 GO\n\
                 BEGIN TRY ALTER TABLE {tbl} NOCHECK CONSTRAINT ALL; END TRY BEGIN CATCH END CATCH;\n\
                 BEGIN TRY ALTER TABLE {tbl} DISABLE TRIGGER ALL; END TRY BEGIN CATCH END CATCH;\n\
                 GO\n\
                 {body}\n\
                 GO\n\
                 BEGIN TRY SET IDENTITY_INSERT {tbl} OFF; END TRY BEGIN CATCH END CATCH;\n\
                 BEGIN TRY ALTER TABLE {tbl} ENABLE TRIGGER ALL; END TRY BEGIN CATCH END CATCH;\n\
                 GO\n",
                tbl = target_tbl,
                body = body,
            );
            let filename = format!("{:03}_{}.sql", i + 1, table_name);
            let _ = std::fs::write(data_dir.join(&filename), file_body);
        }

        // Write a run-all manifest (newline-separated filenames in load
        // order) so the bring-up script can iterate without globbing.
        let manifest: Vec<String> = ordered_files
            .iter()
            .enumerate()
            .map(|(i, (name, _))| format!("{:03}_{}.sql", i + 1, name))
            .collect();
        let _ = std::fs::write(data_dir.join("_manifest.txt"), manifest.join("\n"));

        // Legacy data.sql file kept as an empty placeholder so any
        // existing docker-compose mount configurations don't break.
        let _ = std::fs::write(
            sandbox_dir_pb.join("data.sql"),
            format!(
                "-- MERGE blocks emitted to per-file scripts under data/\n\
                 -- See data/_manifest.txt for load order ({} files).\n\
                 -- run.sh iterates each via a fresh sqlcmd invocation\n\
                 -- so IDENTITY_INSERT session state can't leak between blocks.\n",
                manifest.len()
            ),
        );

        // Write docker-compose.yml. Each sandbox has:
        //   - a unique `name:` (the compose project) so compose-up in one
        //     sandbox doesn't recreate containers in another,
        //   - a unique container_name,
        //   - a unique host port,
        //   - a unique volume path (absolute) so bind-mounts don't collide.
        // Together these let N sandboxes run at once on one Docker daemon.
        let project_name = format!("said-sbx-{}-{}", scoped_label.replace('+', "-"), port);
        let docker = format!(
"name: {project}
services:
  sqlserver:
    image: mcr.microsoft.com/mssql/server:2025-latest
    container_name: {container}
    environment:
      SA_PASSWORD: \"Said_Test_2026!\"
      ACCEPT_EULA: \"Y\"
      MSSQL_PID: \"Developer\"
    ports:
      - \"{port}:1433\"
    volumes:
      - ./schema.sql:/docker-entrypoint-initdb.d/01-schema.sql
      - ./seed-data.sql:/docker-entrypoint-initdb.d/02-seed.sql
      - ./data:/data
    healthcheck:
      test: /opt/mssql-tools18/bin/sqlcmd -S localhost -U sa -P \"Said_Test_2026!\" -C -Q \"SELECT 1\"
      interval: 10s
      timeout: 5s
      retries: 5

# Connection string:
# Server=localhost,{port};Database=master;User Id=sa;Password=Said_Test_2026!;TrustServerCertificate=True
",
            project = project_name, container = container_name, port = port
        );
        let compose_path = sandbox_dir_pb.join("docker-compose.yml");
        if let Err(e) = std::fs::write(&compose_path, &docker) {
            return Ok(CallToolResult::text_content(vec![TextContent::from(format!(
                "âœ— Failed to write {} ({}).", compose_path.display(), e
            ))]));
        }

        // Write run.sh â€” references the per-sandbox container name explicitly
        // so it works even when multiple sandboxes are up at the same time.
        let run_script = format!(
"#!/bin/bash
set -e
echo \"Starting {combo} sandbox on port {port}...\"
docker compose up -d
echo \"Waiting for SQL Server...\"
until docker exec {container} /opt/mssql-tools18/bin/sqlcmd \\
    -S localhost -U sa -P 'Said_Test_2026!' -C -Q 'SELECT 1' 2>/dev/null | grep -q '1 rows'; do
  sleep 2
done
echo \"Loading schema ({tables} tables, {procs} procedures)...\"
docker exec -i {container} /opt/mssql-tools18/bin/sqlcmd \\
  -S localhost -U sa -P 'Said_Test_2026!' -C -i /docker-entrypoint-initdb.d/01-schema.sql
echo \"Loading seed data...\"
docker exec -i {container} /opt/mssql-tools18/bin/sqlcmd \\
  -S localhost -U sa -P 'Said_Test_2026!' -C -i /docker-entrypoint-initdb.d/02-seed.sql
echo \"Loading lookup-data MERGE scripts (one connection per file)...\"
ok=0; fail=0
while IFS= read -r f; do
  [ -z \"$f\" ] && continue
  if docker exec -i {container} /opt/mssql-tools18/bin/sqlcmd \\
       -S localhost -U sa -P 'Said_Test_2026!' -C \\
       -i \"/data/$f\" >/dev/null 2>&1; then
    ok=$((ok+1))
  else
    fail=$((fail+1))
    echo \"  âœ— $f\"
  fi
done < ./data/_manifest.txt
echo \"Lookup data: $ok ok, $fail failed\"
echo
echo \"Sandbox ready!\"
echo \"Connection: Server=localhost,{port};User=sa;Password=Said_Test_2026!\"
",
            combo = combo_label, port = port, container = container_name,
            tables = table_count, procs = proc_count
        );
        let _ = std::fs::write(sandbox_dir_pb.join("run.sh"), &run_script);

        // DEFAULT: bring the container up. A "sandbox" without a running DB
        // isn't a sandbox. Pass explicit `up: false` to suppress.
        let mut up_result = String::new();
        if t.up.unwrap_or(true) {
            use std::process::Command;
            up_result.push_str("\n\nâ”€â”€ Bringing up sandbox â”€â”€\n");
            let compose_up = Command::new("docker")
                .args(["compose", "up", "-d"])
                .current_dir(&sandbox_dir)
                .output();
            match compose_up {
                Ok(r) if r.status.success() => {
                    up_result.push_str("âœ“ docker compose up -d\n");
                }
                Ok(r) => {
                    up_result.push_str(&format!(
                        "âœ— docker compose up failed:\n{}\n",
                        String::from_utf8_lossy(&r.stderr)
                    ));
                    up_result.push_str(&format!(
                        "\nManual recovery:\n  cd {} && bash run.sh\n",
                        sandbox_dir
                    ));
                    let output = format!(
                        "Files written to {}/\n{}", sandbox_dir, up_result
                    );
                    return Ok(CallToolResult::text_content(vec![TextContent::from(output)]));
                }
                Err(e) => {
                    up_result.push_str(&format!(
                        "âœ— Could not invoke docker: {}\n  Manual recovery: cd {} && bash run.sh\n",
                        e, sandbox_dir
                    ));
                    let output = format!(
                        "Files written to {}/\n{}", sandbox_dir, up_result
                    );
                    return Ok(CallToolResult::text_content(vec![TextContent::from(output)]));
                }
            }

            // Wait for SQL Server to become healthy (up to 90s).
            up_result.push_str("Waiting for SQL Server to become healthy...\n");
            let ready = (0..30).any(|_| {
                let probe = Command::new("docker")
                    .args(["exec", &container_name, "/opt/mssql-tools18/bin/sqlcmd",
                           "-S", "localhost", "-U", "sa", "-P", "Said_Test_2026!",
                           "-C", "-Q", "SELECT 1"])
                    .output();
                if let Ok(r) = probe {
                    if r.status.success()
                        && String::from_utf8_lossy(&r.stdout).contains("1 rows") {
                        return true;
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
                false
            });
            if !ready {
                up_result.push_str("âœ— SQL Server did not become healthy within 90s. Check `docker logs ");
                up_result.push_str(&container_name);
                up_result.push_str("`.\n");
                let output = format!(
                    "Files written to {}/\n{}", sandbox_dir, up_result
                );
                return Ok(CallToolResult::text_content(vec![TextContent::from(output)]));
            }
            up_result.push_str("âœ“ SQL Server healthy\n");

            // Load schema.
            up_result.push_str("Loading schema (this takes 20-30s)...\n");
            let load = Command::new("docker")
                .args(["exec", "-i", &container_name,
                       "/opt/mssql-tools18/bin/sqlcmd",
                       "-S", "localhost", "-U", "sa", "-P", "Said_Test_2026!",
                       "-C", "-i", "/docker-entrypoint-initdb.d/01-schema.sql"])
                .output();
            match load {
                Ok(r) => {
                    let stdout = String::from_utf8_lossy(&r.stdout);
                    let error_count = stdout.lines()
                        .filter(|l| l.contains("Msg") && l.contains("Level 16"))
                        .count();
                    up_result.push_str(&format!(
                        "âœ“ Schema loaded ({} non-cascade errors from pre-existing source bugs)\n",
                        error_count
                    ));
                }
                Err(e) => {
                    up_result.push_str(&format!("âœ— Schema load failed: {}\n", e));
                }
            }

            // Load lookup-data MERGE scripts â€” ONE FILE PER sqlcmd
            // invocation. Each invocation = fresh connection = clean
            // session state. This is the only reliable way to ship the
            // dt MERGE scripts, because their per-file IDENTITY_INSERT
            // toggles cannot be safely run in the same sqlcmd session
            // (state leaks between batches when one fails).
            if !data_by_table.is_empty() {
                let manifest_path = sandbox_dir_pb.join("data").join("_manifest.txt");
                let manifest_text = std::fs::read_to_string(&manifest_path)
                    .unwrap_or_default();
                let files: Vec<&str> = manifest_text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect();
                up_result.push_str(&format!(
                    "Loading lookup-data MERGE scripts ({} files, one connection each)...\n",
                    files.len()
                ));
                let mut ok = 0usize;
                let mut fail = 0usize;
                let mut failed_names: Vec<String> = Vec::new();
                for f in &files {
                    let docker_path = format!("/data/{}", f);
                    let r = Command::new("docker")
                        .args(["exec", "-i", &container_name,
                               "/opt/mssql-tools18/bin/sqlcmd",
                               "-S", "localhost", "-U", "sa", "-P", "Said_Test_2026!",
                               "-C", "-b", "-i", &docker_path])
                        .output();
                    match r {
                        Ok(out) if out.status.success() => ok += 1,
                        Ok(_) => {
                            fail += 1;
                            if failed_names.len() < 8 {
                                failed_names.push(f.to_string());
                            }
                        }
                        Err(_) => fail += 1,
                    }
                }
                up_result.push_str(&format!(
                    "âœ“ Lookup data: {} loaded, {} failed\n",
                    ok, fail
                ));
                if !failed_names.is_empty() {
                    for n in &failed_names {
                        up_result.push_str(&format!("    âœ— {}\n", n));
                    }
                    if fail > failed_names.len() {
                        up_result.push_str(&format!(
                            "    ... and {} more (see sandbox/data/ for the full set)\n",
                            fail - failed_names.len()
                        ));
                    }
                }
            }

            // Surface the dependency-closure check result. We wrote
            // `missing-deps.txt` upstream during schema.sql build â€”
            // print a one-liner here so operators see the count without
            // having to grep the sandbox dir.
            if !missing_deps.is_empty() {
                up_result.push_str(&format!(
                    "\nâš  {} cross-object reference{} unresolved in deploy set â€” see {}/missing-deps.txt\n\
                     (procs that call these will RAISERROR `Could not find stored procedure '...'` at runtime)\n",
                    missing_deps.len(),
                    if missing_deps.len() == 1 { "" } else { "s" },
                    sandbox_dir,
                ));
                let preview: Vec<&String> = missing_deps.keys().take(5).collect();
                for name in preview {
                    up_result.push_str(&format!("  âš  {}\n", name));
                }
                if missing_deps.len() > 5 {
                    up_result.push_str(&format!("  ... and {} more\n", missing_deps.len() - 5));
                }
            }

            up_result.push_str(&format!(
                "\nðŸŸ¢ Sandbox LIVE on port {}\n\
                 Connection: Server=localhost,{};User=sa;Password=Said_Test_2026!\n",
                port, port
            ));
        }

        let multi_hint = if is_multi {
            format!("\n\nCROSS-MODULE MODE â€” modules [{}] are deployed into the SAME database.\n\
                     Any proc from any listed module can INSERT into or trigger any other's tables.\n\
                     Use this to catch negative interactions (e.g. billing triggers firing on card updates).",
                    all_modules.join(", "))
        } else {
            String::new()
        };

        let output = format!(
            "Sandbox created: {}/\n\n\
             Modules:   [{}]\n\
             Container: {}\n\
             Port:      {} (host) â†’ 1433 (container)\n\n\
             Snapshot:  {}\n\n\
             Schema (dependency-ordered):\n\
             - {} functions (loaded first â€” DEFAULT constraints depend on these)\n\
             - {} tables (topologically sorted by FK dependencies)\n\
             - {} views\n\
             - {} stored procedures (from selected module(s) only)\n\
             - {} triggers\n\
             - {} seed data statements\n\n\
             Files:\n\
             - docker-compose.yml  (SQL Server 2022, container={}, port={})\n\
             - schema.sql          (functions â†’ tables â†’ views â†’ procs â†’ triggers)\n\
             - seed-data.sql       (inline INSERT statements)\n\
             - data/<NN>_<table>.sql (per-file MERGE scripts, one sqlcmd each)\n\
             - data/_manifest.txt  (load order, data-index.json-driven)\n\
             - run.sh              (start + load, idempotent)\n\n\
             To start:\n\
             cd {} && bash run.sh\n\n\
             Connection: Server=localhost,{};User=sa;Password=Said_Test_2026!\n\n\
             Parallel sandboxes: use different ports per sandbox (e.g. `port=1434`)\n\
             so multiple modules can run simultaneously without conflict.{}{}",
            sandbox_dir,
            all_modules.join(", "),
            container_name, port,
            if has_snapshot { "found (Exclusive/Shared/BOUNDARY.md)" }
            else { "NOT FOUND â€” run 'snapshot' first to extract module files" },
            func_count, table_count, view_count, proc_count, trigger_count, seed_parts.len(),
            container_name, port,
            sandbox_dir, port,
            multi_hint,
            up_result
        );

        Ok(CallToolResult::text_content(vec![TextContent::from(output)]))
    }

    fn handle_history(&self, t: HistoryTool) -> Result<CallToolResult, CallToolError> {
        let brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Walk tombstone chain for this doc_id
        let all_frames = brain.frames.get_all_frames();
        let mut chain: Vec<(u64, String, Option<f32>)> = Vec::new(); // (frame_id, status, delta)

        // Find all frames matching this name (active + tombstoned)
        for meta in &all_frames {
            if meta.doc_id.contains(&t.name) || meta.doc_id.ends_with(&format!("::{}", t.name)) {
                let status = match meta.status {
                    sca_core::frames::FrameStatus::Active => "active",
                    sca_core::frames::FrameStatus::Deleted => "deleted",
                    sca_core::frames::FrameStatus::Tombstone => "tombstone",
                };
                chain.push((meta.id, status.to_string(), Some(meta.semantic_delta)));
            }
        }

        if chain.is_empty() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("No history found for: {}", t.name),
            )]));
        }

        let mut output = format!("History for '{}': {} version(s)\n\n", t.name, chain.len());
        for (i, (fid, status, delta)) in chain.iter().enumerate() {
            output.push_str(&format!(
                "v{}: frame_id={} [{}] delta={:.4}\n",
                i, fid, status, delta.unwrap_or(0.0)
            ));
        }

        Ok(CallToolResult::text_content(vec![TextContent::from(output)]))
    }

    fn handle_checkout(&self, t: CheckoutTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;

        // Find the frame_id for the requested version
        let all_frames = brain.frames.get_all_frames();
        let matching: Vec<&&sca_core::frames::FrameMeta> = all_frames.iter()
            .filter(|m| m.doc_id.contains(&t.name) || m.doc_id.ends_with(&format!("::{}", t.name)))
            .collect();

        if (t.version as usize) >= matching.len() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("Version {} not found. Use 'history' to see available versions (0-{})",
                    t.version, matching.len().saturating_sub(1)),
            )]));
        }

        let frame_id = matching[t.version as usize].id;
        match brain.checkout_version(frame_id) {
            Ok((new_fid, delta)) => {
                brain.save().map_err(|e| CallToolError::from_message(e))?;
                Ok(CallToolResult::text_content(vec![TextContent::from(
                    format!("Checked out v{} â†’ new frame_id={}, semantic delta={:.4}", t.version, new_fid, delta),
                )]))
            }
            Err(e) => Ok(CallToolResult::text_content(vec![TextContent::from(
                format!("Checkout failed: {}", e),
            )])),
        }
    }
}

/// Convert days since 1970-01-01 to a (year, month, day) tuple using
/// Howard Hinnant's civil-date algorithm (portable, no chrono dependency).
/// Good for YYYY-MM-DD stamps on journal entries.
fn days_to_ymd(days_since_epoch: i64) -> (i32, u32, u32) {
    // Shift to 0000-03-01 epoch (2000-03-01 is day 730120 from 1970-01-01, but
    // the classical algorithm shifts by 719468 days).
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32)
}

/// Count regular files under `dir` recursively. Returns 0 on any error
/// (path missing, unreadable, etc.) so callers can treat "0" as a clear
/// signal that nothing got written.
fn walkdir_count(dir: std::path::PathBuf) -> u64 {
    let mut total: u64 = 0;
    let mut stack: Vec<std::path::PathBuf> = vec![dir];
    while let Some(current) = stack.pop() {
        let rd = match std::fs::read_dir(&current) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(entry.path()),
                Ok(ft) if ft.is_file() => total += 1,
                _ => {}
            }
        }
    }
    total
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// Forge MCP tool handlers (feature-gated). Six thin dispatchers over
// said_forge::mcp_api + said_forge::run_one.
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

#[cfg(feature = "forge")]
impl SaidServerHandler {
    fn handle_forge_list(&self, t: ForgeListTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let mut sfb = said_forge::SaidFileBrain::new(&mut brain);
        let project_root = self.brain_dir();
        let items = said_forge::list_stories(&mut sfb, &project_root, t.filter.as_deref())
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let body = serde_json::to_string_pretty(&items).unwrap_or_default();
        Ok(CallToolResult::text_content(vec![TextContent::from(body)]))
    }

    fn handle_forge_get(&self, t: ForgeGetTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let mut sfb = said_forge::SaidFileBrain::new(&mut brain);
        let md = said_forge::get_bundled(&mut sfb, &t.story_ids)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        Ok(CallToolResult::text_content(vec![TextContent::from(md)]))
    }

    fn handle_forge_status(&self, t: ForgeStatusTool) -> Result<CallToolResult, CallToolError> {
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let mut sfb = said_forge::SaidFileBrain::new(&mut brain);
        let project_root = self.brain_dir();
        let items = said_forge::story_status(&mut sfb, &project_root, &t.story_ids)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let body = serde_json::to_string_pretty(&items).unwrap_or_default();
        Ok(CallToolResult::text_content(vec![TextContent::from(body)]))
    }

    async fn handle_forge_load(&self, t: ForgeLoadTool) -> Result<CallToolResult, CallToolError> {
        if !t.confirm {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                "forge_load requires confirm:true â€” ask the user first and state which path/URL will be loaded".to_string(),
            )]));
        }
        let registry = said_forge::SourceRegistry::default();
        let adapter = match t.source.as_deref() {
            Some(name) => registry.by_name(name).map_err(|e| CallToolError::from_message(e.to_string()))?,
            None => registry.detect(&t.path_or_url).map_err(|e| CallToolError::from_message(e.to_string()))?,
        };
        let doc = adapter
            .load(&t.path_or_url, "mcp")
            .await
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let stories = adapter
            .extract_stories(&doc)
            .map_err(|e| CallToolError::from_message(e.to_string()))?;
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let hash = {
            let mut sfb = said_forge::SaidFileBrain::new(&mut brain);
            let h = said_forge::frame::write_directive(&mut sfb, &doc)
                .map_err(|e| CallToolError::from_message(e.to_string()))?;
            said_forge::frame::write_stories(&mut sfb, &stories)
                .map_err(|e| CallToolError::from_message(e.to_string()))?;
            h
        };
        brain.save().map_err(|e| CallToolError::from_message(e))?;
        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "loaded directive {} ({} stories)", hash, stories.len()
        ))]))
    }

    async fn handle_forge_run(&self, t: ForgeRunTool) -> Result<CallToolResult, CallToolError> {
        if !t.confirm {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                "forge_run requires confirm:true â€” show the user the cost estimate before calling".to_string(),
            )]));
        }
        // The MCP path holds `self.brain: Arc<Mutex<SaidFile>>` under a
        // std::sync::Mutex whose guard isn't Send across .await. run_one()
        // crosses multiple awaits (LLM call + generator retry). A clean
        // implementation would plumb brain through a tokio::sync::Mutex
        // or an actor, but that's a bigger refactor than Phase 9 scope.
        //
        // For now, MCP's forge_run returns the operator-facing CLI
        // equivalent and asks the caller to run it. The CLI path is
        // already fully exercised and smoke-tested.
        let hint = build_forge_run_cli_hint(&t);
        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "forge_run via MCP is deferred to v2 (MutexGuard !Send across .await).\n\
             Please run it from the shell:\n\n    {}\n\n\
             All other forge MCP tools (list, get, status, load, reset) work via MCP.",
            hint
        ))]))
    }

    fn handle_forge_reset(&self, t: ForgeResetTool) -> Result<CallToolResult, CallToolError> {
        if !t.confirm {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                "forge_reset requires confirm:true â€” destructive, ask the user".to_string(),
            )]));
        }
        let project_root = self.brain_dir();
        let mut brain = self.brain.lock().map_err(|e| {
            CallToolError::from_message(format!("brain lock: {}", e))
        })?;
        let hash = {
            let sfb = said_forge::SaidFileBrain::new(&mut brain);
            said_forge::frame::latest_directive_hash(&sfb)
                .ok_or_else(|| CallToolError::from_message("no directive loaded".to_string()))?
        };
        let mut total = 0u32;
        for slug_in in &t.story_ids {
            let slug = said_forge::sanitize_slug(slug_in);
            let n = {
                let mut sfb = said_forge::SaidFileBrain::new(&mut brain);
                said_forge::frame::tombstone_story(&mut sfb, &hash, &slug)
                    .map_err(|e| CallToolError::from_message(e.to_string()))?
            };
            total += n;
            said_forge::remove_folder(&project_root, &slug)
                .map_err(|e| CallToolError::from_message(e.to_string()))?;
            let adapter = said_forge::ClaudeAdapter;
            use said_forge::EditorAdapter as _;
            adapter.remove_skill(&project_root, &slug)
                .map_err(|e| CallToolError::from_message(e.to_string()))?;
        }
        brain.save().map_err(|e| CallToolError::from_message(e))?;
        Ok(CallToolResult::text_content(vec![TextContent::from(format!(
            "tombstoned {} frames across {} slugs",
            total, t.story_ids.len()
        ))]))
    }

    fn handle_forge_init(&self, t: ForgeInitTool) -> Result<CallToolResult, CallToolError> {
        use said_forge::init::{apply_scaffold, ApplyMode};
        use std::path::PathBuf;
        let target = t
            .target
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&t.project));
        let mode = if t.force.unwrap_or(false) {
            ApplyMode::Force
        } else if t.merge.unwrap_or(false) {
            ApplyMode::Merge
        } else {
            ApplyMode::Strict
        };
        let result = apply_scaffold(&t.project, &target, mode)
            .map_err(|e| CallToolError::from_message(format!("forge_init: {}", e)))?;
        let payload = serde_json::json!({
            "ok": true,
            "project": t.project,
            "root": target.display().to_string(),
            "said_path": result.said_path.display().to_string(),
            "folders_created": result.folders_created,
            "files_created": result.files_created,
            "files_skipped": result.files_skipped,
            "next_steps": [
                "drop SQL/code into 1-ground-truth/",
                "drop existing code into 2-progress/",
                "drop client PDFs into 3-requirements/",
                "drop OpenAPI + MDs into 4-expectations/",
                "call forge_plan_questions to run the Q&A"
            ]
        });
        Ok(CallToolResult::text_content(vec![TextContent::from(
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )]))
    }

    fn handle_forge_plan_questions(
        &self,
        t: ForgePlanQuestionsTool,
    ) -> Result<CallToolResult, CallToolError> {
        use std::path::PathBuf;
        let root = t
            .target
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            });
        let scan = said_forge::plan_scan::scan_project(&root)
            .map_err(|e| CallToolError::from_message(format!("scan: {}", e)))?;
        let questions = said_forge::plan_cli::questions_for(&scan);
        let payload = serde_json::json!({
            "ok": true,
            "root": root.display().to_string(),
            "scan_summary": {
                "ground_truth": scan.ground_truth.len(),
                "progress": scan.progress.len(),
                "requirements": scan.requirements.len(),
                "expectations": scan.expectations.len(),
                "has_openapi": scan.has_openapi,
                "has_xlsx": scan.has_xlsx,
                "has_dev_planning_md": scan.has_dev_planning_md,
                "orphan_files": scan.orphan_files.len(),
            },
            "questions": questions,
        });
        Ok(CallToolResult::text_content(vec![TextContent::from(
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )]))
    }

    fn handle_forge_gaps(
        &self,
        t: ForgeGapsTool,
    ) -> Result<CallToolResult, CallToolError> {
        use std::path::PathBuf;
        let root = t
            .target
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let gaps_path = root.join(".forge/gaps.md");
        if !gaps_path.exists() {
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                format!(
                    "No gap report at {}. Run `said forge gaps --target {}` to generate one.",
                    gaps_path.display(),
                    root.display()
                ),
            )]));
        }
        let content = std::fs::read_to_string(&gaps_path)
            .map_err(|e| CallToolError::from_message(format!("read gaps: {}", e)))?;
        Ok(CallToolResult::text_content(vec![TextContent::from(content)]))
    }

    fn handle_forge_sync(
        &self,
        t: ForgeSyncTool,
    ) -> Result<CallToolResult, CallToolError> {
        use sca_core::said_file::SaidFile;
        use said_forge::sync::{build_sync_plan, execute_sync_with_cfg, load_manifest_public};
        use said_forge::workspace_config::WorkspaceConfig;
        use std::path::PathBuf;

        let root = t
            .target
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            });

        let cfg_path = WorkspaceConfig::default_path_for(&root);
        if !cfg_path.exists() {
            return Err(CallToolError::from_message(format!(
                "no .forge/config.toml at {} â€” call forge_plan_apply first",
                cfg_path.display()
            )));
        }
        let cfg = WorkspaceConfig::load(&cfg_path)
            .map_err(|e| CallToolError::from_message(format!("load config: {}", e)))?;
        if !cfg.plan_complete {
            return Err(CallToolError::from_message(format!(
                "config at {} has plan_complete=false â€” call forge_plan_apply first",
                cfg_path.display()
            )));
        }

        // Locate the workspace .said (prefer <dirname>.said, else single match).
        let said_path = locate_workspace_said(&root)
            .map_err(CallToolError::from_message)?;

        let mut plan = build_sync_plan(&root, &cfg)
            .map_err(|e| CallToolError::from_message(format!("build plan: {}", e)))?;

        let force = t.force.unwrap_or(false);
        let dry_run = t.dry_run.unwrap_or(false);

        if !force && said_path.exists() {
            let mut said = SaidFile::open(&said_path)
                .map_err(CallToolError::from_message)?;
            let project_name = said_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("forge");
            if let Some(mf) = load_manifest_public(&mut said, project_name) {
                mf.filter_unchanged(&mut plan);
            }
        }

        if dry_run {
            let payload = serde_json::json!({
                "ok": true,
                "dry_run": true,
                "to_ingest": plan.entries.len(),
                "skipped_unchanged": plan.skipped_unchanged.len(),
                "entries": plan.entries.iter().map(|e| serde_json::json!({
                    "path": e.rel_path.display().to_string(),
                    "kind": format!("{:?}", e.kind),
                    "authority": e.authority.to_tag(),
                    "size": e.size,
                })).collect::<Vec<_>>(),
            });
            return Ok(CallToolResult::text_content(vec![TextContent::from(
                serde_json::to_string_pretty(&payload).unwrap_or_default(),
            )]));
        }

        let mut said = if said_path.exists() {
            SaidFile::open(&said_path).map_err(CallToolError::from_message)?
        } else {
            SaidFile::create(&said_path)
        };
        let project_name = said_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("forge")
            .to_string();
        let result = execute_sync_with_cfg(&plan, &mut said, &project_name, Some(&cfg))
            .map_err(|e| CallToolError::from_message(format!("execute sync: {}", e)))?;
        said.save().map_err(|e| CallToolError::from_message(format!("save: {}", e)))?;

        let payload = serde_json::json!({
            "ok": true,
            "said": said_path.display().to_string(),
            "frames_written": result.frames_written,
            "bytes_ingested": result.bytes_ingested,
            "skipped_unchanged": result.files_skipped_unchanged,
            "skipped_xlsx_unsupported": result.files_skipped_xlsx_unsupported,
            "by_kind": result.by_kind,
            "by_authority": result.by_authority,
        });
        Ok(CallToolResult::text_content(vec![TextContent::from(
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )]))
    }

    fn handle_forge_plan_apply(
        &self,
        t: ForgePlanApplyTool,
    ) -> Result<CallToolResult, CallToolError> {
        use std::path::PathBuf;
        let root = t
            .target
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            });
        let scan = said_forge::plan_scan::scan_project(&root)
            .map_err(|e| CallToolError::from_message(format!("scan: {}", e)))?;
        let pairs: Vec<(String, String)> = t
            .answers
            .iter()
            .map(|a| (a.question_id.clone(), a.answer_key.clone()))
            .collect();
        let cfg = said_forge::plan_cli::apply_answers_noninteractive(&scan, &pairs)
            .map_err(|e| CallToolError::from_message(format!("apply: {}", e)))?;
        let cfg_path = said_forge::workspace_config::WorkspaceConfig::default_path_for(&root);
        cfg.save(&cfg_path)
            .map_err(|e| CallToolError::from_message(format!("save: {}", e)))?;
        let payload = serde_json::json!({
            "ok": true,
            "plan_complete": cfg.plan_complete,
            "directive_mode": format!("{:?}", cfg.directive.mode),
            "primary": cfg.directive.primary,
            "secondary": cfg.directive.secondary,
            "config_path": cfg_path.display().to_string(),
            "next_steps": ["call forge_sync to ingest all files"],
        });
        Ok(CallToolResult::text_content(vec![TextContent::from(
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )]))
    }
}

#[cfg(feature = "forge")]
fn locate_workspace_said(root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if let Some(folder_name) = root.file_name().and_then(|n| n.to_str()) {
        let candidate = root.join(format!("{}.said", folder_name));
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)
        .map_err(|e| format!("read {}: {}", root.display(), e))?
    {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.extension().and_then(|e| e.to_str()) == Some("said") {
            found.push(p);
        }
    }
    match found.len() {
        0 => Err(format!("no .said file at {}", root.display())),
        1 => Ok(found.into_iter().next().unwrap()),
        _ => Err(format!(
            "multiple .said files at {} â€” pass an explicit target path",
            root.display()
        )),
    }
}

#[cfg(feature = "forge")]
fn build_forge_run_cli_hint(t: &ForgeRunTool) -> String {
    let mut parts = vec!["said forge run".to_string()];
    if t.all.unwrap_or(false) {
        parts.push("--all".into());
    }
    if let Some(ids) = &t.ids {
        parts.push(format!("--ids {}", ids.join(",")));
    }
    if let Some(f) = &t.filter {
        parts.push(format!("--filter '{}'", f));
    }
    if t.force.unwrap_or(false) {
        parts.push("--force".into());
    }
    if let Some(n) = t.halt_after {
        parts.push(format!("--halt-after {}", n));
    }
    parts.push("--yes".into());
    parts.join(" ")
}
