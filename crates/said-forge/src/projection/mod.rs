//! Projection — writes a `GeneratedArtifacts` bundle to `.forge/<slug>/` atomically.
//!
//! Per spec §10. Folder is a projection of the brain's forge frames —
//! regenerating from frames always yields the same on-disk layout. We write
//! to `.forge/<slug>.tmp-<pid>/` and atomically rename to `.forge/<slug>/`.

pub mod render;

use crate::generator::GenerationResult;
use crate::grounding::GroundingHit;
use crate::{sanitize_slug, ForgeError, ForgeResult, Story};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionMeta {
    pub directive_hash: String,
    pub slug: String,
    pub run_id: String,
    pub generated_at_utc: i64,
    pub llm_provider: String,
    pub llm_model: String,
    pub status: String,
}

/// Write the full feature folder atomically.
pub fn write_folder(
    project_root: &Path,
    story: &Story,
    result: &GenerationResult,
    hits: &[GroundingHit],
    run_id: &str,
) -> ForgeResult<PathBuf> {
    write_folder_with_grounding(project_root, story, result, hits, run_id, None)
}

/// Variant that inlines a pre-rendered TechnicalGrounding block into
/// `brain.md` — used by the runner when the workspace has a SQL catalog.
pub fn write_folder_with_grounding(
    project_root: &Path,
    story: &Story,
    result: &GenerationResult,
    hits: &[GroundingHit],
    run_id: &str,
    technical_grounding_md: Option<&str>,
) -> ForgeResult<PathBuf> {
    let slug = sanitize_slug(&story.slug);
    let dir = project_root.join(".forge").join(&slug);
    let tmp_dir = project_root
        .join(".forge")
        .join(format!("{}.tmp-{}", slug, std::process::id()));

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir).map_err(|cause| ForgeError::Io {
            path: tmp_dir.display().to_string(),
            cause,
        })?;
    }
    std::fs::create_dir_all(&tmp_dir).map_err(|cause| ForgeError::Io {
        path: tmp_dir.display().to_string(),
        cause,
    })?;

    write_file(&tmp_dir, "story.md", &render::story_md(story, &result.artifacts.spec))?;
    write_file(&tmp_dir, "plan.md", &render::plan_md(story, &result.artifacts.plan))?;
    write_file(&tmp_dir, "tasks.md", &render::tasks_md(story, &result.artifacts.tasks))?;
    write_file(
        &tmp_dir,
        "brain.md",
        &render::brain_md_with_tech(
            story,
            &result.artifacts.brain_refs,
            hits,
            technical_grounding_md,
        ),
    )?;

    let meta = ProjectionMeta {
        directive_hash: story.directive_hash.clone(),
        slug: slug.clone(),
        run_id: run_id.to_string(),
        generated_at_utc: now_utc(),
        llm_provider: result.provider.clone(),
        llm_model: result.model.clone(),
        status: "completed".into(),
    };
    let meta_json = serde_json::to_string_pretty(&meta).map_err(ForgeError::Serde)?;
    write_file(&tmp_dir, ".forge-meta", &meta_json)?;

    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|cause| ForgeError::Io {
            path: dir.display().to_string(),
            cause,
        })?;
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|cause| ForgeError::Io {
            path: parent.display().to_string(),
            cause,
        })?;
    }
    std::fs::rename(&tmp_dir, &dir).map_err(|cause| ForgeError::Io {
        path: tmp_dir.display().to_string(),
        cause,
    })?;
    Ok(dir)
}

