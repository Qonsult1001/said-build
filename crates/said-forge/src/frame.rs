//! Frame I/O abstraction for said-forge.
//!
//! Per spec §6.2. All forge artifacts are normal `.said` frames. This module
//! defines the **`BrainIo` trait** — the single seam forge uses to read/write
//! frames. Tests swap in a stub; production wires up `SaidFileBrainIo` in
//! `said-cli` where it can link against `sca-core::SaidFile`.
//!
//! We split the adapter out of the said-forge library to keep the crate
//! sca-core-version-independent at compile time. said-forge's own test
//! suite runs on an in-memory stub, never touching the real brain.

use crate::generator::GenerationResult;
use crate::grounding::GroundingReport;
use crate::story::Pillar;
use crate::{forge_tag, DirectiveDoc, ForgeResult, Story};

/// The single I/O seam between said-forge and the `.said` brain.
pub trait BrainIo {
    /// Write a frame with the given title/body/pillar/tags. Returns the
    /// brain-assigned doc id (string form).
    fn remember(
        &mut self,
        title: &str,
        body: &str,
        pillar: Pillar,
        tags: Vec<String>,
    ) -> ForgeResult<String>;

    /// Retrieve the body of a frame by its tag (exact match). Latest
    /// non-tombstoned frame for that tag, or None.
    ///
    /// Takes `&mut self` because the `sca-core` backing store caches
    /// decompressed bodies on read.
    fn find_body_by_tag(&mut self, tag: &str) -> Option<String>;

    /// Iterate every live frame's tags. Used by `latest_run_n` + directive
    /// discovery. Returns (doc_id, tags, created_at_utc).
    fn iter_tags(&self) -> Vec<(String, Vec<String>, i64)>;

    /// Mark a frame deleted (tombstone). Returns whether anything was
    /// deleted.
    fn delete(&mut self, doc_id: &str) -> bool;

    /// Persist to disk.
    fn save(&mut self) -> ForgeResult<()>;
}

/// Write `forge:directive:<hash>` frame with the raw bytes.
pub fn write_directive<B: BrainIo>(brain: &mut B, doc: &DirectiveDoc) -> ForgeResult<String> {
    let hash = crate::forge_hash(&doc.source);
    let mut tag = forge_tag("directive", &hash, "", None, None);
    // Empty slug leaves a trailing ':'. Strip it.
    while tag.ends_with(':') {
        tag.pop();
    }
    let body = serde_json::to_string_pretty(doc)?;
    let title = format!("forge directive: {}", doc.source);
    brain.remember(&title, &body, Pillar::External, vec![tag])?;
    Ok(hash)
}

/// Write `forge:story:<hash>:<slug>` frames.
pub fn write_stories<B: BrainIo>(brain: &mut B, stories: &[Story]) -> ForgeResult<Vec<String>> {
    let mut tags = Vec::with_capacity(stories.len());
    for s in stories {
        let tag = forge_tag("story", &s.directive_hash, &s.slug, None, None);
        let body = serde_json::to_string_pretty(s)?;
        let title = format!("story: {}", s.slug);
        brain.remember(&title, &body, Pillar::External, vec![tag.clone()])?;
        tags.push(tag);
    }
    Ok(tags)
}

/// Write `forge:request:<hash>:<slug>:rN`.
pub fn write_request<B: BrainIo>(
    brain: &mut B,
    story: &Story,
    run_n: u32,
    operator: &str,
    pillar_snapshot: &[Pillar],
) -> ForgeResult<String> {
    let tag = forge_tag("request", &story.directive_hash, &story.slug, Some(run_n), None);
    let body = serde_json::json!({
        "directive_hash": story.directive_hash,
        "slug": story.slug,
        "run_n": run_n,
        "operator": operator,
        "timestamp_utc": now_utc(),
        "pillar_snapshot": pillar_snapshot.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
    });
    brain.remember(
        &format!("request r{} for {}", run_n, story.slug),
        &body.to_string(),
        Pillar::Memory,
        vec![tag.clone()],
    )?;
    Ok(tag)
}

