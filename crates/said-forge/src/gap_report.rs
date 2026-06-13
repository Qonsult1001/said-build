//! Cross-authority gap report.
//!
//! For each directive operation (OpenAPI path, Dev Planning heading, XLSX
//! row, whatever `forge run` is iterating), check which authority levels
//! have evidence in the brain. Produce a Markdown report showing where
//! ground-truth SQL supports the request, where it's partial, and where
//! it's missing entirely.
//!
//! Written to `.forge/gaps.md` by `forge run` when the workspace config
//! `run.gap_report_phase` is `OneShot` (run after stories) or `GapFirst`
//! (run before stories; halt for approval).
//!
//! Dual-directive mode (`directive.mode = "both"`) additionally detects
//! structural conflicts between primary and secondary directives — an op
//! in one but not the other, or the same slug with a divergent path.

use serde::{Deserialize, Serialize};

use crate::grounding::{authority_from_tag, BrainAccess};
use crate::{ForgeResult, Story};

// ───────────────────────── data model ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapReport {
    pub project: String,
    /// UTC ISO-8601 timestamp.
    pub generated_at: String,
    pub primary_directive: String,
    pub secondary_directive: Option<String>,
    pub authority_order: Vec<String>,
    pub operations: Vec<OperationGap>,
    #[serde(default)]
    pub directive_conflicts: Vec<DirectiveConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationGap {
    pub slug: String,
    /// Human label — usually `METHOD /path` for API ops, title for others.
    pub label: String,
    pub status: GapStatus,
    pub evidence: Vec<Evidence>,
    /// Plain-English recommendation derived from which authorities lit up.
    pub recommendation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GapStatus {
    FullSupport,
    PartialSupport,
    NoSupport,
    /// A primary-secondary divergence exists; see `directive_conflicts`.
    Conflict,
}

impl GapStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::FullSupport => "✓ fully supported",
            Self::PartialSupport => "⚠ partial support",
            Self::NoSupport => "✗ no support",
            Self::Conflict => "⚡ conflict",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub authority: String,
    pub source: String,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DirectiveConflict {
    OnlyInPrimary { slug: String, label: String },
    OnlyInSecondary { slug: String, label: String },
    PathDivergence {
        slug: String,
        primary_path: String,
        secondary_path: String,
    },
}

pub struct GapSummary {
    pub total: usize,
    pub full: usize,
    pub partial: usize,
    pub none: usize,
    pub conflict: usize,
}

impl GapReport {
    pub fn summary(&self) -> GapSummary {
        let total = self.operations.len();
        let full = self
            .operations
            .iter()
            .filter(|o| o.status == GapStatus::FullSupport)
            .count();
        let partial = self
            .operations
            .iter()
            .filter(|o| o.status == GapStatus::PartialSupport)
            .count();
        let none = self
            .operations
            .iter()
            .filter(|o| o.status == GapStatus::NoSupport)
            .count();
        let conflict = self.directive_conflicts.len();
        GapSummary {
            total,
            full,
            partial,
            none,
            conflict,
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Gap Report — {}\n\n", self.project));
        out.push_str(&format!("Generated: {}\n", self.generated_at));
        out.push_str(&format!("Primary directive: {}\n", self.primary_directive));
        if let Some(sec) = &self.secondary_directive {
            out.push_str(&format!("Secondary directive: {}\n", sec));
        }
        if !self.authority_order.is_empty() {
            out.push_str(&format!(
                "Authority order on conflict: {}\n",
                self.authority_order.join(" > ")
            ));
        }

        let s = self.summary();
        out.push_str("\n## Summary\n\n");
        out.push_str(&format!("- {} operations total\n", s.total));
        out.push_str(&format!("- ✓ {} fully supported\n", s.full));
        out.push_str(&format!("- ⚠ {} partial support\n", s.partial));
        out.push_str(&format!("- ✗ {} no support\n", s.none));
        if s.conflict > 0 {
            out.push_str(&format!(
                "- ⚡ {} directive conflicts (primary vs secondary)\n",
                s.conflict
            ));
        }

        if !self.directive_conflicts.is_empty() {
            out.push_str("\n## Directive conflicts\n\n");
            for c in &self.directive_conflicts {
                match c {
                    DirectiveConflict::OnlyInPrimary { slug, label } => {
                        out.push_str(&format!(
                            "- **Only in primary** — `{}` ({}). Secondary has no matching operation; decide whether to drop from scope or mirror into secondary.\n",
                            slug, label
                        ));
                    }
                    DirectiveConflict::OnlyInSecondary { slug, label } => {
                        out.push_str(&format!(
                            "- **Only in secondary** — `{}` ({}). Primary has no matching operation; verify with client.\n",
                            slug, label
                        ));
                    }
                    DirectiveConflict::PathDivergence {
                        slug,
                        primary_path,
                        secondary_path,
                    } => {
                        out.push_str(&format!(
                            "- **Path divergence** — `{}` primary=`{}` secondary=`{}`. Reconcile before implementation.\n",
                            slug, primary_path, secondary_path
                        ));
                    }
                }
            }
        }

        out.push_str("\n## Per-operation details\n\n");
        for op in &self.operations {
            out.push_str(&format!("### `{}` — {}\n\n", op.slug, op.label));
            out.push_str(&format!("**Status:** {}\n\n", op.status.label()));
            if op.evidence.is_empty() {
                out.push_str("_(no grounding evidence found for this operation)_\n\n");
            } else {
                for e in &op.evidence {
                    out.push_str(&format!(
                        "- **{}** (score {:.2}) — `{}`\n  > {}\n",
                        e.authority,
                        e.score,
                        e.source,
                        one_line(&e.snippet)
                    ));
                }
                out.push('\n');
            }
            if !op.recommendation.is_empty() {
                out.push_str(&format!("**Recommendation:** {}\n\n", op.recommendation));
            }
        }
        out
    }
}

fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}

fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Minimal UTC rendering — good enough for audit; avoids adding chrono dep.
    format!("unix_ts={}", secs)
}

