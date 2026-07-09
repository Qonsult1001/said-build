//! Work-state — COMPACTION-SURVIVAL memory (docs/said-structure/30-beat-them-benchmark.md).
//!
//! A CORE memory function (like `ask`/`learn_coding_fix`/blueprint), available to every `.said` user on
//! every surface — NOT a vault/enterprise feature. (The vault is a separate enterprise product for
//! document compliance; work-state belongs in the shared engine.)
//!
//! The #1 unfixable weakness of Claude/Cursor/Kimi: when the host compacts/summarizes the context window,
//! the fact-dense detail (exact thresholds, IDs, decisions, the next step) is LOST — the agent "goes
//! stupid, doesn't know what it was doing." That cannot be fixed from INSIDE the window (the window is
//! what's being compacted). `.said` lives OUTSIDE it: the work-state is stored, re-grounded on the next
//! turn AS IF NOTHING DISAPPEARED.
//!
//! FREE-FORM, agent-authored (the blueprint-NL lesson + Self-Spec, 14.15): we do NOT impose a rigid field
//! schema the agent must map its reality into — that's the trap we hit with blueprint call-tokens. The
//! agent writes the work-state IN ITS OWN WORDS (one ordered NL note: what I'm doing, the next step, the
//! exact values, the dead ends — phrased however it likes). `.said` stores that note as-is and re-injects
//! it VERBATIM after a compaction, so the fact-dense detail summarization would paraphrase away survives
//! exactly. Stored as one frame `workstate::<project>` (Episodic, `kind:workstate`); read back by id =
//! exact string roundtrip (no fuzzy match — it's keyed by project, retrieved exactly).

use crate::frames::Pillar;
use crate::said_file::SaidFile;

/// Marker tag for work-state frames — one source of truth across CLI + MCP + steering.
pub const WORKSTATE_KIND_TAG: &str = "kind:workstate";
pub const WORKSTATE_ID_PREFIX: &str = "workstate::";

