//! System prompt assembly.
//!
//! Per spec §9.2. Seven numbered sections. mem0 principles embedded verbatim.
//! Story-kind hint and pillar-absence flags computed from the story + report.

use crate::grounding::{GroundingHit, GroundingReport};
use crate::story::Pillar;
use crate::{Story, StoryKind};

/// Build `(system, user, cacheable_prelude)`.
pub fn build(
    story: &Story,
    hits: &[GroundingHit],
    report: &GroundingReport,
    inline_top_n: usize,
    supports_caching: bool,
) -> (String, String, Option<String>) {
    let system = build_system(story, report);
    let grounding_prelude = build_grounding_prelude(hits, inline_top_n);
    let user = build_user(story);
    if supports_caching {
        (system, user, Some(grounding_prelude))
    } else {
        (system, format!("{}\n\n{}", grounding_prelude, user), None)
    }
}

fn build_system(story: &Story, report: &GroundingReport) -> String {
    let mut s = String::new();
    s.push_str("# 1. Your role\n\n");
    s.push_str("You generate a specification, implementation plan, task list, and brain-reference list for a single user story, grounded in the retrieved frames provided. Output must call the `record_story_generation` tool with a JSON payload matching the schema.\n\n");

    s.push_str("# 2. Output shape — atomic, contextually rich, bounded\n\n");
    s.push_str("- Each acceptance criterion, plan step, and task: 15–80 words, 1–2 sentences, self-contained. Up to 100 words and 3 sentences only when the item names multiple proper nouns, column names, HTTP status codes, or enumerated specifics.\n");
    s.push_str("- Preserve all proper nouns verbatim: column names, endpoint paths, type names, stored procedure names, status codes. Never generalise a specific identifier to a category. (BAD: 'Use the auth table'. GOOD: 'Use dbo.user_auth with the session_token column'.)\n");
    s.push_str("- Split dense logic into multiple focused steps rather than one paragraph. Prefer 8 focused steps over 3 dense ones.\n");
    s.push_str("- Every piece of information appears exactly once across spec / plan / tasks, in the most appropriate place. No echo.\n\n");

    s.push_str("# 3. Grounding contract\n\n");
    s.push_str("Every specific detail — column name, endpoint path, type name, status code — must appear in one of the grounding frames listed in section 7. If a detail is absent from the grounding, do not invent it. Instead emit `NEEDS-INPUT:<what's-missing>` in the spec's acceptance_criteria.\n\n");
    s.push_str("Every `grounding_frame_ids` entry in plan/tasks MUST be one of the frame IDs listed in section 7. Hallucinated IDs will be flagged and the run marked `failed_grounding_check`.\n\n");

    s.push_str("# 4. Story kind hint\n\n");
    s.push_str(&story_kind_hint(story.kind));
    s.push_str("\n\n");

    s.push_str("# 5. Pillar coverage\n\n");
    s.push_str(&pillar_coverage_block(story, report));
    s.push_str("\n\n");

    s.push_str("# 6. Output schema\n\n");
    s.push_str("Call the tool `record_story_generation` with JSON conforming to its `input_schema`.\n\n");

    s.push_str("# 8. Authority hierarchy\n\n");
    s.push_str("Every grounding frame from the forge sync pipeline carries an `authority` level parsed from its `authority:<level>:<scope>` tag. When two frames disagree, cite the one with higher authority first:\n\n");
    s.push_str("- **law** (ground-truth): SQL schema / stored procedures. Authoritative. If this disagrees with anything else, law wins.\n");
    s.push_str("- **requested** (client): raw client-stated requirement from PDFs or XLSX catalogues. Treat as hard constraint unless the user has explicitly tagged a file as soft.\n");
    s.push_str("- **agreed** (Dev Planning): our agreed scope. Reconcile with law where they diverge.\n");
    s.push_str("- **existing** (progress): current framework code. Reference-grade; may diverge from law.\n");
    s.push_str("- **wishlist** (OpenAPI): client's asked-for contract. May not align with law; flag gaps as `NEEDS-INPUT:`.\n\n");
    s.push_str("When frames at different authorities disagree, cite the highest-authority frame first in the acceptance criteria and flag the gap explicitly.\n\n");

    s
}