/// Remove a projected feature folder (used by `forge reset`).
pub fn remove_folder(project_root: &Path, slug: &str) -> ForgeResult<bool> {
    let sanitized = sanitize_slug(slug);
    let dir = project_root.join(".forge").join(&sanitized);
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

/// "Completed" check (θ from spec §13.2): folder exists and contains all four
/// markdown files + `.forge-meta`.
pub fn is_complete(project_root: &Path, slug: &str) -> bool {
    let sanitized = sanitize_slug(slug);
    let dir = project_root.join(".forge").join(&sanitized);
    ["story.md", "plan.md", "tasks.md", "brain.md", ".forge-meta"]
        .iter()
        .all(|f| dir.join(f).exists())
}

fn write_file(dir: &Path, name: &str, content: &str) -> ForgeResult<()> {
    let path = dir.join(name);
    std::fs::write(&path, content).map_err(|cause| ForgeError::Io {
        path: path.display().to_string(),
        cause,
    })
}

fn now_utc() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::validate::ValidationReport;
    use crate::generator::{
        BrainRef, GeneratedArtifacts, PlanDoc, PlanStep, SpecDoc, TaskItem,
    };
    use crate::story::Pillar;
    use crate::StoryKind;
    use tempfile::TempDir;

    fn story() -> Story {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("method".into(), serde_json::json!("POST"));
        fields.insert("path".into(), serde_json::json!("/pet"));
        Story {
            slug: "post-pet".into(),
            title: "Add a new pet".into(),
            raw_text: "POST /pet".into(),
            kind: StoryKind::ApiEndpoint,
            fields,
            directive_hash: "a3f91".into(),
            source_adapter: "openapi".into(),
            source_anchor: "paths./pet.post".into(),
        }
    }

    fn result() -> GenerationResult {
        GenerationResult {
            artifacts: GeneratedArtifacts {
                spec: SpecDoc {
                    overview: "POST /pet creates a Pet resource.".into(),
                    actors: vec!["api consumer".into()],
                    acceptance_criteria: vec!["Valid body creates row, returns 201.".into()],
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
                    text: "Write integration test.".into(),
                    grounding_frame_ids: vec!["f1".into()],
                }],
                brain_refs: vec![BrainRef {
                    frame_id: "f1".into(),
                    why_relevant: "Defines Pet.".into(),
                }],
            },
            validation: ValidationReport::default(),
            prompt: "p".into(),
            raw_response: "r".into(),
            provider: "stub".into(),
            model: "stub-model".into(),
            input_tokens: 1000,
            output_tokens: 250,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            duration_ms: 5,
        }
    }

    fn hit(id: &str) -> GroundingHit {
        GroundingHit {
            frame_id: id.into(),
            tag: "code:x".into(),
            score: 1.0,
            query_origin: "t".into(),
            snippet: "snippet".into(),
            pillar: Pillar::Code,
            authority: None,
        }
    }

    #[test]
    fn write_folder_creates_all_files() {
        let td = TempDir::new().unwrap();
        let path = write_folder(td.path(), &story(), &result(), &[hit("f1")], "r1").unwrap();
        for f in ["story.md", "plan.md", "tasks.md", "brain.md", ".forge-meta"] {
            assert!(path.join(f).exists(), "{} missing", f);
        }
    }

    #[test]
    fn write_folder_is_idempotent() {
        let td = TempDir::new().unwrap();
        write_folder(td.path(), &story(), &result(), &[hit("f1")], "r1").unwrap();
        let path = write_folder(td.path(), &story(), &result(), &[hit("f1")], "r2").unwrap();
        let meta: ProjectionMeta =
            serde_json::from_str(&std::fs::read_to_string(path.join(".forge-meta")).unwrap())
                .unwrap();
        assert_eq!(meta.run_id, "r2");
    }

    #[test]
    fn write_folder_sanitizes_slug() {
        let mut s = story();
        s.slug = "POST /pet/{petId}".into();
        let td = TempDir::new().unwrap();
        let path = write_folder(td.path(), &s, &result(), &[hit("f1")], "r1").unwrap();
        assert!(path.ends_with("post-pet-petid"));
    }

    #[test]
    fn is_complete_returns_true_after_write() {
        let td = TempDir::new().unwrap();
        write_folder(td.path(), &story(), &result(), &[hit("f1")], "r1").unwrap();
        assert!(is_complete(td.path(), "post-pet"));
    }

    #[test]
    fn is_complete_returns_false_when_missing_file() {
        let td = TempDir::new().unwrap();
        let path = write_folder(td.path(), &story(), &result(), &[hit("f1")], "r1").unwrap();
        std::fs::remove_file(path.join("story.md")).unwrap();
        assert!(!is_complete(td.path(), "post-pet"));
    }

    #[test]
    fn remove_folder_deletes() {
        let td = TempDir::new().unwrap();
        write_folder(td.path(), &story(), &result(), &[hit("f1")], "r1").unwrap();
        assert!(remove_folder(td.path(), "post-pet").unwrap());
        assert!(!is_complete(td.path(), "post-pet"));
        assert!(!remove_folder(td.path(), "post-pet").unwrap());
    }
}
