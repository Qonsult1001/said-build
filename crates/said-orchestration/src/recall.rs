//! Recall the most relevant verified coding iteration from the `.said` brain.
//!
//! Mirrors the proven matcher used by `said recall-fix` (intent-isolated action
//! fingerprint + target overlap), but reads from sca-core directly since the
//! orchestrator is a separate process. The tag/marker strings MUST match what
//! `said learn-fix` writes (kept in sync with said-cli's coding-memory).

use sca_core::said_file::SaidFile;
use std::collections::{HashMap, HashSet};

// Must match said-cli's coding-memory writer.
const FIX_KIND_TAG: &str = "coding-fix";
const FIX_ACTION_ID_PREFIX: &str = "fixaction::";
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

/// The stored problem (TASK: line) for target-token extraction.
fn stored_problem(body: &str) -> String {
    iteration_note(body)
        .lines()
        .find_map(|l| l.strip_prefix("TASK: "))
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Find the best-matching verified iteration for `task`. Returns None if nothing
/// relevant is stored — the orchestrator then proceeds with no recalled context
/// (the LLM starts fresh, exactly like a cold project).
pub fn best_iteration(brain: &mut SaidFile, task: &str) -> Option<Recalled> {
    // Candidate set: coding-fix frames surfaced by fusion recall.
    let (fusion_cands, _kw) = sca_core::ask::ask(brain, task, 25, false, None);
    let candidate_ids: Vec<String> = fusion_cands
        .iter()
        .filter(|c| {
            brain
                .frames
                .get_meta(&c.doc_id)
                .map(|m| m.tags.iter().any(|t| t == FIX_KIND_TAG))
                .unwrap_or(false)
        })
        .map(|c| c.doc_id.clone())
        .collect();
    if candidate_ids.is_empty() {
        return None;
    }

    // Intent-isolated action match (the proven breakthrough): 1-bit fingerprint
    // of the action residue against the companion fixaction:: frames.
    let q_action = sca_core::ask::action_residue(task);
    let action_fp: HashMap<String, f32> = if q_action.is_empty() {
        HashMap::new()
    } else {
        brain
            .rank_by_fingerprint(&q_action, 100)
            .into_iter()
            .filter_map(|(d, s)| d.strip_prefix(FIX_ACTION_ID_PREFIX).map(|id| (id.to_string(), s)))
            .collect()
    };
    let q_action_toks: HashSet<String> =
        q_action.split_whitespace().map(|s| s.to_string()).collect();
    let q_target_toks: HashSet<String> = task
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '/'))
        .filter(|t| t.len() >= 3 && !q_action_toks.contains(&t.to_lowercase()))
        .map(|t| t.to_lowercase())
        .collect();

    let mut best: Option<Recalled> = None;
    for doc_id in candidate_ids {
        let body = brain.get(&doc_id).unwrap_or_default();
        let c_problem = stored_problem(&body);
        let c_action = sca_core::ask::action_residue(&c_problem);
        let c_action_toks: HashSet<String> =
            c_action.split_whitespace().map(|s| s.to_string()).collect();
        let id16 = doc_id.strip_prefix("fix::").unwrap_or(&doc_id);
        let action_score = action_fp.get(id16).copied().unwrap_or(0.0);
        let c_target_toks: HashSet<String> = c_problem
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '/'))
            .filter(|t| t.len() >= 3)
            .map(|t| t.to_lowercase())
            .filter(|t| !c_action_toks.contains(t))
            .collect();
        let target_score = if c_target_toks.is_empty() {
            0.0
        } else {
            let overlap = c_target_toks.iter().filter(|t| q_target_toks.contains(*t)).count();
            overlap as f32 / c_target_toks.len() as f32
        };
        let score = 0.9 * action_score + 0.1 * target_score;
        if best.as_ref().map(|b| score > b.score).unwrap_or(true) {
            best = Some(Recalled {
                doc_id: doc_id.clone(), score,
                note: iteration_note(&body),
                edits_json: iteration_edits(&body),
            });
        }
    }
    best
}