fn story_kind_hint(kind: StoryKind) -> String {
    match kind {
        StoryKind::ApiEndpoint => "This is an HTTP API endpoint. The spec must describe request/response contracts precisely, citing the schema names from grounding. Plan steps should cover: handler implementation, request validation, persistence layer calls, response serialization, error mapping, and integration tests.".into(),
        StoryKind::Requirement => "This is a business requirement. The spec must describe the user-visible outcome in business terms. Plan steps should translate the requirement into discrete code/data changes grounded in the existing schema and codebase. Where the requirement is ambiguous, add a `NEEDS-INPUT:` acceptance criterion.".into(),
        StoryKind::Ticket => "This is a ticket from an issue tracker. Treat priority, status, and labels from the grounding as authoritative. Plan steps cover implementation, testing, and any migration path the ticket describes.".into(),
        StoryKind::TableRow => "This is a structured row from a spreadsheet or table. Each column in grounding may be a separate field of the spec. Plan steps translate row-by-row into discrete code changes.".into(),
        StoryKind::Generic => "This is a free-form story with minimal structure. Lean heavily on the grounding frames. If grounding is sparse, the plan should include discovery/investigation steps before implementation.".into(),
    }
}

fn pillar_coverage_block(story: &Story, report: &GroundingReport) -> String {
    let mut populated = Vec::new();
    let mut absent = Vec::new();
    for p in Pillar::ALL {
        let count = report.pillar_coverage.get(&p.to_string()).copied().unwrap_or(0);
        if count > 0 {
            populated.push(format!("{} ({})", p, count));
        } else {
            absent.push(p.to_string());
        }
    }
    let expected = story.kind.expected_pillars();
    let missing_expected: Vec<String> = expected
        .iter()
        .filter(|p| {
            report.pillar_coverage.get(&p.to_string()).copied().unwrap_or(0) == 0
        })
        .map(|p| p.to_string())
        .collect();

    let mut s = String::new();
    s.push_str(&format!(
        "Populated pillars (with frame counts): {}.\n",
        if populated.is_empty() { "(none)".into() } else { populated.join(", ") }
    ));
    s.push_str(&format!("Absent pillars: {}.\n", absent.join(", ")));
    if !missing_expected.is_empty() {
        s.push_str(&format!(
            "This `{}` story normally expects the following pillars to be populated but they are absent: {}. For each absent expected pillar, add one `NEEDS-INPUT:pillar:<name>` entry to the spec's acceptance_criteria.\n",
            story.kind,
            missing_expected.join(", ")
        ));
    }
    s
}

fn build_grounding_prelude(hits: &[GroundingHit], inline_top_n: usize) -> String {
    let mut s = String::new();
    s.push_str("# 7. Grounding frames\n\n");
    if hits.is_empty() {
        s.push_str("(no frames retrieved — you must flag NEEDS-INPUT for any specific detail)\n");
        return s;
    }
    s.push_str(&format!(
        "The following frame IDs are authoritative. Use ONLY these IDs in `grounding_frame_ids`. The top {} are inlined in full.\n\n",
        inline_top_n.min(hits.len())
    ));
    s.push_str("## Inlined (use these details directly)\n\n");
    for (i, hit) in hits.iter().take(inline_top_n).enumerate() {
        s.push_str(&format!(
            "### Frame {} — `{}` (pillar: {}, authority: {}, score: {:.3})\n{}\n\n",
            i + 1,
            hit.frame_id,
            hit.pillar,
            hit.authority.as_deref().unwrap_or("unknown"),
            hit.score,
            hit.snippet
        ));
    }
    if hits.len() > inline_top_n {
        s.push_str("## References (not inlined)\n\n");
        for hit in hits.iter().skip(inline_top_n) {
            s.push_str(&format!(
                "- `{}` (pillar: {}, score: {:.3}) — {}\n",
                hit.frame_id,
                hit.pillar,
                hit.score,
                hit.snippet.chars().take(80).collect::<String>()
            ));
        }
        s.push('\n');
    }
    s
}