// ───────────────────────── analysis ─────────────────────────

/// Analyse a single story against the brain. Runs the standard grounding
/// retrieval plan and buckets the highest-scoring hit per authority level.
pub fn analyse_operation<B: BrainAccess>(
    brain: &mut B,
    story: &Story,
) -> ForgeResult<OperationGap> {
    use crate::grounding::{strategy, truncate_snippet_pub};
    use std::collections::HashSet;

    let plan = strategy::queries_for(story);
    let mut best: std::collections::BTreeMap<String, Evidence> = Default::default();

    for q in &plan {
        let pillars: HashSet<_> = q.pillar_scope.iter().copied().collect();
        let raw = match q.engine.as_str() {
            "sym" => brain
                .sym(&q.query, 5)
                .into_iter()
                .map(|(id, _)| (id, 1.0_f32))
                .collect::<Vec<_>>(),
            _ => brain.search_by_pillar_with_authority(
                &q.query,
                5,
                &pillars,
                q.authority_filter.as_deref(),
            ),
        };
        for (frame_id, score) in raw {
            let Some(meta) = brain.meta(&frame_id) else { continue };
            let authority = authority_from_tag(&meta.tag)
                .map(str::to_string)
                .unwrap_or_else(|| "unknown".into());
            let candidate = Evidence {
                authority: authority.clone(),
                source: meta.tag.clone(),
                snippet: truncate_snippet_pub(&meta.snippet, 200),
                score,
            };
            match best.get(&authority) {
                Some(existing) if existing.score >= score => {}
                _ => {
                    best.insert(authority, candidate);
                }
            }
        }
    }

    let has = |level: &str| best.contains_key(level);
    let status = if has("law") && (has("requested") || has("agreed")) {
        GapStatus::FullSupport
    } else if has("law") {
        GapStatus::PartialSupport
    } else if has("requested") || has("agreed") || has("wishlist") {
        GapStatus::NoSupport
    } else {
        GapStatus::NoSupport
    };

    let has_any = !best.is_empty();
    let recommendation = match status {
        GapStatus::FullSupport => String::new(),
        GapStatus::PartialSupport => {
            "ground-truth exists but no matching client requirement — verify the scope is still in".into()
        }
        GapStatus::NoSupport if has("requested") || has("wishlist") || has("agreed") => {
            "no matching ground-truth procedure; design new SQL or confirm the request is deferred".into()
        }
        GapStatus::NoSupport if has_any => {
            "some evidence present but no client-stated requirement; confirm this operation is still wanted".into()
        }
        GapStatus::NoSupport => {
            "no authority evidence at all — check the directive picked up this operation".into()
        }
        GapStatus::Conflict => String::new(),
    };

    Ok(OperationGap {
        slug: story.slug.clone(),
        label: operation_label(story),
        status,
        evidence: best.into_values().collect(),
        recommendation,
    })
}

