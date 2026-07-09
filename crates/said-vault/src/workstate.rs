//! Vault work-state — COMPACTION-SURVIVAL memory (docs/said-structure/30-beat-them-benchmark.md).
//!
//! The #1 unfixable weakness of Claude/Cursor/Kimi: when the host compacts/summarizes the context window,
//! the fact-dense detail (exact thresholds, IDs, decisions, the next step) is LOST — the agent "goes
//! stupid, doesn't know what it was doing." That cannot be fixed from INSIDE the window (the window is
//! what's being compacted). `.said` lives OUTSIDE it: a work-state frame stored byte-exact in the vault,
//! re-grounded on the next turn AS IF NOTHING DISAPPEARED.
//!
//! Stored as one frame `vault:workstate:<project>` (latest wins — there's one current work-state per
//! project). Byte-exact: `remember_with_pillar(Some(id), json, ..)` + `read(id)` is an exact string
//! roundtrip, and the captured `exact_values` are preserved verbatim (the thing summarization paraphrases
//! away). The schema is exactly doc 30's: task / next_step / decisions / exact_values / files /
//! blockers+ruled_out / plan_status.

use serde::{Deserialize, Serialize};

use sca_core::frames::Pillar;
use sca_core::said_file::SaidFile;

/// The current mid-task work-state for a project — the lossy detail that must survive a compaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkState {
    pub project: String,
    /// What I'm doing right now ("building compaction-survival in said-vault").
    pub task: String,
    /// The immediate next action — the thing the agent forgets after compaction.
    pub next_step: String,
    /// Choices made + their exact form ("chose Postgres"; "threshold = size > 1").
    pub decisions: Vec<String>,
    /// The fact-dense detail summarization discards first: thresholds, IDs, flags, file:line, commits,
    /// numbers. Preserved VERBATIM — this is what makes re-grounding "byte-exact", not paraphrased.
    pub exact_values: Vec<String>,
    /// The working set (files being edited).
    pub files: Vec<String>,
    /// What's stuck.
    pub blockers: Vec<String>,
    /// Dead ends already tried — don't repeat them after compaction.
    pub ruled_out: Vec<String>,
    /// Where in the plan we are (the bit Claude/Cursor lose on resume).
    pub plan_status: String,
    /// Unix seconds this state was captured.
    pub updated_at: u64,
}

impl WorkState {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("WorkState serde must not fail")
    }
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
    /// Render the state as the plain-facts block re-injected into the host on the next turn after a
    /// compaction — the "where you left off, exactly" ground-truth. Pure facts (doc 16 trusted channel).
    pub fn render_resume(&self) -> String {
        let mut s = String::from("<work_state source=\".said\" kind=\"resume-after-compaction\">\n");
        s.push_str("Resume EXACTLY here (the host compacted; this is the lost detail, verbatim):\n");
        s.push_str(&format!("TASK: {}\n", self.task));
        s.push_str(&format!("NEXT: {}\n", self.next_step));
        if !self.plan_status.is_empty() { s.push_str(&format!("PLAN: {}\n", self.plan_status)); }
        if !self.decisions.is_empty() { s.push_str(&format!("DECISIONS: {}\n", self.decisions.join(" | "))); }
        if !self.exact_values.is_empty() { s.push_str(&format!("EXACT: {}\n", self.exact_values.join(" | "))); }
        if !self.files.is_empty() { s.push_str(&format!("FILES: {}\n", self.files.join(", "))); }
        if !self.blockers.is_empty() { s.push_str(&format!("BLOCKERS: {}\n", self.blockers.join(" | "))); }
        if !self.ruled_out.is_empty() { s.push_str(&format!("RULED OUT (don't retry): {}\n", self.ruled_out.join(" | "))); }
        s.push_str("</work_state>");
        s
    }
}

fn frame_id(project: &str) -> String {
    format!("vault:workstate:{}", project.trim())
}

/// CAPTURE (save/overwrite) the current work-state for a project — latest wins. Byte-exact storage.
pub fn save(brain: &mut SaidFile, mut state: WorkState) -> String {
    state.updated_at = sca_core::time_compat::unix_secs();
    let id = frame_id(&state.project);
    let tags = vec![
        "vault:workstate".to_string(),
        format!("vault:workstate:project:{}", state.project.trim()),
    ];
    brain.remember_with_pillar(Some(&id), &state.to_json(), None, Pillar::Document, tags);
    id
}

/// RE-GROUND: read back the work-state for a project, byte-exact (the post-compaction recovery).
pub fn load(brain: &mut SaidFile, project: &str) -> Option<WorkState> {
    brain.read(&frame_id(project)).and_then(|j| WorkState::from_json(&j).ok())
}

/// The resume block to inject after a compaction, or None if there's no captured state.
pub fn resume_block(brain: &mut SaidFile, project: &str) -> Option<String> {
    load(brain, project).map(|s| s.render_resume())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workstate_roundtrips_byte_exact_through_the_vault() {
        let p = std::env::temp_dir().join(format!("ws_{}.said", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let mut brain = SaidFile::create(p.to_string_lossy().as_ref());

        // capture rich state with the EXACT fact-dense detail summarization would paraphrase away.
        let state = WorkState {
            project: "said-build".into(),
            task: "building compaction-survival in said-vault".into(),
            next_step: "wire SessionStart re-ground + write the e2e".into(),
            decisions: vec!["OWN not pointer".into(), "threshold = size > 1, NOT >= 1".into()],
            exact_values: vec!["MIN_COMMON_STEPS=3".into(), "commit fd2bf9e".into(), "ask.rs:1542 score=rel_conf*(0.3+0.4*sem+0.3*intent)".into()],
            files: vec!["crates/said-vault/src/workstate.rs".into()],
            blockers: vec![],
            ruled_out: vec!["Soft-ZCA whitening (tested negative)".into()],
            plan_status: "step 2 of 3 (work-state continuity)".into(),
            ..Default::default()
        };
        save(&mut brain, state.clone());

        // SIMULATE COMPACTION: the in-context detail is gone. Re-ground from the vault.
        let recovered = load(&mut brain, "said-build").expect("work-state must survive");

        // byte-exact: every fact-dense value comes back VERBATIM (not paraphrased) — the whole point.
        assert_eq!(recovered.task, state.task);
        assert_eq!(recovered.next_step, state.next_step);
        assert_eq!(recovered.decisions, state.decisions);
        assert_eq!(recovered.exact_values, state.exact_values, "EXACT values must survive verbatim");
        assert_eq!(recovered.ruled_out, state.ruled_out);
        assert_eq!(recovered.plan_status, state.plan_status);

        // the resume block carries the exact thresholds/commits an LLM summary would have dropped.
        let block = resume_block(&mut brain, "said-build").unwrap();
        assert!(block.contains("threshold = size > 1, NOT >= 1"));
        assert!(block.contains("MIN_COMMON_STEPS=3"));
        assert!(block.contains("commit fd2bf9e"));
        assert!(block.contains("Soft-ZCA whitening (tested negative)"));

        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(format!("{}.spill", p.to_string_lossy()));
    }
}
