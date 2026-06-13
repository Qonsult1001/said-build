//! Grounding retrieval with pillar scoping.
//!
//! Per spec §8. For each story, build per-kind queries, execute them against
//! the .said brain with pillar scoping, dedupe + cap, apply pillar-aware
//! boosts, and emit an audit frame.
//!
//! The concrete `SaidFile` adapter lives in `said_file_brain.rs` and is
//! wired up in Phase 8 once sca-core's public API stabilizes.

pub mod strategy;

use crate::story::Pillar as ForgePillar;
use crate::{ForgeResult, Story, StoryKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// One retrieved frame with provenance + score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingHit {
    pub frame_id: String,
    pub tag: String,
    pub score: f32,
    pub query_origin: String,
    pub snippet: String,
    pub pillar: ForgePillar,
    /// Authority level parsed from the frame's `authority:<level>:<scope>`
    /// tag (see Phase 14 sync). `None` for frames written outside the forge
    /// sync pipeline.
    #[serde(default)]
    pub authority: Option<String>,
}

/// A single retrieval query within a story's retrieval plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalQuery {
    pub query: String,
    pub origin: String,
    pub pillar_scope: Vec<ForgePillar>,
    /// Engine to use: "sca" (vector + lexical mixed) or "sym" (exact symbol lookup).
    pub engine: String,
    /// Optional — if set, only frames whose tags carry an authority in this
    /// list are kept. Example: `vec!["law", "requested", "agreed"]` keeps
    /// ground-truth and client-stated content but drops wishlist-only frames.
    #[serde(default)]
    pub authority_filter: Option<Vec<String>>,
}

/// Brain-access trait. Tests swap in a stub; production uses
/// `said_file_brain::SaidFileBrain` (Phase 8).
pub trait BrainAccess {
    /// Vector+lexical search scoped to the given pillars. Returns (frame_id, score).
    fn search_by_pillar(
        &mut self,
        query: &str,
        top_k: usize,
        pillars: &HashSet<ForgePillar>,
    ) -> Vec<(String, f32)>;

    /// Like `search_by_pillar` but additionally filters results whose frame
    /// metadata carries an `authority:<level>:...` tag matching one of the
    /// supplied levels. Default impl post-filters after over-fetching; adapters
    /// can override for index-native filtering.
    fn search_by_pillar_with_authority(
        &mut self,
        query: &str,
        top_k: usize,
        pillars: &HashSet<ForgePillar>,
        authority_filter: Option<&[String]>,
    ) -> Vec<(String, f32)> {
        match authority_filter {
            None => self.search_by_pillar(query, top_k, pillars),
            Some(filter) if filter.is_empty() => self.search_by_pillar(query, top_k, pillars),
            Some(filter) => {
                // Over-fetch then post-filter. 3× factor because authority is
                // typically sparse on small corpora.
                let raw = self.search_by_pillar(query, top_k * 3, pillars);
                raw.into_iter()
                    .filter(|(id, _)| {
                        self.meta(id)
                            .and_then(|m| authority_from_tag(&m.tag).map(str::to_string))
                            .map(|a| filter.iter().any(|f| f == &a))
                            .unwrap_or(false)
                    })
                    .take(top_k)
                    .collect()
            }
        }
    }

    /// Exact symbol lookup. Returns (frame_id, pillar).
    fn sym(&mut self, name: &str, top_k: usize) -> Vec<(String, ForgePillar)>;

    /// Fetch frame metadata (tag + pillar + snippet of content).
    fn meta(&self, frame_id: &str) -> Option<FrameMeta>;
}

#[derive(Debug, Clone)]
pub struct FrameMeta {
    pub frame_id: String,
    pub tag: String,
    pub pillar: ForgePillar,
    pub snippet: String,
}

/// Parse the authority level from an `authority:<level>:<scope>` tag.
/// Returns `None` if the tag doesn't carry authority info.
///
/// Note: `tag` on `FrameMeta` is usually a single-tag string like
/// `xlsx:foo:5` — the real authority tag lives elsewhere in the frame's
/// tag list. This helper is lenient: it finds `authority:` anywhere in the
/// string and extracts the level token.
pub fn authority_from_tag(tag: &str) -> Option<&str> {
    let idx = tag.find("authority:")?;
    let rest = &tag[idx + "authority:".len()..];
    rest.split(|c: char| c == ':' || c.is_whitespace())
        .next()
        .filter(|s| !s.is_empty())
}

/// Output of a full retrieval run. Written as `forge:run:<hash>:<slug>:r<N>:input`
/// (per spec §8.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingReport {
    pub queries: Vec<QueryOutcome>,
    pub selected_frame_ids: Vec<String>,
    pub inlined_frame_ids: Vec<String>,
    pub pillar_coverage: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryOutcome {
    pub query: String,
    pub origin: String,
    pub pillar_scope: Vec<ForgePillar>,
    pub engine: String,
    pub hits: Vec<String>, // frame_ids
}