/// Write `forge:run:*:input / prompt / output / meta` ledger.
pub fn write_run_ledger<B: BrainIo>(
    brain: &mut B,
    story: &Story,
    run_n: u32,
    grounding: &GroundingReport,
    result: &GenerationResult,
) -> ForgeResult<Vec<String>> {
    let mut tags = Vec::with_capacity(4);

    let input_tag = forge_tag("run", &story.directive_hash, &story.slug, Some(run_n), Some("input"));
    let input_body = serde_json::to_string_pretty(grounding)?;
    brain.remember(
        &format!("run r{} input {}", run_n, story.slug),
        &input_body,
        Pillar::Memory,
        vec![input_tag.clone()],
    )?;
    tags.push(input_tag);

    let prompt_tag = forge_tag("run", &story.directive_hash, &story.slug, Some(run_n), Some("prompt"));
    brain.remember(
        &format!("run r{} prompt {}", run_n, story.slug),
        &result.prompt,
        Pillar::Memory,
        vec![prompt_tag.clone()],
    )?;
    tags.push(prompt_tag);

    let output_tag = forge_tag("run", &story.directive_hash, &story.slug, Some(run_n), Some("output"));
    brain.remember(
        &format!("run r{} output {}", run_n, story.slug),
        &result.raw_response,
        Pillar::Memory,
        vec![output_tag.clone()],
    )?;
    tags.push(output_tag);

    let meta_tag = forge_tag("run", &story.directive_hash, &story.slug, Some(run_n), Some("meta"));
    let meta_body = serde_json::json!({
        "provider": result.provider,
        "model": result.model,
        "input_tokens": result.input_tokens,
        "output_tokens": result.output_tokens,
        "cache_read_tokens": result.cache_read_tokens,
        "cache_write_tokens": result.cache_write_tokens,
        "duration_ms": result.duration_ms,
        "validation": result.validation,
        "status": "completed",
    });
    brain.remember(
        &format!("run r{} meta {}", run_n, story.slug),
        &meta_body.to_string(),
        Pillar::Memory,
        vec![meta_tag.clone()],
    )?;
    tags.push(meta_tag);

    Ok(tags)
}

/// Write spec / plan / tasks / brain frames.
pub fn write_artifacts<B: BrainIo>(
    brain: &mut B,
    story: &Story,
    artifacts: &crate::GeneratedArtifacts,
    rendered_brain_md: &str,
) -> ForgeResult<Vec<String>> {
    let mut tags = Vec::with_capacity(4);
    for (ty, body) in [
        ("spec", serde_json::to_string_pretty(&artifacts.spec)?),
        ("plan", serde_json::to_string_pretty(&artifacts.plan)?),
        ("tasks", serde_json::to_string_pretty(&artifacts.tasks)?),
        ("brain", rendered_brain_md.to_string()),
    ] {
        let tag = forge_tag(ty, &story.directive_hash, &story.slug, None, None);
        let title = format!("{} for {}", ty, story.slug);
        brain.remember(&title, &body, Pillar::External, vec![tag.clone()])?;
        tags.push(tag);
    }
    Ok(tags)
}

/// Find the highest run number for a story. Returns 0 if none.
pub fn latest_run_n<B: BrainIo>(brain: &B, directive_hash: &str, slug: &str) -> u32 {
    let prefix = format!("forge:run:{}:{}:r", directive_hash, slug);
    let mut max_n = 0u32;
    for (_, tags, _) in brain.iter_tags() {
        for tag in tags {
            if let Some(rest) = tag.strip_prefix(&prefix) {
                let n_str = rest.split(':').next().unwrap_or("");
                if let Ok(n) = n_str.parse::<u32>() {
                    if n > max_n {
                        max_n = n;
                    }
                }
            }
        }
    }
    max_n
}

/// Find the newest (by created_at) directive hash in the brain.
pub fn latest_directive_hash<B: BrainIo>(brain: &B) -> Option<String> {
    let mut latest: Option<(i64, String)> = None;
    for (_, tags, ts) in brain.iter_tags() {
        for tag in tags {
            if let Some(hash) = tag.strip_prefix("forge:directive:") {
                match &latest {
                    None => latest = Some((ts, hash.to_string())),
                    Some((cur, _)) if ts > *cur => latest = Some((ts, hash.to_string())),
                    _ => {}
                }
            }
        }
    }
    latest.map(|(_, h)| h)
}

