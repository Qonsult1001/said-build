//! Batch runner — orchestrates one story-loop iteration + circuit breaker + resume.
//!
//! Per spec §4.3 + §13.

use crate::adapter::{EditorAdapter, ProjectedStory};
use crate::frame::{self, BrainIo};
use crate::grounding::{self, BrainAccess};
use crate::llm::LlmProvider;
use crate::projection::{self, is_complete, write_folder, write_folder_with_grounding};
use crate::{ForgeConfig, ForgeError, Story};
#[cfg(test)]
use crate::ForgeResult;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct StoryOutcome {
    pub slug: String,
    pub status: StoryStatus,
    pub run_n: u32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryStatus {
    Completed,
    Skipped,
    FailedNetwork,
    FailedRateLimit,
    FailedAuth,
    FailedParse,
    FailedContext,
    FailedOther,
}

impl StoryStatus {
    pub fn class_name(&self) -> &'static str {
        match self {
            StoryStatus::Completed => "completed",
            StoryStatus::Skipped => "skipped",
            StoryStatus::FailedNetwork => "network",
            StoryStatus::FailedRateLimit => "rate_limit",
            StoryStatus::FailedAuth => "auth",
            StoryStatus::FailedParse => "parse",
            StoryStatus::FailedContext => "context",
            StoryStatus::FailedOther => "other",
        }
    }
    pub fn is_failure(&self) -> bool {
        !matches!(self, StoryStatus::Completed | StoryStatus::Skipped)
    }
}

/// Circuit breaker state.
pub struct CircuitBreaker {
    threshold: u32,
    recent: Vec<&'static str>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32) -> Self {
        Self { threshold, recent: Vec::new() }
    }

    /// Record an outcome. Returns true if the breaker has tripped.
    pub fn record(&mut self, s: &StoryStatus) -> bool {
        if matches!(s, StoryStatus::Completed | StoryStatus::Skipped) {
            self.recent.clear();
            return false;
        }
        self.recent.push(s.class_name());
        if self.recent.len() > self.threshold as usize {
            let drop_count = self.recent.len() - self.threshold as usize;
            self.recent.drain(..drop_count);
        }
        self.recent.len() as u32 == self.threshold
            && self.recent.windows(2).all(|w| w[0] == w[1])
    }

    pub fn last_class(&self) -> Option<&str> {
        self.recent.last().copied()
    }
}

/// Preflight cost estimate: story count × average input/output tokens × rates.
pub fn preflight_estimate(
    cfg: &ForgeConfig,
    story_count: u32,
    avg_input_tokens: u32,
    avg_output_tokens: u32,
) -> f64 {
    let input = story_count as f64 * avg_input_tokens as f64;
    let output = story_count as f64 * avg_output_tokens as f64;
    (input / 1_000_000.0) * cfg.costs.input_usd_per_mtok
        + (output / 1_000_000.0) * cfg.costs.output_usd_per_mtok
}

/// Classify an error into a status.
pub fn classify_error(err: &ForgeError) -> StoryStatus {
    match err {
        ForgeError::Http { .. } => StoryStatus::FailedNetwork,
        ForgeError::Llm(msg) if msg.to_lowercase().contains("rate limit") => {
            StoryStatus::FailedRateLimit
        }
        ForgeError::Llm(msg) if msg.to_lowercase().contains("auth") => StoryStatus::FailedAuth,
        ForgeError::ContextExceeded { .. } => StoryStatus::FailedContext,
        ForgeError::Serde(_) | ForgeError::Validation(_) => StoryStatus::FailedParse,
        _ => StoryStatus::FailedOther,
    }
}

pub struct RunOptions<'a> {
    pub project_root: &'a Path,
    pub config: &'a ForgeConfig,
    pub force: bool,
    pub halt_after: u32,
    pub operator: String,
    pub inline_top_n: usize,
    pub max_frames: usize,
}