/// Top-level entry point.
pub fn retrieve<B: BrainAccess>(
    story: &Story,
    brain: &mut B,
    max_frames: usize,
    inline_top_n: usize,
) -> ForgeResult<(Vec<GroundingHit>, GroundingReport)> {
    let plan = strategy::queries_for(story);
    let mut outcomes = Vec::with_capacity(plan.len());
    let mut seen: std::collections::HashMap<String, GroundingHit> = std::collections::HashMap::new();
    for q in &plan {
        let pillar_set: HashSet<ForgePillar> = q.pillar_scope.iter().copied().collect();
        let raw: Vec<(String, f32)> = match q.engine.as_str() {
            "sym" => brain
                .sym(&q.query, 8)
                .into_iter()
                .map(|(id, _)| (id, 1.0_f32))
                .collect(),
            _ => brain.search_by_pillar_with_authority(
                &q.query,
                12,
                &pillar_set,
                q.authority_filter.as_deref(),
            ),
        };
        let mut hit_ids = Vec::new();
        for (frame_id, base_score) in raw {
            hit_ids.push(frame_id.clone());
            let Some(meta) = brain.meta(&frame_id) else { continue };
            let authority = authority_from_tag(&meta.tag).map(str::to_string);
            let pillar_boosted = apply_boost(story.kind, meta.pillar, base_score);
            let boosted = apply_authority_boost(
                story.kind,
                authority.as_deref(),
                pillar_boosted,
            );
            let hit = GroundingHit {
                frame_id: frame_id.clone(),
                tag: meta.tag.clone(),
                score: boosted,
                query_origin: q.origin.clone(),
                snippet: truncate_snippet(&meta.snippet, 200),
                pillar: meta.pillar,
                authority,
            };
            // Keep the highest-scoring version of a frame that appears in
            // multiple queries (dedupe-by-id).
            match seen.get(&frame_id) {
                Some(prev) if prev.score >= boosted => {}
                _ => {
                    seen.insert(frame_id, hit);
                }
            }
        }
        outcomes.push(QueryOutcome {
            query: q.query.clone(),
            origin: q.origin.clone(),
            pillar_scope: q.pillar_scope.clone(),
            engine: q.engine.clone(),
            hits: hit_ids,
        });
    }
    let mut hits: Vec<GroundingHit> = seen.into_values().collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(max_frames);
    let inlined_ids: Vec<String> = hits
        .iter()
        .take(inline_top_n)
        .map(|h| h.frame_id.clone())
        .collect();
    let mut pillar_coverage: std::collections::BTreeMap<String, u32> = Default::default();
    for p in ForgePillar::ALL {
        pillar_coverage.insert(p.to_string(), 0);
    }
    for h in &hits {
        *pillar_coverage.entry(h.pillar.to_string()).or_insert(0) += 1;
    }
    let report = GroundingReport {
        queries: outcomes,
        selected_frame_ids: hits.iter().map(|h| h.frame_id.clone()).collect(),
        inlined_frame_ids: inlined_ids,
        pillar_coverage,
    };
    Ok((hits, report))
}

/// Apply pillar-aware score boosts per spec §8.1.1.
pub fn apply_boost(kind: StoryKind, hit_pillar: ForgePillar, base: f32) -> f32 {
    let mult = match (kind, hit_pillar) {
        (StoryKind::ApiEndpoint, ForgePillar::Code) => 1.3,
        (StoryKind::ApiEndpoint, ForgePillar::External) => 1.2,
        (StoryKind::Requirement, ForgePillar::External) => 1.2,
        (StoryKind::Requirement, ForgePillar::Procedural) => 1.1,
        (StoryKind::TableRow | StoryKind::Ticket, ForgePillar::Code) => 1.2,
        _ => 1.0,
    };
    base * mult
}

/// Authority-aware score boost — stacks on top of `apply_boost`. Reflects
/// the authority hierarchy the spec settled on: ground truth (SQL) beats
/// client-stated requirements, which beat agreed-scope Dev Planning, which
/// beats existing C# code, which beats raw OpenAPI wishlist.
pub fn apply_authority_boost(
    kind: StoryKind,
    authority_level: Option<&str>,
    base: f32,
) -> f32 {
    let Some(level) = authority_level else { return base };
    let mult = match (kind, level) {
        // API endpoint stories: law > requested > agreed > existing > wishlist.
        (StoryKind::ApiEndpoint, "law") => 1.4,
        (StoryKind::ApiEndpoint, "requested") => 1.3,
        (StoryKind::ApiEndpoint, "agreed") => 1.25,
        (StoryKind::ApiEndpoint, "existing") => 1.1,
        (StoryKind::ApiEndpoint, "wishlist") => 1.05,
        // Requirement stories prefer law + requested heavily; agreed is softer.
        (StoryKind::Requirement, "law") => 1.3,
        (StoryKind::Requirement, "requested") => 1.25,
        (StoryKind::Requirement, "agreed") => 1.2,
        // Other story kinds: a mild law nudge is enough.
        (_, "law") => 1.2,
        _ => 1.0,
    };
    base * mult
}

