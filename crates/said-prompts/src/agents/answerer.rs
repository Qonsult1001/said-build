//! Answerer agent — the current main-loop role.
//!
//! Composes the shared sections into the system-prompt string the WASM
//! agent loop uses. Output is byte-identical to the prior inline JS
//! `buildSystemPrompt` (modulo the runtime-injected files block).
//!
//! Dynamic context (file list, total docs) is supplied at assembly time
//! via `Context`. Everything else is static text from sibling modules.

use crate::{
    conversation, core, retrieval, strategy, tools,
};

/// Runtime context the answerer prompt needs to be assembled. The caller
/// (WASM agent loop, MCP handler, native CLI) is responsible for
/// gathering these from the loaded library at session start.
#[derive(Debug, Clone)]
pub struct Context {
    /// Filenames of currently loaded `.said` brains. Empty list →
    /// rendered as "(no files loaded)".
    pub files: Vec<String>,
    /// Sum of active memories across all loaded brains.
    pub total_docs: usize,
}

impl Context {
    pub fn new() -> Self {
        Self { files: Vec::new(), total_docs: 0 }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the answerer system prompt. Joins the canonical sections with
/// blank-line separators in the same order the legacy JS used.
pub fn build(ctx: &Context) -> String {
    let files_note = if ctx.files.is_empty() {
        "(no files loaded)".to_string()
    } else {
        ctx.files
            .iter()
            .map(|n| format!("  - {}", n))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut parts: Vec<String> = Vec::with_capacity(20);
    parts.push(core::DEFAULT_IDENTITY.to_string());
    parts.push(String::new());
    parts.push(core::SYSTEM_PRINCIPLES.to_string());
    parts.push(String::new());
    parts.push("AVAILABLE FILES:".to_string());
    parts.push(files_note);
    parts.push(format!(
        "TOTAL ACTIVE MEMORIES ACROSS THE LIBRARY: {}",
        ctx.total_docs
    ));
    parts.push(String::new());
    parts.push(tools::TOOLS_READ.to_string());
    parts.push(String::new());
    parts.push(tools::TOOLS_CODING.to_string());
    parts.push(String::new());
    parts.push(tools::TOOLS_WRITE.to_string());
    parts.push(String::new());
    parts.push(conversation::CONVERSATION_MEMORY.to_string());
    parts.push(String::new());
    parts.push(conversation::THREE_SOURCES_OF_TRUTH.to_string());
    parts.push(String::new());
    parts.push(conversation::EPISODIC_FRAME_SCHEMA.to_string());
    parts.push(String::new());
    parts.push(conversation::RECALL_EPISODIC_ITERATION.to_string());
    parts.push(String::new());
    parts.push(strategy::ANSWERER_STRATEGY.to_string());
    parts.push(String::new());
    parts.push(retrieval::HONESTY_PATTERNS.to_string());
    parts.push(String::new());
    parts.push(retrieval::ASK_FUSED_RESULT_SHAPE.to_string());
    parts.push(String::new());
    parts.push(retrieval::NEVER_DO.to_string());
    parts.push(String::new());
    parts.push(strategy::PRONOUN_AND_TOOL_NAMES.to_string());
    parts.push(String::new());
    parts.push(retrieval::ANSWER_FORMAT.to_string());

    parts.join("\n")
}