fn build_user(story: &Story) -> String {
    let mut s = String::new();
    s.push_str("# Story\n\n");
    s.push_str(&format!("slug: `{}`\n", story.slug));
    s.push_str(&format!("title: {}\n", story.title));
    s.push_str(&format!("kind: {}\n", story.kind));
    s.push_str(&format!("source_adapter: {}\n", story.source_adapter));
    s.push_str(&format!("source_anchor: {}\n\n", story.source_anchor));
    s.push_str("## raw_text\n");
    s.push_str(&story.raw_text);
    s.push_str("\n\n");
    if !story.fields.is_empty() {
        s.push_str("## fields (source-specific structured data)\n");
        s.push_str("```json\n");
        s.push_str(&serde_json::to_string_pretty(&story.fields).unwrap_or_else(|_| "{}".into()));
        s.push_str("\n```\n\n");
    }
    s.push_str("Generate the spec + plan + tasks + brain_refs now.\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::GroundingHit;
    use std::collections::BTreeMap;

    fn story(kind: StoryKind) -> Story {
        Story {
            slug: "post-pet".into(),
            title: "Add a new pet".into(),
            raw_text: "POST /pet creates a Pet resource.".into(),
            kind,
            fields: BTreeMap::new(),
            directive_hash: "a3f91".into(),
            source_adapter: "openapi".into(),
            source_anchor: "paths./pet.post".into(),
        }
    }

    fn report(counts: &[(&str, u32)]) -> GroundingReport {
        let mut m = std::collections::BTreeMap::new();
        for (name, n) in counts {
            m.insert((*name).to_string(), *n);
        }
        for p in Pillar::ALL {
            m.entry(p.to_string()).or_insert(0);
        }
        GroundingReport {
            queries: Vec::new(),
            selected_frame_ids: Vec::new(),
            inlined_frame_ids: Vec::new(),
            pillar_coverage: m,
        }
    }

    #[test]
    fn system_prompt_has_all_six_section_headers() {
        let s = build_system(&story(StoryKind::ApiEndpoint), &report(&[]));
        for header in ["# 1.", "# 2.", "# 3.", "# 4.", "# 5.", "# 6."] {
            assert!(s.contains(header), "missing {}", header);
        }
    }

    #[test]
    fn api_endpoint_hint_mentions_request_response_contracts() {
        let s = build_system(&story(StoryKind::ApiEndpoint), &report(&[]));
        assert!(s.to_lowercase().contains("request/response"));
    }

    #[test]
    fn requirement_hint_differs_from_api() {
        let r = build_system(&story(StoryKind::Requirement), &report(&[]));
        let a = build_system(&story(StoryKind::ApiEndpoint), &report(&[]));
        assert_ne!(r, a);
        assert!(r.contains("business"));
    }

    #[test]
    fn pillar_coverage_flags_missing_expected_for_api() {
        // ApiEndpoint expects Code + External. Here only External is populated.
        let s = build_system(&story(StoryKind::ApiEndpoint), &report(&[("External", 2)]));
        assert!(s.contains("NEEDS-INPUT:pillar:"));
        assert!(s.contains("Code"));
    }

    #[test]
    fn grounding_prelude_inlines_top_n_and_references_rest() {
        let hits = (0..12)
            .map(|i| GroundingHit {
                frame_id: format!("f{}", i),
                tag: "code:x".into(),
                score: 1.0 - (i as f32 * 0.05),
                query_origin: "method+path".into(),
                snippet: format!("snippet {}", i),
                pillar: Pillar::Code,
                authority: None,
            })
            .collect::<Vec<_>>();
        let prelude = build_grounding_prelude(&hits, 3);
        assert!(prelude.contains("## Inlined"));
        assert!(prelude.contains("Frame 1 — `f0`"));
        assert!(prelude.contains("Frame 3 — `f2`"));
        assert!(prelude.contains("## References"));
        assert!(prelude.contains("- `f3`"));
    }

    #[test]
    fn build_returns_cacheable_prelude_when_supported() {
        let s = story(StoryKind::ApiEndpoint);
        let (sys, user, prelude) = build(&s, &[], &report(&[]), 8, true);
        assert!(!sys.is_empty());
        assert!(user.starts_with("# Story"));
        assert!(prelude.is_some());
    }

    #[test]
    fn build_folds_prelude_into_user_when_caching_unsupported() {
        let s = story(StoryKind::ApiEndpoint);
        let (_sys, user, prelude) = build(&s, &[], &report(&[]), 8, false);
        assert!(prelude.is_none());
        assert!(user.contains("Grounding frames"));
    }

    #[test]
    fn absent_pillar_list_does_not_flag_non_expected() {
        let s = build_system(&story(StoryKind::Generic), &report(&[]));
        assert!(!s.contains("NEEDS-INPUT:pillar:"));
    }
}