/// Run one story start-to-finish.
///
/// The brain must implement **both** `BrainIo` (for frame writes + tombstone +
/// directive lookup) and `BrainAccess` (for grounding retrieval). Production
/// `SaidFileBrain` will implement both over a single `&mut SaidFile`; tests
/// use a combined `MemBrain`.
pub async fn run_one<B>(
    brain: &mut B,
    story: &Story,
    llm: &dyn LlmProvider,
    adapter: &dyn EditorAdapter,
    opts: &RunOptions<'_>,
) -> StoryOutcome
where
    B: BrainIo + BrainAccess,
{
    let slug = story.slug.clone();
    if !opts.force && is_complete(opts.project_root, &slug) {
        return StoryOutcome {
            slug,
            status: StoryStatus::Skipped,
            run_n: 0,
            message: "already complete".into(),
        };
    }

    let run_n = frame::latest_run_n(brain, &story.directive_hash, &slug) + 1;

    // 1. Request frame
    if let Err(e) = frame::write_request(brain, story, run_n, &opts.operator, &[]) {
        return StoryOutcome { slug, status: classify_error(&e), run_n, message: e.to_string() };
    }

    // 2. Retrieval
    let (hits, report) = match grounding::retrieve(story, brain, opts.max_frames, opts.inline_top_n) {
        Ok(r) => r,
        Err(e) => return StoryOutcome { slug, status: classify_error(&e), run_n, message: e.to_string() },
    };

    // 3. Generation
    let result = match crate::generator::generate(story, &hits, &report, llm, opts.inline_top_n).await {
        Ok(r) => r,
        Err(e) => return StoryOutcome { slug, status: classify_error(&e), run_n, message: e.to_string() },
    };

    // 4. Audit ledger
    if let Err(e) = frame::write_run_ledger(brain, story, run_n, &report, &result) {
        return StoryOutcome { slug, status: classify_error(&e), run_n, message: e.to_string() };
    }

    // 5. Build technical grounding (Phase 19) — if the workspace has a
    //    1-ground-truth/ folder with SQL, parse it into TableSchema +
    //    SqlCatalog and produce a per-op grounding block. Best-effort:
    //    silently skips when no SQL is present or parsing fails.
    let tech_md: Option<String> = build_tech_grounding_for_story(opts.project_root, story);

    // 6. Artifact frames (spec/plan/tasks/brain) — embed the grounding
    //    block in brain.md so the forge_get MCP tool can return it inline.
    let brain_md = projection::render::brain_md_with_tech(
        story,
        &result.artifacts.brain_refs,
        &hits,
        tech_md.as_deref(),
    );
    if let Err(e) = frame::write_artifacts(brain, story, &result.artifacts, &brain_md) {
        return StoryOutcome { slug, status: classify_error(&e), run_n, message: e.to_string() };
    }

    // 7. Project to .forge/<slug>/
    if let Err(e) = write_folder_with_grounding(
        opts.project_root,
        story,
        &result,
        &hits,
        &format!("r{}", run_n),
        tech_md.as_deref(),
    ) {
        return StoryOutcome { slug, status: classify_error(&e), run_n, message: e.to_string() };
    }

    // 7. Deploy editor skill
    let proj = ProjectedStory { story, result: &result };
    if let Err(e) = adapter.write_skill(opts.project_root, &proj) {
        return StoryOutcome { slug, status: classify_error(&e), run_n, message: e.to_string() };
    }

    StoryOutcome {
        slug,
        status: StoryStatus::Completed,
        run_n,
        message: format!(
            "{} tokens in / {} out, {}ms",
            result.input_tokens, result.output_tokens, result.duration_ms
        ),
    }
}

