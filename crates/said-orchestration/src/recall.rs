//! Recall the most relevant verified coding iteration from the `.said` brain.
//!
//! Scored by `.said`'s OWN documented retrieval pipeline (`ask` fusion
//! confidence — SCA fingerprint + BM25 + boosts; see
//! docs/said-structure/03-core-subsystems/3.5-retrieval-pipeline.md). The
//! tag/marker strings MUST match what `said learn-fix` writes.

use sca_core::said_file::SaidFile;

// Must match said-cli's coding-memory writer.
const FIX_KIND_TAG: &str = "coding-fix";
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
    let (fusion_cands, _kw) = sca_core::ask::ask(brain, task, 25, false, None);
    let dbg = std::env::var("SAID_RECALL_DEBUG").is_ok();
    let min = recall_min();

    // Highest-confidence coding-fix frame from the ranked candidates.
    let mut best: Option<Recalled> = None;
    for c in &fusion_cands {
        let is_fix = brain.frames.get_meta(&c.doc_id)
            .map(|m| m.tags.iter().any(|t| t == FIX_KIND_TAG)).unwrap_or(false);
        if !is_fix { continue; }
        if dbg {
            eprintln!("[recall-dbg] {} confidence={:.3}", c.doc_id, c.confidence);
        }
        if best.as_ref().map(|b| c.confidence > b.score).unwrap_or(true) {
            let body = brain.get(&c.doc_id).unwrap_or_default();
            best = Some(Recalled {
                doc_id: c.doc_id.clone(), score: c.confidence,
                note: iteration_note(&body),
                edits_json: iteration_edits(&body),
            });
        }
    }
    // MISS unless the best genuine match clears the floor.
    match best {
        Some(b) if b.score >= min => Some(b),
        _ => None,
    }
}