/// Tombstone every forge frame for (directive_hash, slug). Returns count.
pub fn tombstone_story<B: BrainIo>(
    brain: &mut B,
    directive_hash: &str,
    slug: &str,
) -> ForgeResult<u32> {
    let prefixes = [
        format!("forge:story:{}:{}", directive_hash, slug),
        format!("forge:request:{}:{}", directive_hash, slug),
        format!("forge:run:{}:{}", directive_hash, slug),
        format!("forge:spec:{}:{}", directive_hash, slug),
        format!("forge:plan:{}:{}", directive_hash, slug),
        format!("forge:tasks:{}:{}", directive_hash, slug),
        format!("forge:brain:{}:{}", directive_hash, slug),
    ];
    let mut to_delete = Vec::new();
    for (doc_id, tags, _) in brain.iter_tags() {
        for tag in tags {
            if prefixes
                .iter()
                .any(|p| tag == *p || tag.starts_with(&format!("{}:", p)))
            {
                to_delete.push(doc_id.clone());
                break;
            }
        }
    }
    let n = to_delete.len() as u32;
    for doc_id in to_delete {
        brain.delete(&doc_id);
    }
    Ok(n)
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
    use crate::{DirectiveMeta, Story, StoryKind};

    /// In-memory BrainIo stub for unit tests.
    #[derive(Default)]
    pub struct MemBrain {
        pub frames: Vec<(String, String, String, Pillar, Vec<String>, i64, bool)>,
        next_id: u64,
    }

    impl BrainIo for MemBrain {
        fn remember(
            &mut self,
            title: &str,
            body: &str,
            pillar: Pillar,
            tags: Vec<String>,
        ) -> ForgeResult<String> {
            self.next_id += 1;
            let id = format!("doc_{}", self.next_id);
            self.frames
                .push((id.clone(), title.into(), body.into(), pillar, tags, now_utc(), false));
            Ok(id)
        }
        fn find_body_by_tag(&mut self, tag: &str) -> Option<String> {
            for (_, _, body, _, tags, _, deleted) in &self.frames {
                if !*deleted && tags.iter().any(|t| t == tag) {
                    return Some(body.clone());
                }
            }
            None
        }
        fn iter_tags(&self) -> Vec<(String, Vec<String>, i64)> {
            self.frames
                .iter()
                .filter(|(_, _, _, _, _, _, deleted)| !*deleted)
                .map(|(id, _, _, _, tags, ts, _)| (id.clone(), tags.clone(), *ts))
                .collect()
        }
        fn delete(&mut self, doc_id: &str) -> bool {
            let mut any = false;
            for f in &mut self.frames {
                if f.0 == doc_id && !f.6 {
                    f.6 = true;
                    any = true;
                }
            }
            any
        }
        fn save(&mut self) -> ForgeResult<()> {
            Ok(())
        }
    }

    fn dummy_story() -> Story {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("method".into(), serde_json::json!("POST"));
        Story {
            slug: "post-pet".into(),
            title: "Add pet".into(),
            raw_text: "POST /pet".into(),
            kind: StoryKind::ApiEndpoint,
            fields,
            directive_hash: "a3f91".into(),
            source_adapter: "openapi".into(),
            source_anchor: "paths./pet.post".into(),
        }
    }

    fn dummy_result() -> GenerationResult {
        GenerationResult {
            artifacts: GeneratedArtifacts {
                spec: SpecDoc {
                    overview: "ok.".into(),
                    actors: Vec::new(),
                    acceptance_criteria: vec!["ok.".into()],
                },
                plan: PlanDoc {
                    steps: vec![PlanStep {
                        id: "S1".into(),
                        action: "do it.".into(),
                        grounding_frame_ids: Vec::new(),
                    }],
                },
                tasks: vec![TaskItem {
                    id: "T1".into(),
                    text: "test it.".into(),
                    grounding_frame_ids: Vec::new(),
                }],
                brain_refs: vec![BrainRef {
                    frame_id: "f1".into(),
                    why_relevant: "because".into(),
                }],
            },
            validation: ValidationReport::default(),
            prompt: "p".into(),
            raw_response: "r".into(),
            provider: "stub".into(),
            model: "stub-m".into(),
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            duration_ms: 1,
        }
    }

    #[test]
    fn write_directive_returns_hash_and_stores_frame() {
        let mut b = MemBrain::default();
        let doc = DirectiveDoc {
            source: "fixtures/petstore.yaml".into(),
            adapter: "openapi".into(),
            raw: b"openapi:\n".to_vec(),
            meta: DirectiveMeta {
                loaded_at_utc: 0,
                operator: "t".into(),
                content_type: None,
            },
        };
        let hash = write_directive(&mut b, &doc).unwrap();
        assert_eq!(hash.len(), 5);
        let expected_tag = format!("forge:directive:{}", hash);
        assert!(b.find_body_by_tag(&expected_tag).is_some());
    }

    #[test]
    fn write_stories_writes_one_per_story() {
        let mut b = MemBrain::default();
        let stories = vec![dummy_story()];
        let tags = write_stories(&mut b, &stories).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0], "forge:story:a3f91:post-pet");
        assert!(b.find_body_by_tag(&tags[0]).is_some());
    }

    #[test]
    fn write_request_increments_run_number() {
        let mut b = MemBrain::default();
        let s = dummy_story();
        write_request(&mut b, &s, 1, "op", &[Pillar::External]).unwrap();
        write_request(&mut b, &s, 2, "op", &[Pillar::External]).unwrap();
        assert!(b.find_body_by_tag("forge:request:a3f91:post-pet:r1").is_some());
        assert!(b.find_body_by_tag("forge:request:a3f91:post-pet:r2").is_some());
    }

    #[test]
    fn latest_run_n_reads_run_ledger_tags_not_request_tags() {
        let mut b = MemBrain::default();
        let s = dummy_story();
        let r = dummy_result();
        let report = GroundingReport {
            queries: Vec::new(),
            selected_frame_ids: Vec::new(),
            inlined_frame_ids: Vec::new(),
            pillar_coverage: Default::default(),
        };
        write_run_ledger(&mut b, &s, 1, &report, &r).unwrap();
        write_run_ledger(&mut b, &s, 2, &report, &r).unwrap();
        assert_eq!(latest_run_n(&b, "a3f91", "post-pet"), 2);
    }

    #[test]
    fn write_run_ledger_writes_four_parts() {
        let mut b = MemBrain::default();
        let s = dummy_story();
        let r = dummy_result();
        let report = GroundingReport {
            queries: Vec::new(),
            selected_frame_ids: Vec::new(),
            inlined_frame_ids: Vec::new(),
            pillar_coverage: Default::default(),
        };
        let tags = write_run_ledger(&mut b, &s, 1, &report, &r).unwrap();
        assert_eq!(tags.len(), 4);
        for part in ["input", "prompt", "output", "meta"] {
            let tag = format!("forge:run:a3f91:post-pet:r1:{}", part);
            assert!(b.find_body_by_tag(&tag).is_some(), "{} missing", part);
        }
    }

    #[test]
    fn write_artifacts_writes_four_types() {
        let mut b = MemBrain::default();
        let s = dummy_story();
        let r = dummy_result();
        let tags = write_artifacts(&mut b, &s, &r.artifacts, "brain md body").unwrap();
        assert_eq!(tags.len(), 4);
        for ty in ["spec", "plan", "tasks", "brain"] {
            let tag = format!("forge:{}:a3f91:post-pet", ty);
            assert!(b.find_body_by_tag(&tag).is_some(), "{} missing", ty);
        }
    }

    #[test]
    fn latest_directive_hash_picks_newest() {
        let mut b = MemBrain::default();
        let doc = |s: &str| DirectiveDoc {
            source: s.into(),
            adapter: "openapi".into(),
            raw: b"openapi:\n".to_vec(),
            meta: DirectiveMeta {
                loaded_at_utc: 0,
                operator: "t".into(),
                content_type: None,
            },
        };
        write_directive(&mut b, &doc("first.yaml")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write_directive(&mut b, &doc("second.yaml")).unwrap();
        let hash = latest_directive_hash(&b).unwrap();
        assert_eq!(hash, crate::forge_hash("second.yaml"));
    }

    #[test]
    fn tombstone_story_marks_all_related_frames_deleted() {
        let mut b = MemBrain::default();
        let s = dummy_story();
        let r = dummy_result();
        write_stories(&mut b, &[s.clone()]).unwrap();
        write_request(&mut b, &s, 1, "op", &[]).unwrap();
        let report = GroundingReport {
            queries: Vec::new(),
            selected_frame_ids: Vec::new(),
            inlined_frame_ids: Vec::new(),
            pillar_coverage: Default::default(),
        };
        write_run_ledger(&mut b, &s, 1, &report, &r).unwrap();
        write_artifacts(&mut b, &s, &r.artifacts, "brain md body").unwrap();

        // 1 story + 1 request + 4 run ledger + 4 artifacts = 10 frames
        let before = b.iter_tags().len();
        assert_eq!(before, 10);

        let n = tombstone_story(&mut b, "a3f91", "post-pet").unwrap();
        assert_eq!(n, 10);

        let after = b.iter_tags().len();
        assert_eq!(after, 0, "all related frames tombstoned");
    }
}
