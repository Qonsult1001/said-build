//! Markdown renderers — one per file in `.forge/<slug>/`.
//!
//! Per spec §10.1–§10.4. Output is Spec-Kit-compatible.

use crate::generator::{BrainRef, PlanDoc, SpecDoc, TaskItem};
use crate::grounding::GroundingHit;
use crate::Story;

pub fn story_md(story: &Story, spec: &SpecDoc) -> String {
    let mut s = String::new();
    s.push_str("---\n");
    s.push_str(&format!("slug: {}\n", story.slug));
    s.push_str(&format!("kind: {}\n", story.kind));
    s.push_str(&format!("directive_hash: {}\n", story.directive_hash));
    s.push_str(&format!("source_adapter: {}\n", story.source_adapter));
    s.push_str(&format!("source_anchor: {}\n", story.source_anchor));
    s.push_str("---\n\n");
    s.push_str(&format!("# {}\n\n", story.title));
    s.push_str("## Overview\n\n");
    s.push_str(&spec.overview);
    s.push_str("\n\n");
    if !spec.actors.is_empty() {
        s.push_str("## Actors\n\n");
        for a in &spec.actors {
            s.push_str(&format!("- {}\n", a));
        }
        s.push('\n');
    }
    s.push_str("## Acceptance Criteria\n\n");
    for (i, ac) in spec.acceptance_criteria.iter().enumerate() {
        s.push_str(&format!("{}. {}\n", i + 1, ac));
    }
    s.push_str("\n## Grounding\n\nSee [brain.md](brain.md) for frame references.\n");
    s
}