fn truncate_snippet(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out = s.chars().take(max).collect::<String>();
        out.push('…');
        out
    }
}

/// Public wrapper — same as `truncate_snippet` for callers outside this module.
pub fn truncate_snippet_pub(s: &str, max: usize) -> String {
    truncate_snippet(s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_endpoint_boost_for_code_is_1_3x() {
        let boosted = apply_boost(StoryKind::ApiEndpoint, ForgePillar::Code, 1.0);
        assert!((boosted - 1.3).abs() < 0.0001);
    }

    #[test]
    fn api_endpoint_boost_for_external_is_1_2x() {
        let boosted = apply_boost(StoryKind::ApiEndpoint, ForgePillar::External, 1.0);
        assert!((boosted - 1.2).abs() < 0.0001);
    }

    #[test]
    fn requirement_boost_for_external_and_procedural() {
        assert!((apply_boost(StoryKind::Requirement, ForgePillar::External, 1.0) - 1.2).abs() < 1e-4);
        assert!((apply_boost(StoryKind::Requirement, ForgePillar::Procedural, 1.0) - 1.1).abs() < 1e-4);
    }

    #[test]
    fn generic_kind_has_no_boost() {
        for p in ForgePillar::ALL {
            assert!((apply_boost(StoryKind::Generic, *p, 1.0) - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn authority_boost_api_endpoint_law_is_1_4x() {
        let b = apply_authority_boost(StoryKind::ApiEndpoint, Some("law"), 1.0);
        assert!((b - 1.4).abs() < 1e-4);
    }

    #[test]
    fn authority_boost_api_endpoint_requested_is_1_3x() {
        let b = apply_authority_boost(StoryKind::ApiEndpoint, Some("requested"), 1.0);
        assert!((b - 1.3).abs() < 1e-4);
    }

    #[test]
    fn authority_boost_wishlist_under_law() {
        let wish = apply_authority_boost(StoryKind::ApiEndpoint, Some("wishlist"), 1.0);
        let law = apply_authority_boost(StoryKind::ApiEndpoint, Some("law"), 1.0);
        assert!(law > wish);
    }

    #[test]
    fn authority_boost_with_none_is_passthrough() {
        let b = apply_authority_boost(StoryKind::ApiEndpoint, None, 2.0);
        assert!((b - 2.0).abs() < 1e-4);
    }

    #[test]
    fn authority_from_tag_parses_level() {
        assert_eq!(authority_from_tag("authority:law:ground-truth"), Some("law"));
        assert_eq!(
            authority_from_tag("xlsx:reqs:5 authority:requested:requirements other"),
            Some("requested")
        );
        assert_eq!(authority_from_tag("code:pet"), None);
        assert_eq!(authority_from_tag(""), None);
    }

    #[test]
    fn truncate_snippet_appends_ellipsis() {
        let s = "x".repeat(250);
        let t = truncate_snippet(&s, 200);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), 201); // 200 + ellipsis
    }

    #[test]
    fn truncate_snippet_passthrough_when_under_cap() {
        assert_eq!(truncate_snippet("short", 200), "short");
    }

    // ---------------- integration with stub brain ----------------

    #[derive(Default)]
    struct StubBrain {
        frames: std::collections::HashMap<String, FrameMeta>,
        canned: Vec<(String, Vec<(String, f32)>)>,
    }

    impl StubBrain {
        fn add_frame(&mut self, frame_id: &str, tag: &str, pillar: ForgePillar, snippet: &str) {
            self.frames.insert(
                frame_id.to_string(),
                FrameMeta {
                    frame_id: frame_id.to_string(),
                    tag: tag.to_string(),
                    pillar,
                    snippet: snippet.to_string(),
                },
            );
        }
        fn canned(&mut self, query: &str, hits: Vec<(String, f32)>) {
            self.canned.push((query.to_string(), hits));
        }
    }

    impl BrainAccess for StubBrain {
        fn search_by_pillar(
            &mut self,
            query: &str,
            _top_k: usize,
            _pillars: &HashSet<ForgePillar>,
        ) -> Vec<(String, f32)> {
            for (q, hits) in &self.canned {
                if q == query {
                    return hits.clone();
                }
            }
            Vec::new()
        }
        fn sym(&mut self, name: &str, _top_k: usize) -> Vec<(String, ForgePillar)> {
            self.canned
                .iter()
                .find(|(q, _)| q == name)
                .map(|(_, hits)| {
                    hits.iter()
                        .filter_map(|(id, _)| self.frames.get(id).map(|m| (id.clone(), m.pillar)))
                        .collect()
                })
                .unwrap_or_default()
        }
        fn meta(&self, frame_id: &str) -> Option<FrameMeta> {
            self.frames.get(frame_id).cloned()
        }
    }

    fn fixture_story() -> Story {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("method".into(), serde_json::json!("POST"));
        fields.insert("path".into(), serde_json::json!("/pet"));
        fields.insert("operation_id".into(), serde_json::json!("addPet"));
        fields.insert("tags".into(), serde_json::json!(["pet"]));
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

    #[test]
    fn retrieve_dedupes_frames_across_queries() {
        let mut brain = StubBrain::default();
        brain.add_frame("f1", "code:pet", ForgePillar::Code, "fn add_pet() { ... }");
        brain.add_frame("f2", "format:openapi", ForgePillar::External, "POST /pet spec");
        brain.canned("POST /pet", vec![("f1".into(), 0.5), ("f2".into(), 0.4)]);
        brain.canned("pet", vec![("f1".into(), 0.3)]);
        brain.canned("addPet", vec![("f1".into(), 1.0)]);

        let story = fixture_story();
        let (hits, report) = retrieve(&story, &mut brain, 24, 8).unwrap();

        let ids: Vec<_> = hits.iter().map(|h| h.frame_id.clone()).collect();
        assert!(ids.iter().all(|id| id == "f1" || id == "f2"));
        assert_eq!(ids.iter().filter(|id| *id == "f1").count(), 1, "f1 deduped");
        assert_eq!(ids.iter().filter(|id| *id == "f2").count(), 1, "f2 deduped");
        // `f1` appears at 1.0 (sym match via addPet); boosted 1.3× because
        // story.kind is ApiEndpoint and f1 is in Code pillar.
        let f1 = hits.iter().find(|h| h.frame_id == "f1").unwrap();
        assert!((f1.score - 1.3).abs() < 1e-4, "sym hit 1.0 × 1.3 boost, got {}", f1.score);
        // `f2` is External pillar × 1.2 = 0.48
        let f2 = hits.iter().find(|h| h.frame_id == "f2").unwrap();
        assert!((f2.score - 0.48).abs() < 1e-4, "External hit 0.4 × 1.2 = 0.48, got {}", f2.score);

        assert_eq!(report.selected_frame_ids.len(), 2);
        assert_eq!(report.inlined_frame_ids.len(), 2);
        assert_eq!(report.pillar_coverage.get("Code").copied(), Some(1));
        assert_eq!(report.pillar_coverage.get("External").copied(), Some(1));
    }

    #[test]
    fn retrieve_caps_to_max_frames() {
        let mut brain = StubBrain::default();
        for i in 0..50 {
            let id = format!("f{:02}", i);
            brain.add_frame(&id, "code:x", ForgePillar::Code, "x");
        }
        let hits: Vec<(String, f32)> = (0..50)
            .map(|i| (format!("f{:02}", i), 1.0 - (i as f32 * 0.01)))
            .collect();
        brain.canned("POST /pet", hits);

        let story = fixture_story();
        let (hits, _) = retrieve(&story, &mut brain, 24, 8).unwrap();
        assert_eq!(hits.len(), 24);
        // Highest-scoring first
        assert_eq!(hits[0].frame_id, "f00");
    }

    #[test]
    fn retrieve_sorts_by_score_desc() {
        let mut brain = StubBrain::default();
        brain.add_frame("low", "code:x", ForgePillar::Code, "low");
        brain.add_frame("high", "code:x", ForgePillar::Code, "high");
        brain.canned("POST /pet", vec![("low".into(), 0.2), ("high".into(), 0.9)]);

        let story = fixture_story();
        let (hits, _) = retrieve(&story, &mut brain, 24, 8).unwrap();
        assert_eq!(hits[0].frame_id, "high");
        assert_eq!(hits[1].frame_id, "low");
    }

    #[test]
    fn retrieve_snippet_truncated_to_200() {
        let mut brain = StubBrain::default();
        let long = "a".repeat(500);
        brain.add_frame("big", "code:x", ForgePillar::Code, &long);
        brain.canned("POST /pet", vec![("big".into(), 0.5)]);
        let story = fixture_story();
        let (hits, _) = retrieve(&story, &mut brain, 24, 8).unwrap();
        assert!(hits[0].snippet.chars().count() <= 201);
        assert!(hits[0].snippet.ends_with('…'));
    }
}