fn operation_label(story: &Story) -> String {
    let method = story.fields.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let path = story.fields.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if !method.is_empty() && !path.is_empty() {
        format!("{} {}", method, path)
    } else if !story.title.is_empty() {
        story.title.clone()
    } else {
        story.slug.clone()
    }
}

// ───────────────────────── top-level build ─────────────────────────

pub struct BuildReportInput<'a> {
    pub project: &'a str,
    pub primary_directive: &'a str,
    pub secondary_directive: Option<&'a str>,
    pub authority_order: &'a [String],
    pub primary_stories: &'a [Story],
    pub secondary_stories: Option<&'a [Story]>,
}

pub fn build_report<B: BrainAccess>(
    input: BuildReportInput<'_>,
    brain: &mut B,
) -> ForgeResult<GapReport> {
    let mut ops = Vec::with_capacity(input.primary_stories.len());
    for s in input.primary_stories {
        ops.push(analyse_operation(brain, s)?);
    }

    let conflicts = match input.secondary_stories {
        Some(sec) => detect_dual_directive_conflicts(input.primary_stories, sec),
        None => Vec::new(),
    };

    // Flip ops with an entry in conflicts to GapStatus::Conflict for clarity.
    let conflict_slugs: std::collections::HashSet<&str> = conflicts
        .iter()
        .filter_map(|c| match c {
            DirectiveConflict::OnlyInPrimary { slug, .. }
            | DirectiveConflict::OnlyInSecondary { slug, .. }
            | DirectiveConflict::PathDivergence { slug, .. } => Some(slug.as_str()),
        })
        .collect();
    for op in &mut ops {
        if conflict_slugs.contains(op.slug.as_str()) {
            op.status = GapStatus::Conflict;
        }
    }

    Ok(GapReport {
        project: input.project.into(),
        generated_at: iso_now(),
        primary_directive: input.primary_directive.into(),
        secondary_directive: input.secondary_directive.map(String::from),
        authority_order: input.authority_order.to_vec(),
        operations: ops,
        directive_conflicts: conflicts,
    })
}