pub fn plan_md(story: &Story, plan: &PlanDoc) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Plan: {}\n\n", story.title));
    s.push_str("| Step | Action | Grounding |\n");
    s.push_str("|---|---|---|\n");
    for step in &plan.steps {
        let grounding = if step.grounding_frame_ids.is_empty() {
            "_(none)_".into()
        } else {
            step.grounding_frame_ids
                .iter()
                .map(|id| format!("`{}`", id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let action_escaped = step.action.replace('|', "\\|").replace('\n', " ");
        s.push_str(&format!("| {} | {} | {} |\n", step.id, action_escaped, grounding));
    }
    s
}

pub fn tasks_md(story: &Story, tasks: &[TaskItem]) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Tasks: {}\n\n", story.title));
    for t in tasks {
        let grounding = if t.grounding_frame_ids.is_empty() {
            String::new()
        } else {
            format!(
                "  _(grounding: {})_",
                t.grounding_frame_ids.iter().map(|id| format!("`{}`", id)).collect::<Vec<_>>().join(", ")
            )
        };
        s.push_str(&format!("- [ ] **{}** — {}{}\n", t.id, t.text.trim(), grounding));
    }
    s
}

pub fn brain_md(story: &Story, refs: &[BrainRef], hits: &[GroundingHit]) -> String {
    brain_md_with_tech(story, refs, hits, None)
}

/// Same as `brain_md` but accepts a pre-rendered technical-grounding block
/// that the runner builds from the SQL catalog when `forge-docs` is enabled.
/// The block is inserted right after the story title, before the directly-
/// grounded frames list — so the developer sees the SQL context first.
pub fn brain_md_with_tech(
    story: &Story,
    refs: &[BrainRef],
    hits: &[GroundingHit],
    technical_grounding_md: Option<&str>,
) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Brain Map: {}\n\n", story.title));
    if let Some(tg) = technical_grounding_md {
        s.push_str(tg);
        if !tg.ends_with('\n') {
            s.push('\n');
        }
        s.push('\n');
    }
    s.push_str("## Directly Grounded Frames\n\n");
    for r in refs {
        s.push_str(&format!("- `{}` — {}\n", r.frame_id, r.why_relevant));
    }
    if refs.is_empty() {
        s.push_str("_(no brain_refs emitted)_\n");
    }
    s.push_str("\n## All Retrieved Grounding Frames (for deeper exploration)\n\n");
    for h in hits {
        s.push_str(&format!(
            "- `{}` — pillar: `{}` — score: {:.3} — origin: `{}`\n",
            h.frame_id, h.pillar, h.score, h.query_origin
        ));
    }
    s.push_str("\n## Commands to Navigate\n\n");
    s.push_str("- `said ask \"<query>\"` — semantic question over the brain\n");
    s.push_str("- `said sym <name>` — symbol lookup\n");
    s.push_str("- `said get <frame-id>` — fetch a specific frame's full body\n");
    s.push_str(&format!("- `said forge show {}` — re-print this story's bundled markdown\n", story.slug));
    s.push_str("\n## MCP Tools the AI Editor Uses\n\n");
    s.push_str(&format!("- `forge_get {}` — the bundled story + plan + tasks + this brain map\n", story.slug));
    s.push_str("- `search <query>` — semantic search over the whole brain\n");
    s.push_str("- `get <frame-id>` — fetch a specific frame\n");
    s.push_str("- `sym <name>` — symbol lookup\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::PlanStep;
    use crate::story::Pillar;
    use crate::StoryKind;

    fn story() -> Story {
        Story {
            slug: "post-pet".into(),
            title: "Add a new pet".into(),
            raw_text: "POST /pet".into(),
            kind: StoryKind::ApiEndpoint,
            fields: Default::default(),
            directive_hash: "a3f91".into(),
            source_adapter: "openapi".into(),
            source_anchor: "paths./pet.post".into(),
        }
    }

    #[test]
    fn story_md_has_frontmatter_and_sections() {
        let spec = SpecDoc {
            overview: "Short overview.".into(),
            actors: vec!["user".into()],
            acceptance_criteria: vec!["one".into(), "two".into()],
        };
        let out = story_md(&story(), &spec);
        assert!(out.starts_with("---\n"));
        assert!(out.contains("slug: post-pet"));
        assert!(out.contains("## Overview"));
        assert!(out.contains("## Actors"));
        assert!(out.contains("## Acceptance Criteria"));
        assert!(out.contains("1. one"));
        assert!(out.contains("2. two"));
    }

    #[test]
    fn plan_md_renders_table() {
        let plan = PlanDoc {
            steps: vec![PlanStep {
                id: "S1".into(),
                action: "Do the thing | with pipe".into(),
                grounding_frame_ids: vec!["f1".into()],
            }],
        };
        let out = plan_md(&story(), &plan);
        assert!(out.contains("| S1 | Do the thing \\| with pipe | `f1` |"));
    }

    #[test]
    fn tasks_md_emits_checkboxes() {
        let tasks = vec![TaskItem {
            id: "T1".into(),
            text: "write test".into(),
            grounding_frame_ids: vec!["f1".into(), "f2".into()],
        }];
        let out = tasks_md(&story(), &tasks);
        assert!(out.contains("- [ ] **T1** — write test"));
        assert!(out.contains("grounding: `f1`, `f2`"));
    }

    #[test]
    fn brain_md_includes_refs_and_hits() {
        let refs = vec![BrainRef {
            frame_id: "f1".into(),
            why_relevant: "defines shape".into(),
        }];
        let hits = vec![GroundingHit {
            frame_id: "f2".into(),
            tag: "code:pet".into(),
            score: 0.87,
            query_origin: "tag".into(),
            snippet: "x".into(),
            pillar: Pillar::External,
            authority: None,
        }];
        let out = brain_md(&story(), &refs, &hits);
        assert!(out.contains("`f1` — defines shape"));
        assert!(out.contains("`f2` — pillar: `External`"));
        assert!(out.contains("said forge show post-pet"));
        assert!(out.contains("forge_get post-pet"));
    }

    #[test]
    fn brain_md_with_tech_inserts_grounding_block_before_refs() {
        let tg = "## Technical grounding — POST /pet\n\n### Tables touched\n\n- `pet.cpf_Pets`\n";
        let out = brain_md_with_tech(&story(), &[], &[], Some(tg));
        assert!(out.contains("# Brain Map"));
        assert!(out.contains("## Technical grounding"));
        // Grounding must come BEFORE "## Directly Grounded Frames"
        let tech_idx = out.find("Technical grounding").unwrap();
        let refs_idx = out.find("Directly Grounded Frames").unwrap();
        assert!(tech_idx < refs_idx);
    }
}
