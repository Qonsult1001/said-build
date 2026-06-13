//! Fusion: grounding + story + schema → specification + plan + tasks.
//!
//! Per spec §9. LLM-driven, with mem0 post-validation and one retry on parse error.

pub mod prompt;
pub mod schema;
pub mod validate;

use crate::grounding::{GroundingHit, GroundingReport};
use crate::llm::{CompletionRequest, LlmProvider};
use crate::{ForgeError, ForgeResult, Story};
use serde::{Deserialize, Serialize};

pub use schema::{BrainRef, GeneratedArtifacts, PlanDoc, PlanStep, SpecDoc, TaskItem};

/// One call to `generate` returns everything needed to project a story.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationResult {
    pub artifacts: GeneratedArtifacts,
    pub validation: validate::ValidationReport,
    pub prompt: String,
    pub raw_response: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    pub duration_ms: u64,
}

impl GenerationResult {
    pub fn usage_cache_hit_ratio(&self) -> f32 {
        let total = self.input_tokens + self.cache_read_tokens;
        if total == 0 { 0.0 } else { self.cache_read_tokens as f32 / total as f32 }
    }
}

/// Generate one story's artifacts. Retries once on parse error (per spec §13.1).
pub async fn generate(
    story: &Story,
    hits: &[GroundingHit],
    report: &GroundingReport,
    llm: &dyn LlmProvider,
    inline_top_n: usize,
) -> ForgeResult<GenerationResult> {
    let (system, user, cacheable) = prompt::build(
        story,
        hits,
        report,
        inline_top_n,
        llm.capabilities().supports_prompt_caching,
    );
    let schema_value = schema::output_schema();
    let req = CompletionRequest {
        system: system.clone(),
        user: user.clone(),
        cacheable_prelude: cacheable,
        schema: schema_value,
        schema_name: "record_story_generation".into(),
        max_output_tokens: 4096,
        temperature: 0.1,
    };

    match llm.complete(&req).await {
        Ok(resp) => {
            let parsed: Result<GeneratedArtifacts, _> = serde_json::from_value(resp.json.clone());
            match parsed {
                Ok(artifacts) => {
                    let report_v = validate::run_all(&artifacts, hits);
                    Ok(GenerationResult {
                        artifacts,
                        validation: report_v,
                        prompt: format!("{}\n---\n{}", system, user),
                        raw_response: resp.raw,
                        provider: resp.provider,
                        model: resp.model,
                        input_tokens: resp.usage.input_tokens,
                        output_tokens: resp.usage.output_tokens,
                        cache_read_tokens: resp.usage.cache_read_tokens,
                        cache_write_tokens: resp.usage.cache_write_tokens,
                        duration_ms: resp.duration_ms,
                    })
                }
                Err(e) => {
                    // One retry with the parse error appended to user msg.
                    let feedback = format!(
                        "\n\n[RETRY] Your previous response failed to parse: {}. Please return JSON strictly matching the `record_story_generation` schema.",
                        e
                    );
                    let retry_req = CompletionRequest {
                        user: format!("{}{}", user, feedback),
                        ..req.clone()
                    };
                    match llm.complete(&retry_req).await {
                        Ok(resp2) => {
                            let artifacts: GeneratedArtifacts = serde_json::from_value(resp2.json.clone())
                                .map_err(|e| ForgeError::Validation(format!("second parse failure: {}", e)))?;
                            let report_v = validate::run_all(&artifacts, hits);
                            Ok(GenerationResult {
                                artifacts,
                                validation: report_v,
                                prompt: format!("{}\n---\n{}{}", system, user, feedback),
                                raw_response: resp2.raw,
                                provider: resp2.provider,
                                model: resp2.model,
                                input_tokens: resp2.usage.input_tokens,
                                output_tokens: resp2.usage.output_tokens,
                                cache_read_tokens: resp2.usage.cache_read_tokens,
                                cache_write_tokens: resp2.usage.cache_write_tokens,
                                duration_ms: resp2.duration_ms,
                            })
                        }
                        Err(e2) => Err(e2),
                    }
                }
            }
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::{GroundingHit, GroundingReport};
    use crate::llm::stub::StubProvider;
    use crate::story::Pillar;
    use crate::StoryKind;
    use serde_json::json;

    fn story() -> Story {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("method".into(), json!("POST"));
        fields.insert("path".into(), json!("/pet"));
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

    fn hits() -> Vec<GroundingHit> {
        vec![
            GroundingHit {
                frame_id: "f1".into(),
                tag: "code:pet".into(),
                score: 1.2,
                query_origin: "method+path".into(),
                snippet: "pub struct Pet { name: String }".into(),
                pillar: Pillar::Code,
                authority: None,
            },
            GroundingHit {
                frame_id: "f2".into(),
                tag: "format:openapi".into(),
                score: 1.0,
                query_origin: "tag".into(),
                snippet: "POST /pet 201 creates Pet".into(),
                pillar: Pillar::External,
                authority: None,
            },
        ]
    }

    fn empty_report() -> GroundingReport {
        let mut m = std::collections::BTreeMap::new();
        for p in Pillar::ALL {
            m.insert(p.to_string(), 0);
        }
        m.insert("Code".into(), 1);
        m.insert("External".into(), 1);
        GroundingReport {
            queries: Vec::new(),
            selected_frame_ids: vec!["f1".into(), "f2".into()],
            inlined_frame_ids: vec!["f1".into(), "f2".into()],
            pillar_coverage: m,
        }
    }

    #[tokio::test]
    async fn happy_path_end_to_end_with_stub() {
        let p = StubProvider::new();
        p.canned_for(
            "post-pet",
            json!({
                "spec": {
                    "overview": "The POST /pet endpoint creates a Pet resource and returns the stored record with its assigned id.",
                    "actors": ["api consumer"],
                    "acceptance_criteria": [
                        "Valid POST /pet with name and photoUrls creates a Pet row via the code in f1 and returns 201 with the stored record."
                    ]
                },
                "plan": {
                    "steps": [
                        { "id": "S1", "action": "Implement the POST /pet handler using the Pet struct defined in f1.", "grounding_frame_ids": ["f1"] }
                    ]
                },
                "tasks": [
                    { "id": "T1", "text": "Write integration test for POST /pet against the handler.", "grounding_frame_ids": ["f1"] }
                ],
                "brain_refs": [
                    { "frame_id": "f1", "why_relevant": "Defines the Pet struct shape." },
                    { "frame_id": "f2", "why_relevant": "OpenAPI spec for POST /pet." }
                ]
            }),
        );

        let result = generate(&story(), &hits(), &empty_report(), &p, 8).await.unwrap();
        assert_eq!(result.artifacts.plan.steps.len(), 1);
        assert_eq!(result.artifacts.spec.acceptance_criteria.len(), 1);
        assert!(result.validation.fabricated_frame_ids.is_empty());
        assert!(result.validation.needs_input.is_empty());
        assert_eq!(result.provider, "stub");
        assert_eq!(result.input_tokens, 1000);
    }

    #[tokio::test]
    async fn retry_on_parse_error_succeeds_second_attempt() {
        // Register [RETRY] match FIRST so it wins on second call (which contains
        // both "post-pet" and "[RETRY]" substrings).
        let p = StubProvider::new();
        p.canned_for(
            "[RETRY]",
            json!({
                "spec": { "overview": "Valid after retry.", "actors": [], "acceptance_criteria": ["A valid criterion."] },
                "plan": { "steps": [{ "id": "S1", "action": "Do thing.", "grounding_frame_ids": [] }] },
                "tasks": [{ "id": "T1", "text": "Test thing.", "grounding_frame_ids": [] }],
                "brain_refs": []
            }),
        );
        p.canned_for("post-pet", json!({ "spec": { "overview": "oops" } }));

        let result = generate(&story(), &hits(), &empty_report(), &p, 8).await.unwrap();
        assert_eq!(result.artifacts.spec.overview, "Valid after retry.");
    }

    #[tokio::test]
    async fn second_parse_failure_returns_error() {
        let p = StubProvider::new();
        // [RETRY] first so retry call matches it, not the "post-pet" match.
        p.canned_for("[RETRY]", json!({ "still": "bad" }));
        p.canned_for("post-pet", json!({ "spec": { "overview": "oops" } }));
        let err = generate(&story(), &hits(), &empty_report(), &p, 8).await.unwrap_err();
        let msg = format!("{}", err).to_lowercase();
        assert!(msg.contains("parse") || msg.contains("validation") || msg.contains("missing field"),
            "expected parse/validation error, got: {}", msg);
    }

    #[tokio::test]
    async fn fabricated_frame_ids_are_reported_not_fatal() {
        let p = StubProvider::new();
        p.canned_for(
            "post-pet",
            json!({
                "spec": { "overview": "OK.", "actors": [], "acceptance_criteria": ["OK."] },
                "plan": { "steps": [{ "id": "S1", "action": "Uses invented FAKE.", "grounding_frame_ids": ["FAKE-ID"] }] },
                "tasks": [{ "id": "T1", "text": "OK.", "grounding_frame_ids": [] }],
                "brain_refs": []
            }),
        );
        let result = generate(&story(), &hits(), &empty_report(), &p, 8).await.unwrap();
        assert_eq!(result.validation.fabricated_frame_ids, vec!["FAKE-ID".to_string()]);
    }
}
