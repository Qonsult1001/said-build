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

/// Find the best-matching verified iteration for `task` via the shared reader.
/// Returns None when nothing clears the confidence floor, so memory only injects on
/// a GENUINE match. No bespoke format/scoring here — that drift is what we removed.
pub fn best_iteration(brain: &mut SaidFile, task: &str) -> Option<Recalled> {
    let min = recall_min();
    let hit = sca_core::ask::recall_coding_fix(brain, task, min)?;
    if std::env::var("SAID_RECALL_DEBUG").is_ok() {
        eprintln!("[recall-dbg] {} score={:.3} (floor {:.2})", hit.doc_id, hit.score, min);
    }
    Some(Recalled {
        doc_id: hit.doc_id,
        score: hit.score,
        note: hit.note,
        edits_json: hit.edits_json,
    })
}