/// Build the per-op technical grounding from the workspace's SQL catalog.
/// Silent no-op when the 1-ground-truth/ folder is missing (e.g. stories
/// generated from a non-forge workspace). Returns None on any failure.
fn build_tech_grounding_for_story(project_root: &Path, story: &Story) -> Option<String> {
    use crate::directive::OpSpec;
    use crate::sql_catalog::build_catalog;
    use crate::tech_grounding::build_technical_grounding;

    let ground_truth = project_root.join("1-ground-truth");
    if !ground_truth.is_dir() {
        return None;
    }
    let catalog = build_catalog(project_root).ok()?;
    if catalog.tables.is_empty() && catalog.objects.is_empty() {
        return None;
    }

    // Convert the sca-story into an OpSpec so we can reuse the phase-18/19
    // grounding code. Story.fields already carries method/path from the
    // OpenAPI adapter; for markdown stories we fall back to title parsing.
    let method = story
        .fields
        .get("method")
        .and_then(|v| v.as_str())
        .map(String::from);
    let path = story
        .fields
        .get("path")
        .and_then(|v| v.as_str())
        .map(String::from);

    let op = OpSpec {
        slug: story.slug.clone(),
        label: story.title.clone(),
        method,
        path,
        summary: None,
        fields: Vec::new(), // runner-time field shape lives in story.raw_text; schema_diff adds no value here
        adapter: story.source_adapter.clone(),
        source: story.source_anchor.clone(),
    };
    let tg = build_technical_grounding(&op, &catalog);
    // Only emit something if we actually found candidate tables or procs;
    // otherwise the block is pure boilerplate.
    if tg.primary_tables.is_empty()
        && tg.related_tables.is_empty()
        && tg.procedures.is_empty()
        && tg.triggers.is_empty()
        && tg.views.is_empty()
    {
        return None;
    }
    Some(tg.to_markdown())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::BrainIo;
    use crate::grounding::{BrainAccess, FrameMeta};
    use crate::llm::stub::StubProvider;
    use crate::story::Pillar;
    use crate::{ClaudeAdapter, StoryKind};
    use std::collections::HashSet;
    use tempfile::TempDir;

    /// Combined stub implementing BOTH BrainIo + BrainAccess.
    #[derive(Default)]
    struct MemBrain {
        frames: Vec<FrameRecord>,
        next_id: u64,
    }

    struct FrameRecord {
        doc_id: String,
        title: String,
        body: String,
        pillar: Pillar,
        tags: Vec<String>,
        created_at: i64,
        deleted: bool,
    }

    impl MemBrain {
        fn primary_tag(meta_tags: &[String]) -> Option<&str> {
            meta_tags.first().map(|s| s.as_str())
        }
    }

    impl BrainIo for MemBrain {
        fn remember(&mut self, title: &str, body: &str, pillar: Pillar, tags: Vec<String>) -> ForgeResult<String> {
            self.next_id += 1;
            let id = format!("doc_{}", self.next_id);
            self.frames.push(FrameRecord {
                doc_id: id.clone(),
                title: title.into(),
                body: body.into(),
                pillar,
                tags,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                deleted: false,
            });
            Ok(id)
        }
        fn find_body_by_tag(&mut self, tag: &str) -> Option<String> {
            self.frames.iter().find(|f| !f.deleted && f.tags.iter().any(|t| t == tag))
                .map(|f| f.body.clone())
        }
        fn iter_tags(&self) -> Vec<(String, Vec<String>, i64)> {
            self.frames.iter().filter(|f| !f.deleted)
                .map(|f| (f.doc_id.clone(), f.tags.clone(), f.created_at))
                .collect()
        }
        fn delete(&mut self, doc_id: &str) -> bool {
            let mut any = false;
            for f in &mut self.frames {
                if f.doc_id == doc_id && !f.deleted {
                    f.deleted = true;
                    any = true;
                }
            }
            any
        }
        fn save(&mut self) -> ForgeResult<()> { Ok(()) }
    }

    impl BrainAccess for MemBrain {
        fn search_by_pillar(&mut self, _query: &str, _top_k: usize, _pillars: &HashSet<Pillar>) -> Vec<(String, f32)> {
            Vec::new() // stubbed — grounding retrieval returns nothing in runner tests
        }
        fn sym(&mut self, _name: &str, _top_k: usize) -> Vec<(String, Pillar)> { Vec::new() }
        fn meta(&self, frame_id: &str) -> Option<FrameMeta> {
            self.frames.iter().find(|f| !f.deleted && f.doc_id == frame_id).map(|f| FrameMeta {
                frame_id: f.doc_id.clone(),
                tag: Self::primary_tag(&f.tags).unwrap_or("").to_string(),
                pillar: f.pillar,
                snippet: f.body.chars().take(200).collect(),
            })
        }
    }

    fn stub_llm_ok() -> StubProvider {
        let p = StubProvider::new();
        // When the user prompt includes the slug, return a valid payload.
        p.canned_for("post-pet", serde_json::json!({
            "spec": { "overview": "ok.", "actors": [], "acceptance_criteria": ["ok."] },
            "plan": { "steps": [{ "id": "S1", "action": "do it.", "grounding_frame_ids": [] }] },
            "tasks": [{ "id": "T1", "text": "test it.", "grounding_frame_ids": [] }],
            "brain_refs": []
        }));
        p
    }

    fn story() -> Story {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("method".into(), serde_json::json!("POST"));
        fields.insert("path".into(), serde_json::json!("/pet"));
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

    fn cfg() -> ForgeConfig {
        ForgeConfig::default()
    }

    #[test]
    fn circuit_breaker_trips_on_five_same_class() {
        let mut cb = CircuitBreaker::new(5);
        for _ in 0..4 {
            assert!(!cb.record(&StoryStatus::FailedAuth));
        }
        assert!(cb.record(&StoryStatus::FailedAuth), "fifth auth failure trips breaker");
    }

    #[test]
    fn circuit_breaker_does_not_trip_on_mixed_classes() {
        let mut cb = CircuitBreaker::new(5);
        cb.record(&StoryStatus::FailedAuth);
        cb.record(&StoryStatus::FailedNetwork);
        cb.record(&StoryStatus::FailedAuth);
        cb.record(&StoryStatus::FailedParse);
        assert!(!cb.record(&StoryStatus::FailedAuth));
    }

    #[test]
    fn circuit_breaker_resets_on_completed() {
        let mut cb = CircuitBreaker::new(3);
        cb.record(&StoryStatus::FailedAuth);
        cb.record(&StoryStatus::FailedAuth);
        cb.record(&StoryStatus::Completed);
        assert!(!cb.record(&StoryStatus::FailedAuth));
    }

    #[test]
    fn preflight_estimate_computes_usd() {
        let c = cfg();
        // 100 stories × 3000 input + 1000 output each.
        // input: 100 * 3000 / 1_000_000 * 15.0 = 4.50
        // output: 100 * 1000 / 1_000_000 * 75.0 = 7.50
        let usd = preflight_estimate(&c, 100, 3000, 1000);
        assert!((usd - 12.0).abs() < 1e-6);
    }

    #[test]
    fn classify_error_maps_correctly() {
        assert_eq!(
            classify_error(&ForgeError::Http { url: "x".into(), message: "timeout".into() }),
            StoryStatus::FailedNetwork
        );
        assert_eq!(
            classify_error(&ForgeError::Llm("rate limit exceeded".into())),
            StoryStatus::FailedRateLimit
        );
        assert_eq!(
            classify_error(&ForgeError::Llm("auth failure".into())),
            StoryStatus::FailedAuth
        );
        assert_eq!(
            classify_error(&ForgeError::ContextExceeded { slug: "s".into(), estimated_tokens: 300_000 }),
            StoryStatus::FailedContext
        );
    }

    #[tokio::test]
    async fn run_one_completes_end_to_end() {
        let td = TempDir::new().unwrap();
        let mut brain = MemBrain::default();
        let llm = stub_llm_ok();
        let adapter = ClaudeAdapter;
        let c = cfg();
        let opts = RunOptions {
            project_root: td.path(),
            config: &c,
            force: false,
            halt_after: 5,
            operator: "test".into(),
            inline_top_n: 4,
            max_frames: 8,
        };
        let outcome = run_one(&mut brain, &story(), &llm, &adapter, &opts).await;
        assert_eq!(outcome.status, StoryStatus::Completed);
        assert!(td.path().join(".forge").join("post-pet").join("story.md").exists());
        assert!(td.path().join(".claude").join("skills").join("post-pet").join("SKILL.md").exists());
        // Audit ledger present
        let expected = [
            "forge:request:a3f91:post-pet:r1",
            "forge:run:a3f91:post-pet:r1:input",
            "forge:run:a3f91:post-pet:r1:prompt",
            "forge:run:a3f91:post-pet:r1:output",
            "forge:run:a3f91:post-pet:r1:meta",
            "forge:spec:a3f91:post-pet",
            "forge:plan:a3f91:post-pet",
            "forge:tasks:a3f91:post-pet",
            "forge:brain:a3f91:post-pet",
        ];
        for tag in expected {
            assert!(brain.find_body_by_tag(tag).is_some(), "missing: {}", tag);
        }
    }

    #[tokio::test]
    async fn run_one_skips_already_completed_when_not_forced() {
        let td = TempDir::new().unwrap();
        let mut brain = MemBrain::default();
        let llm = stub_llm_ok();
        let adapter = ClaudeAdapter;
        let c = cfg();
        let opts = RunOptions {
            project_root: td.path(),
            config: &c,
            force: false,
            halt_after: 5,
            operator: "test".into(),
            inline_top_n: 4,
            max_frames: 8,
        };
        let first = run_one(&mut brain, &story(), &llm, &adapter, &opts).await;
        assert_eq!(first.status, StoryStatus::Completed);
        let second = run_one(&mut brain, &story(), &llm, &adapter, &opts).await;
        assert_eq!(second.status, StoryStatus::Skipped);
    }

    #[tokio::test]
    async fn run_one_force_reruns_completed_story() {
        let td = TempDir::new().unwrap();
        let mut brain = MemBrain::default();
        let llm = stub_llm_ok();
        let adapter = ClaudeAdapter;
        let c = cfg();
        let mut opts = RunOptions {
            project_root: td.path(),
            config: &c,
            force: false,
            halt_after: 5,
            operator: "test".into(),
            inline_top_n: 4,
            max_frames: 8,
        };
        run_one(&mut brain, &story(), &llm, &adapter, &opts).await;
        opts.force = true;
        let second = run_one(&mut brain, &story(), &llm, &adapter, &opts).await;
        assert_eq!(second.status, StoryStatus::Completed);
        assert_eq!(second.run_n, 2);
    }
}