pub fn detect_dual_directive_conflicts(
    primary: &[Story],
    secondary: &[Story],
) -> Vec<DirectiveConflict> {
    let mut out = Vec::new();
    let p_map: std::collections::HashMap<&str, &Story> =
        primary.iter().map(|s| (s.slug.as_str(), s)).collect();
    let s_map: std::collections::HashMap<&str, &Story> =
        secondary.iter().map(|s| (s.slug.as_str(), s)).collect();

    for (slug, p) in &p_map {
        if !s_map.contains_key(*slug) {
            out.push(DirectiveConflict::OnlyInPrimary {
                slug: (*slug).into(),
                label: operation_label(p),
            });
        }
    }
    for (slug, s) in &s_map {
        if !p_map.contains_key(*slug) {
            out.push(DirectiveConflict::OnlyInSecondary {
                slug: (*slug).into(),
                label: operation_label(s),
            });
        }
    }
    for (slug, p) in &p_map {
        if let Some(s) = s_map.get(*slug) {
            let p_path = p.fields.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let s_path = s.fields.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if !p_path.is_empty() && !s_path.is_empty() && p_path != s_path {
                out.push(DirectiveConflict::PathDivergence {
                    slug: (*slug).into(),
                    primary_path: p_path.into(),
                    secondary_path: s_path.into(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::{BrainAccess, FrameMeta};
    use crate::story::Pillar as ForgePillar;
    use crate::{Story, StoryKind};

    struct StubBrain {
        frames: std::collections::HashMap<String, FrameMeta>,
        canned: Vec<(String, Vec<(String, f32)>)>,
    }

    impl StubBrain {
        fn new() -> Self {
            Self {
                frames: Default::default(),
                canned: Vec::new(),
            }
        }
        fn add_frame(&mut self, id: &str, tag: &str, pillar: ForgePillar, snippet: &str) {
            self.frames.insert(
                id.into(),
                FrameMeta {
                    frame_id: id.into(),
                    tag: tag.into(),
                    pillar,
                    snippet: snippet.into(),
                },
            );
        }
        fn canned(&mut self, query: &str, hits: Vec<(String, f32)>) {
            self.canned.push((query.into(), hits));
        }
    }

    impl BrainAccess for StubBrain {
        fn search_by_pillar(
            &mut self,
            query: &str,
            _top_k: usize,
            _pillars: &std::collections::HashSet<ForgePillar>,
        ) -> Vec<(String, f32)> {
            self.canned
                .iter()
                .find(|(q, _)| q == query)
                .map(|(_, hits)| hits.clone())
                .unwrap_or_default()
        }
        fn sym(
            &mut self,
            name: &str,
            _top_k: usize,
        ) -> Vec<(String, ForgePillar)> {
            self.canned
                .iter()
                .find(|(q, _)| q == name)
                .map(|(_, hits)| {
                    hits.iter()
                        .filter_map(|(id, _)| {
                            self.frames.get(id).map(|m| (id.clone(), m.pillar))
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        fn meta(&self, frame_id: &str) -> Option<FrameMeta> {
            self.frames.get(frame_id).cloned()
        }
    }

    fn api_story(slug: &str, method: &str, path: &str) -> Story {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("method".into(), serde_json::json!(method));
        fields.insert("path".into(), serde_json::json!(path));
        Story {
            slug: slug.into(),
            title: format!("{} {}", method, path),
            raw_text: String::new(),
            kind: StoryKind::ApiEndpoint,
            fields,
            directive_hash: "h".into(),
            source_adapter: "openapi".into(),
            source_anchor: format!("paths.{}.{}", path, method.to_lowercase()),
        }
    }

    #[test]
    fn full_support_when_law_and_requested_both_match() {
        let mut brain = StubBrain::new();
        brain.add_frame("law1", "authority:law:ground-truth sql", ForgePillar::Code, "create cardholder proc");
        brain.add_frame("req1", "authority:requested:requirements pdf", ForgePillar::External, "client rule: freeze must audit");
        brain.canned("POST /cardholder/freeze", vec![("law1".into(), 0.9), ("req1".into(), 0.85)]);
        brain.canned("cardholder freeze", vec![("req1".into(), 0.85)]);
        let s = api_story("post-cardholder-freeze", "POST", "/cardholder/freeze");
        let op = analyse_operation(&mut brain, &s).unwrap();
        assert_eq!(op.status, GapStatus::FullSupport);
    }

    #[test]
    fn partial_support_when_only_law_matches() {
        let mut brain = StubBrain::new();
        brain.add_frame("law1", "authority:law:ground-truth", ForgePillar::Code, "orphan sql");
        brain.canned("POST /orphan", vec![("law1".into(), 0.9)]);
        brain.canned("orphan", vec![("law1".into(), 0.5)]);
        let s = api_story("post-orphan", "POST", "/orphan");
        let op = analyse_operation(&mut brain, &s).unwrap();
        assert_eq!(op.status, GapStatus::PartialSupport);
    }

    #[test]
    fn no_support_when_requested_but_no_law() {
        let mut brain = StubBrain::new();
        brain.add_frame("req1", "authority:requested:requirements", ForgePillar::External, "client asks for magic api");
        brain.canned("POST /magic", vec![("req1".into(), 0.85)]);
        brain.canned("magic", vec![("req1".into(), 0.85)]);
        let s = api_story("post-magic", "POST", "/magic");
        let op = analyse_operation(&mut brain, &s).unwrap();
        assert_eq!(op.status, GapStatus::NoSupport);
        assert!(op.recommendation.contains("ground-truth"));
    }

    #[test]
    fn dual_directive_detects_only_in_primary() {
        let primary = vec![
            api_story("a", "GET", "/a"),
            api_story("b", "GET", "/b"),
        ];
        let secondary = vec![api_story("a", "GET", "/a")];
        let conflicts = detect_dual_directive_conflicts(&primary, &secondary);
        assert_eq!(conflicts.len(), 1);
        match &conflicts[0] {
            DirectiveConflict::OnlyInPrimary { slug, .. } => assert_eq!(slug, "b"),
            _ => panic!("expected OnlyInPrimary"),
        }
    }

    #[test]
    fn dual_directive_detects_path_divergence() {
        let primary = vec![api_story("x", "POST", "/cardholder/freeze")];
        let secondary = vec![api_story("x", "POST", "/cardholder/{id}/freeze")];
        let conflicts = detect_dual_directive_conflicts(&primary, &secondary);
        assert_eq!(conflicts.len(), 1);
        match &conflicts[0] {
            DirectiveConflict::PathDivergence { slug, primary_path, secondary_path } => {
                assert_eq!(slug, "x");
                assert_eq!(primary_path, "/cardholder/freeze");
                assert_eq!(secondary_path, "/cardholder/{id}/freeze");
            }
            _ => panic!("expected PathDivergence"),
        }
    }

    #[test]
    fn to_markdown_renders_summary_and_ops() {
        let report = GapReport {
            project: "dtcard".into(),
            generated_at: "unix_ts=1".into(),
            primary_directive: "spec/api.yaml".into(),
            secondary_directive: None,
            authority_order: vec!["agreed".into(), "wishlist".into()],
            operations: vec![OperationGap {
                slug: "post-x".into(),
                label: "POST /x".into(),
                status: GapStatus::FullSupport,
                evidence: vec![Evidence {
                    authority: "law".into(),
                    source: "tag:sql".into(),
                    snippet: "create proc".into(),
                    score: 0.9,
                }],
                recommendation: String::new(),
            }],
            directive_conflicts: Vec::new(),
        };
        let md = report.to_markdown();
        assert!(md.contains("# Gap Report — dtcard"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("✓ 1 fully supported"));
        assert!(md.contains("POST /x"));
        assert!(md.contains("law"));
    }

    #[test]
    fn build_report_flips_conflict_slugs_to_conflict_status() {
        let mut brain = StubBrain::new();
        brain.add_frame("law1", "authority:law:ground-truth", ForgePillar::Code, "x");
        brain.canned("POST /a", vec![("law1".into(), 0.9)]);
        brain.canned("a", vec![("law1".into(), 0.5)]);
        let primary = vec![api_story("a", "POST", "/a")];
        let secondary = vec![api_story("a", "POST", "/a/alt")];
        let rep = build_report(
            BuildReportInput {
                project: "p",
                primary_directive: "pri",
                secondary_directive: Some("sec"),
                authority_order: &[],
                primary_stories: &primary,
                secondary_stories: Some(&secondary),
            },
            &mut brain,
        )
        .unwrap();
        assert_eq!(rep.operations[0].status, GapStatus::Conflict);
        assert!(!rep.directive_conflicts.is_empty());
    }
}