/// A suggested (NOT enforced) shape for the agent's note, offered as guidance only. The agent may follow
/// it, reorder it, or ignore it — whatever captures the mid-task reality. Surfaced by CLI/MCP as a hint.
pub const WORKSTATE_HINT: &str = "\
Write your current work-state in your own words so you can resume EXACTLY after a context compaction.
Include whatever matters; a useful shape (optional):
  - what I'm doing right now
  - the immediate next step
  - decisions made (in their exact form, e.g. 'threshold = size > 1, NOT >= 1')
  - exact values that a summary would paraphrase away (IDs, numbers, file:line, commits)
  - dead ends already tried (don't retry them)
  - where in the plan I am";

fn frame_id(project: &str) -> String {
    format!("{}{}", WORKSTATE_ID_PREFIX, project.trim())
}

/// Shared concept that wiki-links every work-state frame of a project into ONE chain (the OKF graph
/// edge, same `link:<concept>` namespace a `[[wikilink]]` yields). Walking this concept = the full,
/// byte-exact history across every compaction — nothing is lost, not even the oldest round.
fn chain_concept(project: &str) -> String {
    // hyphenated (concepts are single tokens); kept lowercase to match the link: namespace.
    format!("workstate-{}", project.trim().to_lowercase())
}

/// The per-round frame id, sequence-numbered so history is deterministically orderable WITHOUT relying
/// on metadata timestamps (which are not settable; see frames.rs). Zero-padded for lexical ordering.
fn chain_frame_id(project: &str, seq: usize) -> String {
    format!("{}{}::{:06}", WORKSTATE_ID_PREFIX, project.trim(), seq)
}

/// Parse the sequence number back out of a chain frame id (None if it isn't one).
fn seq_of(project: &str, doc_id: &str) -> Option<usize> {
    let prefix = format!("{}{}::", WORKSTATE_ID_PREFIX, project.trim());
    doc_id.strip_prefix(&prefix)?.parse::<usize>().ok()
}

/// CAPTURE the agent's free-form work-state note for a project — latest wins (one current state per
/// project). Stored verbatim; `note` is the agent's own words. Returns the frame doc_id.
pub fn save_work_state(brain: &mut SaidFile, project: &str, note: &str) -> String {
    let id = frame_id(project);
    let tags = vec![
        WORKSTATE_KIND_TAG.to_string(),
        format!("project:{}", project.trim()),
    ];
    brain.remember_with_pillar(Some(&id), note.trim(), None, Pillar::Episodic, tags);
    let _ = brain.build_index();
    id
}

/// APPEND a work-state note as a NEW frame in the project's wiki-linked chain (the owner's insight:
/// link each compaction's memory to the previous one so the WHOLE history survives byte-exact, never
/// just the latest). Each call:
///   - writes a new sequence-numbered frame `workstate::<project>::NNNNNN`,
///   - wiki-links it to the prior round (`[[workstate::<project>::PREV]]` in the body + the shared
///     `link:workstate-<project>` chain concept) so the graph connects newest -> oldest,
///   - refreshes the latest pointer `workstate::<project>` so `load_work_state`/`resume_block` (the hot
///     post-compaction path) still return the newest note unchanged.
/// Returns the new frame's doc_id. This is the chain-aware capture; `save_work_state` remains the
/// latest-only capture for callers that don't want history.
pub fn append_work_state(brain: &mut SaidFile, project: &str, note: &str) -> String {
    let concept = chain_concept(project);
    // next sequence = max existing + 1 (1-based; deterministic from the existing chain).
    let next = work_state_chain_ids(brain, project)
        .iter()
        .filter_map(|id| seq_of(project, id))
        .max()
        .map(|m| m + 1)
        .unwrap_or(1);
    let id = chain_frame_id(project, next);

    // Body carries an explicit [[prev]] wikilink so the chain is visible IN the note too (not only in
    // tags) — newest frame points at the one before it. Round 1 has no predecessor.
    let body = if next > 1 {
        format!(
            "{}\n\n[[{}]]",
            note.trim(),
            chain_frame_id(project, next - 1)
        )
    } else {
        note.trim().to_string()
    };

    let tags = vec![
        WORKSTATE_KIND_TAG.to_string(),
        format!("project:{}", project.trim()),
        format!("link:{}", concept), // the shared chain edge (OKF graph namespace)
    ];
    brain.remember_with_pillar(Some(&id), &body, None, Pillar::Episodic, tags);

    // refresh the latest pointer to the newest note (verbatim, no chain decoration) for the hot path.
    save_work_state(brain, project, note);
    let _ = brain.build_index();
    id
}

/// All chain frame doc_ids for a project (unordered). Uses the shared chain concept edge.
fn work_state_chain_ids(brain: &mut SaidFile, project: &str) -> Vec<String> {
    brain
        .frames_linking_concept(&chain_concept(project))
        .into_iter()
        .filter(|id| seq_of(project, id).is_some())
        .collect()
}

/// The FULL work-state history for a project, newest-first, each note byte-exact. This is what makes
/// the moat complete: after N compactions you can walk back to round 1 and every decision/exact value
/// is still here — the opposite of a decaying in-band summary. history[0] = newest, last = oldest.
pub fn work_state_history(brain: &mut SaidFile, project: &str) -> Vec<String> {
    let mut ids = work_state_chain_ids(brain, project);
    // order by embedded sequence DESC (newest first); deterministic, no metadata-timestamp reliance.
    ids.sort_by_key(|id| std::cmp::Reverse(seq_of(project, id).unwrap_or(0)));
    ids.iter().filter_map(|id| brain.read(id)).collect()
}

/// RE-GROUND: read back the work-state note for a project, byte-exact (the post-compaction recovery).
pub fn load_work_state(brain: &mut SaidFile, project: &str) -> Option<String> {
    brain.read(&frame_id(project)).filter(|s| !s.trim().is_empty())
}

/// The resume block to inject after a compaction — the agent's own note, wrapped in the trusted-channel
/// frame (doc 16) with the "resume exactly here" lead. The note itself is verbatim. None if nothing saved.
pub fn resume_block(brain: &mut SaidFile, project: &str) -> Option<String> {
    load_work_state(brain, project).map(|note| format!(
        "<work_state source=\".said\" kind=\"resume-after-compaction\">\n\
         Resume EXACTLY here (the host compacted; this is your own saved work-state, verbatim):\n\
         {}\n</work_state>", note))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_form_work_state_survives_compaction_verbatim() {
        let p = std::env::temp_dir().join(format!("ws_core_{}.said", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(p.to_string_lossy().as_ref());

        // the agent writes it IN ITS OWN WORDS — no enforced fields — incl the exact fact-dense detail.
        let note = "Building compaction-survival in sca-core (core, not vault).\n\
            Next: wire SessionStart re-ground.\n\
            Decided: OWN not pointer; idempotency threshold = size > 1, NOT >= 1.\n\
            Exact: MIN_COMMON_STEPS=3; recall score = rel_conf*(0.3+0.4*sem+0.3*intent) @ ask.rs:1542; last good commit fd2bf9e.\n\
            Ruled out: Soft-ZCA whitening (tested negative, recall@3 stayed 1/3) -- DON'T retry.\n\
            Plan: step 2 of 3 (work-state continuity).";
        save_work_state(&mut brain, "said-build", note);

        // SIMULATE COMPACTION: in-context detail gone. Re-ground from .said -> the note comes back VERBATIM.
        let recovered = load_work_state(&mut brain, "said-build").expect("work-state must survive");
        assert_eq!(recovered, note.trim(), "the agent's note must survive byte-for-byte");

        let block = resume_block(&mut brain, "said-build").unwrap();
        assert!(block.contains("threshold = size > 1, NOT >= 1"));
        assert!(block.contains("MIN_COMMON_STEPS=3"));
        assert!(block.contains("commit fd2bf9e"));
        assert!(block.contains("Soft-ZCA whitening (tested negative"));

        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
    }

    #[test]
    fn wiki_linked_chain_preserves_full_history_across_n_compactions() {
        // The owner's insight: each compaction should APPEND a new work-state frame that wiki-links back
        // to the previous one, so the WHOLE session history survives byte-exact -- not just the latest.
        // This is strictly better than /compact (which decays) AND than latest-only (which drops history).
        let p = std::env::temp_dir().join(format!("ws_chain_{}.said", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(p.to_string_lossy().as_ref());

        // 7 successive compactions; each round saves a DISTINCT note with a unique anchor.
        const ROUNDS: usize = 7;
        for r in 1..=ROUNDS {
            let note = format!(
                "round {r}: decided thing-{r}; exact value V{r} = {}; ruled out dead-end-{r}.",
                r * 100 + 1
            );
            append_work_state(&mut brain, "said-build", &note);
        }

        // walk the chain newest -> oldest; every round's note must be present, byte-exact, in order.
        let history = work_state_history(&mut brain, "said-build");
        assert_eq!(history.len(), ROUNDS, "every compaction's frame must survive (no overwrite)");
        // history[0] = newest (round 7), history[last] = oldest (round 1)
        for (i, note) in history.iter().enumerate() {
            let round = ROUNDS - i;
            assert!(note.contains(&format!("round {round}:")), "round {round} note byte-exact in chain");
            assert!(note.contains(&format!("V{round} = {}", round * 100 + 1)), "round {round} exact value survives");
        }
        // the OLDEST detail (round 1) -- the first thing a decaying summary loses -- is still byte-exact.
        assert!(history.last().unwrap().contains("round 1: decided thing-1; exact value V1 = 101"));

        // latest-resume still returns the newest (round 7) -- chain doesn't break the hot path.
        assert!(load_work_state(&mut brain, "said-build").unwrap().contains("round 7:"));

        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
    }
}
