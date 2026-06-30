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
}
