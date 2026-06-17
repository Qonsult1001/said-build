//! STEP 6 — LEARN (memory).
//!
//! | Claude does | .said provides |
//! |-------------|----------------|
//! | session memory, "tell the whole story" + compress | the iteration store + the LEARN extraction prompt + the compressor |
//!
//! Runs ONLY on a green gate — `success` is the sole recorded outcome. Mirrors
//! Claude Code's session-memory extraction EXACTLY: the LLM AUTHORS a structured
//! 10-section note (via the LEARN prompt + template in said-prompts), then .said
//! COMPRESSES it (per-section cap + cycle-out, see [`crate::compress`]) and stores
//! it. The next task recalls this whole story — the compounding answer base.

use crate::compress::compress_note;
use sca_core::said_file::SaidFile;
use said_llm::{CompletionRequest, LlmProvider};
use said_prompts::coding::{ITERATION_TEMPLATE, LEARN};

/// Author (via the LLM) + compress + store the verified iteration. `transcript`
/// is the completed work (task + plan + code + gate result) the LLM summarizes.
/// `change_set` is the verified edits output, stored as the machine payload.
pub async fn run(
    brain: &mut SaidFile,
    provider: &dyn LlmProvider,
    task: &str,
    transcript: &str,
    change_set: &str,
) -> Result<(), String> {
    // 1. LLM authors the structured 10-section note (Claude's extraction move).
    let user = LEARN
        .replace("{{transcript}}", transcript.trim())
        .replace("{{template}}", ITERATION_TEMPLATE);
    let req = CompletionRequest {
        system: said_prompts::coding::SYSTEM_LEARN.to_string(),
        user,
        cacheable_prelude: None,
        schema: serde_json::json!({
            "type": "object",
            "properties": { "note": { "type": "string" } },
            "required": ["note"],
            "additionalProperties": false
        }),
        schema_name: "iteration_note".to_string(),
        max_output_tokens: 32768,
        temperature: 0.2,
        json_object: true,
    };
    let authored = match provider.complete(&req).await {
        Ok(resp) => resp
            .json
            .get("note")
            .and_then(|v| v.as_str())
            .unwrap_or(&resp.raw)
            .to_string(),
        // If authoring fails, fall back to a minimal note rather than losing the
        // verified result — the gate already confirmed it works.
        Err(_) => format!(
            "# Title\nVerified iteration\n\n# Task\n{}\n\n# Key Results\n{}\n",
            task.trim(),
            change_set.trim()
        ),
    };

    // 2. .said compresses it (per-section cap + cycle-out).
    let note = compress_note(&authored);

    // 3. Store via the ONE shared writer (sca_core::ask::learn_coding_fix) so the
    //    frame format, tags, and blake3 doc_id are byte-identical to what the CLI
    //    `learn-fix` and the MCP learn_fix tool write — all three contribute to the
    //    SAME learning store. (No bespoke body assembly / hashing here; that drift
    //    is exactly what we removed.)
    sca_core::ask::learn_coding_fix(brain, task, &note, change_set, None);
    brain.save().map_err(|e| format!("save brain: {}", e))?;
    Ok(())
}
