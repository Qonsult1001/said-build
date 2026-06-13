//! Post-generation validators.
//!
//! Per spec §9.4 and §2.3. Four checks:
//! 1. Word counts per field
//! 2. Grounding-check — every grounding_frame_ids entry exists in the grounding set
//! 3. Duplicate detection (Jaccard > 0.85 between peer items)
//! 4. NEEDS-INPUT inventory

use crate::generator::GeneratedArtifacts;
use crate::grounding::GroundingHit;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationReport {
    pub word_count_warnings: Vec<WordCountWarning>,
    pub fabricated_frame_ids: Vec<String>,
    pub duplicate_warnings: Vec<DuplicateWarning>,
    pub needs_input: Vec<String>,
    pub is_retry_worthy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordCountWarning {
    pub field_path: String,
    pub word_count: u32,
    pub max_allowed: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateWarning {
    pub a_path: String,
    pub b_path: String,
    pub jaccard: f32,
}

pub fn run_all(artifacts: &GeneratedArtifacts, hits: &[GroundingHit]) -> ValidationReport {
    let mut report = ValidationReport::default();
    report.word_count_warnings.extend(word_counts(artifacts));
    report.fabricated_frame_ids = grounding_check(artifacts, hits);
    report.duplicate_warnings = duplicate_check(artifacts);
    report.needs_input = collect_needs_input(artifacts);

    let total_items = artifacts.spec.acceptance_criteria.len()
        + artifacts.plan.steps.len()
        + artifacts.tasks.len();
    let egregious = report
        .word_count_warnings
        .iter()
        .filter(|w| w.word_count > (w.max_allowed * 2))
        .count();
    report.is_retry_worthy = total_items > 0 && (egregious * 10 / total_items.max(1)) > 3;
    report
}

pub fn word_counts(a: &GeneratedArtifacts) -> Vec<WordCountWarning> {
    let mut out = Vec::new();
    for (idx, ac) in a.spec.acceptance_criteria.iter().enumerate() {
        if let Some(w) = check_item(&format!("spec.acceptance_criteria[{}]", idx), ac, 100) {
            out.push(w);
        }
    }
    for (idx, step) in a.plan.steps.iter().enumerate() {
        if let Some(w) = check_item(&format!("plan.steps[{}].action", idx), &step.action, 100) {
            out.push(w);
        }
    }
    for (idx, t) in a.tasks.iter().enumerate() {
        if let Some(w) = check_item(&format!("tasks[{}].text", idx), &t.text, 100) {
            out.push(w);
        }
    }
    let overview_words = word_count(&a.spec.overview);
    if overview_words > 250 {
        out.push(WordCountWarning {
            field_path: "spec.overview".into(),
            word_count: overview_words,
            max_allowed: 250,
            preview: preview(&a.spec.overview, 120),
        });
    }
    out
}

fn check_item(path: &str, text: &str, max_allowed: u32) -> Option<WordCountWarning> {
    let wc = word_count(text);
    if wc > max_allowed {
        Some(WordCountWarning {
            field_path: path.into(),
            word_count: wc,
            max_allowed,
            preview: preview(text, 120),
        })
    } else {
        None
    }
}

fn word_count(text: &str) -> u32 {
    text.split_whitespace().count() as u32
}

fn preview(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

pub fn grounding_check(a: &GeneratedArtifacts, hits: &[GroundingHit]) -> Vec<String> {
    let known: HashSet<&str> = hits.iter().map(|h| h.frame_id.as_str()).collect();
    let mut fabricated: Vec<String> = Vec::new();
    for step in &a.plan.steps {
        for fid in &step.grounding_frame_ids {
            if !known.contains(fid.as_str()) && !fabricated.contains(fid) {
                fabricated.push(fid.clone());
            }
        }
    }
    for t in &a.tasks {
        for fid in &t.grounding_frame_ids {
            if !known.contains(fid.as_str()) && !fabricated.contains(fid) {
                fabricated.push(fid.clone());
            }
        }
    }
    for b in &a.brain_refs {
        if !known.contains(b.frame_id.as_str()) && !fabricated.contains(&b.frame_id) {
            fabricated.push(b.frame_id.clone());
        }
    }
    fabricated
}

pub fn duplicate_check(a: &GeneratedArtifacts) -> Vec<DuplicateWarning> {
    let mut out = Vec::new();
    let ac = &a.spec.acceptance_criteria;
    for i in 0..ac.len() {
        for j in (i + 1)..ac.len() {
            let jac = jaccard(&ac[i], &ac[j]);
            if jac > 0.85 {
                out.push(DuplicateWarning {
                    a_path: format!("spec.acceptance_criteria[{}]", i),
                    b_path: format!("spec.acceptance_criteria[{}]", j),
                    jaccard: jac,
                });
            }
        }
    }
    let steps = &a.plan.steps;
    for i in 0..steps.len() {
        for j in (i + 1)..steps.len() {
            let jac = jaccard(&steps[i].action, &steps[j].action);
            if jac > 0.85 {
                out.push(DuplicateWarning {
                    a_path: format!("plan.steps[{}]", i),
                    b_path: format!("plan.steps[{}]", j),
                    jaccard: jac,
                });
            }
        }
    }
    let ts = &a.tasks;
    for i in 0..ts.len() {
        for j in (i + 1)..ts.len() {
            let jac = jaccard(&ts[i].text, &ts[j].text);
            if jac > 0.85 {
                out.push(DuplicateWarning {
                    a_path: format!("tasks[{}]", i),
                    b_path: format!("tasks[{}]", j),
                    jaccard: jac,
                });
            }
        }
    }
    out
}

fn jaccard(a: &str, b: &str) -> f32 {
    let normalize = |s: &str| -> HashSet<String> {
        s.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|w| !w.is_empty())
            .collect()
    };
    let sa = normalize(a);
    let sb = normalize(b);
    if sa.is_empty() && sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    if union == 0 { 0.0 } else { inter as f32 / union as f32 }
}

pub fn collect_needs_input(a: &GeneratedArtifacts) -> Vec<String> {
    let mut out = Vec::new();
    for ac in &a.spec.acceptance_criteria {
        if ac.contains("NEEDS-INPUT") {
            out.push(ac.clone());
        }
    }
    for step in &a.plan.steps {
        if step.action.contains("NEEDS-INPUT") {
            out.push(format!("[{}] {}", step.id, step.action));
        }
    }
    for t in &a.tasks {
        if t.text.contains("NEEDS-INPUT") {
            out.push(format!("[{}] {}", t.id, t.text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::{BrainRef, GeneratedArtifacts, PlanDoc, PlanStep, SpecDoc, TaskItem};
    use crate::grounding::GroundingHit;
    use crate::story::Pillar;

    fn hit(id: &str) -> GroundingHit {
        GroundingHit {
            frame_id: id.into(),
            tag: "code:x".into(),
            score: 1.0,
            query_origin: "test".into(),
            snippet: id.into(),
            pillar: Pillar::Code,
            authority: None,
        }
    }

    fn artifacts(
        spec_criteria: Vec<&str>,
        step_actions: Vec<&str>,
        step_frame_ids: Vec<Vec<&str>>,
        tasks: Vec<&str>,
        brain_ids: Vec<&str>,
    ) -> GeneratedArtifacts {
        GeneratedArtifacts {
            spec: SpecDoc {
                overview: "overview".into(),
                actors: vec!["tester".into()],
                acceptance_criteria: spec_criteria.into_iter().map(String::from).collect(),
            },
            plan: PlanDoc {
                steps: step_actions
                    .into_iter()
                    .zip(step_frame_ids)
                    .enumerate()
                    .map(|(i, (a, ids))| PlanStep {
                        id: format!("S{}", i + 1),
                        action: a.into(),
                        grounding_frame_ids: ids.into_iter().map(String::from).collect(),
                    })
                    .collect(),
            },
            tasks: tasks
                .into_iter()
                .enumerate()
                .map(|(i, t)| TaskItem {
                    id: format!("T{}", i + 1),
                    text: t.into(),
                    grounding_frame_ids: Vec::new(),
                })
                .collect(),
            brain_refs: brain_ids
                .into_iter()
                .map(|id| BrainRef {
                    frame_id: id.into(),
                    why_relevant: "because".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn word_count_flags_overlong_task() {
        let long = "word ".repeat(150);
        let a = artifacts(vec!["short criterion."], vec!["short action."], vec![vec![]], vec![&long], vec![]);
        let ws = word_counts(&a);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].field_path, "tasks[0].text");
        assert!(ws[0].word_count > 100);
    }

    #[test]
    fn word_count_passes_compliant_items() {
        let a = artifacts(
            vec!["A short valid criterion."],
            vec!["A short valid action."],
            vec![vec![]],
            vec!["A short valid task."],
            vec![],
        );
        assert!(word_counts(&a).is_empty());
    }

    #[test]
    fn grounding_check_flags_fabricated_ids() {
        let hits = vec![hit("real-1"), hit("real-2")];
        let a = artifacts(
            vec!["ok."],
            vec!["Use real-1"],
            vec![vec!["real-1", "FAKE-ID"]],
            vec!["Use real-2."],
            vec!["real-1", "ALSO-FAKE"],
        );
        let fab = grounding_check(&a, &hits);
        assert_eq!(fab.len(), 2);
        assert!(fab.contains(&"FAKE-ID".to_string()));
        assert!(fab.contains(&"ALSO-FAKE".to_string()));
    }

    #[test]
    fn grounding_check_clean_when_all_real() {
        let hits = vec![hit("f1"), hit("f2")];
        let a = artifacts(
            vec!["ok."],
            vec!["Use f1"],
            vec![vec!["f1"]],
            vec!["Use f2."],
            vec!["f1", "f2"],
        );
        assert!(grounding_check(&a, &hits).is_empty());
    }

    #[test]
    fn duplicate_check_flags_near_identical_criteria() {
        let a = artifacts(
            vec![
                "Valid POST accounts creates row in dbo.accounts and returns 201.",
                "Valid POST accounts creates row in dbo.accounts and returns 201 OK.",
            ],
            vec!["distinct action one."],
            vec![vec![]],
            vec!["distinct task one."],
            vec![],
        );
        let dups = duplicate_check(&a);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].a_path, "spec.acceptance_criteria[0]");
    }

    #[test]
    fn duplicate_check_ignores_different_criteria() {
        let a = artifacts(
            vec!["Valid POST creates a row.", "Invalid body returns 400."],
            vec!["distinct action."],
            vec![vec![]],
            vec!["distinct task."],
            vec![],
        );
        assert!(duplicate_check(&a).is_empty());
    }

    #[test]
    fn collect_needs_input_finds_tokens() {
        let a = artifacts(
            vec![
                "Pass case returns 201.",
                "NEEDS-INPUT:pillar:code — no stored procedure listed in grounding.",
            ],
            vec!["NEEDS-INPUT: column names for the audit table."],
            vec![vec![]],
            vec!["Test the happy path."],
            vec![],
        );
        let ni = collect_needs_input(&a);
        assert_eq!(ni.len(), 2);
    }

    #[test]
    fn run_all_aggregates_everything() {
        let hits = vec![hit("f1")];
        let a = artifacts(
            vec!["NEEDS-INPUT: no API description."],
            vec!["Step references real f1.", "Step references FAKE."],
            vec![vec!["f1"], vec!["FAKE"]],
            vec!["short task."],
            vec!["f1"],
        );
        let r = run_all(&a, &hits);
        assert_eq!(r.fabricated_frame_ids, vec!["FAKE".to_string()]);
        assert_eq!(r.needs_input.len(), 1);
        assert!(!r.is_retry_worthy);
    }
}
