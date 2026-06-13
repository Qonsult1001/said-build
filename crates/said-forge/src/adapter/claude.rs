//! Claude Code adapter — writes `.claude/skills/<slug>/SKILL.md`.
//!
//! Per spec §11.1 and the Agent Skills open standard. Claude Code's live
//! directory watcher picks up new files in `.claude/skills/` without
//! requiring a restart.

use crate::adapter::{EditorAdapter, ProjectedStory};
use crate::{sanitize_slug, ForgeError, ForgeResult};
use std::path::{Path, PathBuf};

pub struct ClaudeAdapter;

impl EditorAdapter for ClaudeAdapter {
    fn name(&self) -> &'static str { "claude" }

    fn skill_path(&self, project_root: &Path, slug: &str) -> PathBuf {
        project_root
            .join(".claude")
            .join("skills")
            .join(sanitize_slug(slug))
            .join("SKILL.md")
    }

    fn write_skill(&self, project_root: &Path, proj: &ProjectedStory<'_>) -> ForgeResult<PathBuf> {
        let slug = sanitize_slug(&proj.story.slug);
        let dir = project_root.join(".claude").join("skills").join(&slug);
        std::fs::create_dir_all(&dir).map_err(|cause| ForgeError::Io {
            path: dir.display().to_string(),
            cause,
        })?;
        let body = render_skill(&slug, proj);
        let path = dir.join("SKILL.md");
        std::fs::write(&path, body).map_err(|cause| ForgeError::Io {
            path: path.display().to_string(),
            cause,
        })?;
        Ok(path)
    }

    fn remove_skill(&self, project_root: &Path, slug: &str) -> ForgeResult<bool> {
        let dir = project_root
            .join(".claude")
            .join("skills")
            .join(sanitize_slug(slug));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|cause| ForgeError::Io {
                path: dir.display().to_string(),
                cause,
            })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn render_skill(slug: &str, proj: &ProjectedStory<'_>) -> String {
    let description = truncate(&proj.result.artifacts.spec.overview, 200);
    let title = &proj.story.title;
    let mut out = format!(
        r#"---
name: {slug}
description: {description}
when_to_use: "User invokes /{slug} or asks to implement {title}"
allowed-tools: forge_get forge_status search sym get
argument-hint: "[optional: additional context]"
---

You are implementing the user story **{title}** (slug `{slug}`).

## Workflow

1. Call `forge_get {slug}` to fetch the bundled story, plan, tasks, and brain map.
2. Read the Plan table. For each row, fetch the grounding frames via `get <frame-id>` before writing code.
3. Tick off tasks in `.forge/{slug}/tasks.md` as you complete them.
4. When all tasks are green, summarize the changes and stop. The human will review.

## Rules of engagement

- **Never invent** database columns, endpoint paths, type names, or status codes. If the grounding doesn't have it, ask the user.
- **Preserve proper nouns verbatim** — column names, schema names, HTTP status codes, path params.
- Keep each response focused on **one plan step at a time**.
- If you hit ambiguity, call `search <query>` before guessing.
- Do not modify `.forge/{slug}/` — it is a projection of the `.said` brain.

## Brain map

Grounding frame pointers for quick reference:

"#,
        slug = slug,
        description = description,
        title = title,
    );
    out.push_str(&brain_pointer_list(proj));
    out
}

fn brain_pointer_list(proj: &ProjectedStory<'_>) -> String {
    let mut s = String::new();
    for r in &proj.result.artifacts.brain_refs {
        s.push_str(&format!("- `{}` — {}\n", r.frame_id, r.why_relevant));
    }
    if proj.result.artifacts.brain_refs.is_empty() {
        s.push_str("_(no direct grounding frames — see the Plan table for step-level references)_\n");
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.replace('\n', " ")
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out.replace('\n', " ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::validate::ValidationReport;
    use crate::generator::{
        BrainRef, GeneratedArtifacts, GenerationResult, PlanDoc, PlanStep, SpecDoc, TaskItem,
    };
    use crate::{Story, StoryKind};
    use tempfile::TempDir;

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

    fn result() -> GenerationResult {
        GenerationResult {
            artifacts: GeneratedArtifacts {
                spec: SpecDoc {
                    overview: "Create a pet via POST /pet — returns the stored record.".into(),
                    actors: vec!["api consumer".into()],
                    acceptance_criteria: vec!["Valid body → 201.".into()],
                },
                plan: PlanDoc {
                    steps: vec![PlanStep {
                        id: "S1".into(),
                        action: "Implement handler.".into(),
                        grounding_frame_ids: vec!["f1".into()],
                    }],
                },
                tasks: vec![TaskItem {
                    id: "T1".into(),
                    text: "Write test.".into(),
                    grounding_frame_ids: vec!["f1".into()],
                }],
                brain_refs: vec![BrainRef {
                    frame_id: "f1".into(),
                    why_relevant: "Defines Pet".into(),
                }],
            },
            validation: ValidationReport::default(),
            prompt: "p".into(),
            raw_response: "r".into(),
            provider: "stub".into(),
            model: "stub".into(),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            duration_ms: 0,
        }
    }

    #[test]
    fn skill_path_is_directory_based() {
        let a = ClaudeAdapter;
        let td = TempDir::new().unwrap();
        let p = a.skill_path(td.path(), "post-pet");
        // Normalize separators for OS-agnostic check
        let p_str = p.to_string_lossy().replace('\\', "/");
        assert!(p_str.ends_with(".claude/skills/post-pet/SKILL.md"), "got {}", p_str);
    }

    #[test]
    fn write_skill_creates_directory_and_file() {
        let a = ClaudeAdapter;
        let td = TempDir::new().unwrap();
        let s = story();
        let r = result();
        let proj = ProjectedStory { story: &s, result: &r };
        let path = a.write_skill(td.path(), &proj).unwrap();
        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.starts_with("---\n"));
        assert!(body.contains("name: post-pet"));
        assert!(body.contains("when_to_use:"));
        assert!(body.contains("allowed-tools: forge_get forge_status search sym get"));
        assert!(body.contains("forge_get post-pet"));
        assert!(body.contains("- `f1` — Defines Pet"));
    }

    #[test]
    fn write_skill_idempotent_overwrites_body() {
        let a = ClaudeAdapter;
        let td = TempDir::new().unwrap();
        let s = story();
        let r = result();
        let proj = ProjectedStory { story: &s, result: &r };
        let p1 = a.write_skill(td.path(), &proj).unwrap();
        let p2 = a.write_skill(td.path(), &proj).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn remove_skill_deletes_directory() {
        let a = ClaudeAdapter;
        let td = TempDir::new().unwrap();
        let s = story();
        let r = result();
        let proj = ProjectedStory { story: &s, result: &r };
        a.write_skill(td.path(), &proj).unwrap();
        assert!(a.remove_skill(td.path(), "post-pet").unwrap());
        assert!(!a.remove_skill(td.path(), "post-pet").unwrap());
    }

    #[test]
    fn description_truncated_to_200_chars() {
        let a = ClaudeAdapter;
        let mut r = result();
        r.artifacts.spec.overview = "x".repeat(500);
        let s = story();
        let td = TempDir::new().unwrap();
        let proj = ProjectedStory { story: &s, result: &r };
        let path = a.write_skill(td.path(), &proj).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let desc_line = body.lines().find(|l| l.starts_with("description:")).unwrap();
        assert!(desc_line.len() < 230);
        assert!(desc_line.ends_with('…'));
    }
}
