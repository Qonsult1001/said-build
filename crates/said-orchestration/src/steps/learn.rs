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

// Must match said-cli's coding-memory + said-orchestration::recall.
const FIX_KIND_TAG: &str = "coding-fix";
const FIX_PILLAR_TAG: &str = "pillar:procedural";
const FIX_SUCCESS_TAG: &str = "procedural:outcome=success";
const FIX_ACTION_TAG: &str = "coding-fix-action";
const FIX_ACTION_ID_PREFIX: &str = "fixaction::";
const FIX_EDITS_SEP: &str = "\n<<<SAID-FIX-EDITS>>>\n";
const FIX_ACTION_SEP: &str = "\n<<<SAID-FIX-ACTION>>>\n";

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
        system: "You write concise, info-dense coding-iteration memory notes. Output ONLY the \
                 filled-in template as JSON: {\"note\": \"<the full note>\"}."
            .to_string(),
        user,
        cacheable_prelude: None,
        schema: serde_json::json!({
            "type": "object",
            "properties": { "note": { "type": "string" } },
            "required": ["note"],
            "additionalProperties": false
        }),
        schema_name: "iteration_note".to_string(),
        max_output_tokens: 4096,
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

    // 3. Store as a Procedural-pillar coding-fix frame + action companion. The
    //    body is: TASK line (for recall) + the compressed note + machine payload.
    let action = sca_core::ask::action_residue(task);
    let mut body = format!("TASK: {}\n\n", task.trim());
    body.push_str(&note);
    body.push_str(FIX_EDITS_SEP);
    body.push_str(change_set.trim());
    body.push_str(FIX_ACTION_SEP);
    body.push_str(action.trim());

    let id16 = short_hash(&body);
    let doc_id = format!("fix::{}", id16);
    brain.remember_as(&doc_id, &body, Some("coding-fix"));
    brain.add_tag(&doc_id, FIX_PILLAR_TAG);
    brain.add_tag(&doc_id, FIX_SUCCESS_TAG);
    brain.add_tag(&doc_id, FIX_KIND_TAG);
    if !action.is_empty() {
        let action_id = format!("{}{}", FIX_ACTION_ID_PREFIX, id16);
        brain.remember_as(&action_id, &action, Some("coding-fix-action"));
        brain.add_tag(&action_id, FIX_ACTION_TAG);
    }
    let _ = brain.build_index();
    brain.save().map_err(|e| format!("save brain: {}", e))?;
    Ok(())
}

/// 16-hex-char FNV-1a of the body (no extra deps; deterministic).
fn short_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x00000100000001B3);
    }
    format!("{:016x}", h)
}
