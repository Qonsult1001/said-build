//! Recall the most relevant verified coding iteration from the `.said` brain.
//!
//! Thin wrapper over the ONE shared coding-fix reader,
//! [`sca_core::ask::recall_coding_fix`] (semantic `best_coding_fix` scorer + the
//! shared frame format), so the CLI `recall-fix`, the MCP tool, and this orchestrator
//! all read the SAME learning store with identical scoring. See
//! docs/said-structure/03-core-subsystems/3.5-retrieval-pipeline.md.

use sca_core::said_file::SaidFile;

/// A recalled verified iteration.
pub struct Recalled {
    pub doc_id: String,
    pub score: f32,
    /// The full human-readable iteration note (everything before the machine
    /// payload) — the whole story the LLM reloads.
    pub note: String,
    /// The stored VERIFIED change-set JSON (the edits that built+passed).
    pub edits_json: String,
}

/// Minimum recall confidence to treat a stored fix as a real match worth injecting.
/// Below this, return None (MISS) — critical at scale: with thousands of stored fixes
/// there is ALWAYS a top candidate, but injecting an irrelevant one harms the prompt.
/// Override with SAID_RECALL_MIN.
fn recall_min() -> f32 {
    std::env::var("SAID_RECALL_MIN").ok().and_then(|s| s.parse().ok()).unwrap_or(0.45)
}

/// How many recalled learnings to inject (the semantic top-k contract). Default 5:
/// measured recall@5 = 100% at 1000 records, and an A/B proved top-5 rescues cases
/// where the right learning isn't rank-1 — on a rank-3 brain, top-1 injected a WRONG
/// decoy and went RED (5 attempts), top-5 included the real fix and went GREEN (1).
/// The model picks/adapts the fitting candidate; the gate still judges. Override with
/// SAID_INJECT_TOPK (e.g. =1 for a minimal prompt when the store is small/clean).
pub fn inject_topk() -> usize {
    std::env::var("SAID_INJECT_TOPK").ok().and_then(|s| s.parse().ok()).filter(|&k| k >= 1).unwrap_or(5)
}

/// Find the best-matching verified iteration for `task` via the shared reader.
/// Returns None when nothing clears the confidence floor, so memory only injects on
/// a GENUINE match. No bespoke format/scoring here — that drift is what we removed.
pub fn best_iteration(brain: &mut SaidFile, task: &str) -> Option<Recalled> {
    best_iterations(brain, task, 1).into_iter().next()
}

/// Top-K matching verified iterations for `task` (highest score first), each above
/// the confidence floor. Empty when nothing matches. Used by the orchestrator's
/// memory step to inject one or several learnings.
pub fn best_iterations(brain: &mut SaidFile, task: &str, k: usize) -> Vec<Recalled> {
    let min = recall_min();
    let hits = sca_core::ask::recall_coding_fixes(brain, task, k, min);
    let dbg = std::env::var("SAID_RECALL_DEBUG").is_ok();
    hits.into_iter().map(|h| {
        if dbg {
            eprintln!("[recall-dbg] {} score={:.3} (floor {:.2})", h.doc_id, h.score, min);
        }
        Recalled { doc_id: h.doc_id, score: h.score, note: h.note, edits_json: h.edits_json }
    }).collect()
}
