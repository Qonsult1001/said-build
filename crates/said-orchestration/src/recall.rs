//! Recall the most relevant verified coding iteration from the `.said` brain.
//!
//! Scored by `.said`'s OWN documented retrieval pipeline (`ask` fusion
//! confidence — SCA fingerprint + BM25 + boosts; see
//! docs/said-structure/03-core-subsystems/3.5-retrieval-pipeline.md). The
//! tag/marker strings MUST match what `said learn-fix` writes.

use sca_core::said_file::SaidFile;

// Must match said-cli's coding-memory writer.
const FIX_EDITS_SEP: &str = "\n<<<SAID-FIX-EDITS>>>\n";
const FIX_ACTION_SEP: &str = "\n<<<SAID-FIX-ACTION>>>\n";

/// A recalled verified iteration.
pub struct Recalled {
    pub doc_id: String,
    pub score: f32,
    /// The full human-readable iteration note (everything before the machine
    /// payload) — the whole story the LLM reloads.
    pub note: String,
    /// The stored VERIFIED change-set JSON (the edits that built+passed). This is
    /// what fix-replay applies directly — the moat (no LLM re-derivation).
    pub edits_json: String,
}

/// The full iteration note (everything before the machine payload markers).
fn iteration_note(body: &str) -> String {
    body.split(FIX_EDITS_SEP).next().unwrap_or(body).trim().to_string()
}

/// The stored change-set JSON (between the edits + action markers).
fn iteration_edits(body: &str) -> String {
    let after = match body.split_once(FIX_EDITS_SEP) {
        Some((_, b)) => b,
        None => return String::new(),
    };
    after.split(FIX_ACTION_SEP).next().unwrap_or(after).trim().to_string()
}

/// Minimum recall confidence to treat a stored fix as a real match worth
/// injecting. Below this, return None (MISS) — critical at scale: with thousands
/// of stored fixes there is ALWAYS a top candidate, but injecting an irrelevant
/// one harms the prompt. Calibrated against `ask` fusion confidence (sym=1.0,
/// grep 0.4-0.95, semantic 0.3-0.8); 0.45 keeps genuine matches, drops noise.
/// Override with SAID_RECALL_MIN.
fn recall_min() -> f32 {
    std::env::var("SAID_RECALL_MIN").ok().and_then(|s| s.parse().ok()).unwrap_or(0.45)
}

/// Find the best-matching verified iteration for `task`, scored by `.said`'s own
/// documented retrieval pipeline (`ask` fusion confidence — SCA fingerprint +
/// BM25 + boosts, scale-tested). Returns None when nothing clears the confidence
/// floor, so memory only injects when there's a GENUINE match (safe at 1000s of
/// records). No bespoke/fragile id-join — uses the engine's ranked score directly.
pub fn best_iteration(brain: &mut SaidFile, task: &str) -> Option<Recalled> {
    let min = recall_min();
    // ONE scorer, shared with the CLI (sca-core::ask::best_coding_fix): intent
    // fingerprint (gate) + symmetric distinctive-target overlap (picks the right
    // one). NOT raw fusion text confidence — that's bag-of-words and collides at
    // scale, and it disagreed with the CLI scorer (the bug we measured).
    let (doc_id, score) = sca_core::ask::best_coding_fix(brain, task)?;
    if std::env::var("SAID_RECALL_DEBUG").is_ok() {
        eprintln!("[recall-dbg] {} score={:.3} (floor {:.2})", doc_id, score, min);
    }
    if score < min {
        return None; // MISS — caller falls through to the LLM with no injection
    }
    let body = brain.get(&doc_id).unwrap_or_default();
    Some(Recalled {
        doc_id,
        score,
        note: iteration_note(&body),
        edits_json: iteration_edits(&body),
    })
}
