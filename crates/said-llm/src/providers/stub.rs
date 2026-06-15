//! Test-only LLM stub. Returns canned responses matched by substring on the
//! user message. Used in unit/integration tests so we never hit a real LLM.

use crate::error::{LlmError, LlmFailureClass, LlmResult};
use crate::types::{
    CompletionRequest, CompletionResponse, LlmCapabilities, LlmProvider, TokenUsage,
};
use async_trait::async_trait;
use std::sync::Mutex;

pub struct StubProvider {
    canned: Mutex<Vec<Canned>>,
    failures: Mutex<Vec<LlmFailureClass>>,
    call_count: Mutex<u32>,
}

#[derive(Clone)]
struct Canned {
    match_user_contains: String,
    response: serde_json::Value,
}

impl StubProvider {
    pub fn new() -> Self {
        Self {
            canned: Mutex::new(Vec::new()),
            failures: Mutex::new(Vec::new()),
            call_count: Mutex::new(0),
        }
    }

    pub fn canned_for(&self, match_user_contains: &str, json: serde_json::Value) {
        self.canned.lock().unwrap().push(Canned {
            match_user_contains: match_user_contains.to_string(),
            response: json,
        });
    }

    pub fn queue_failures(&self, classes: &[LlmFailureClass]) {
        self.failures.lock().unwrap().extend_from_slice(classes);
    }

    pub fn calls(&self) -> u32 {
        *self.call_count.lock().unwrap()
    }
}

impl Default for StubProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for StubProvider {
    fn name(&self) -> &'static str {
        "stub"
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities {
            supports_structured_output: true,
            supports_prompt_caching: false,
            max_context_window: 200_000,
            default_max_output_tokens: 8_192,
        }
    }

    async fn complete(&self, req: &CompletionRequest) -> LlmResult<CompletionResponse> {
        *self.call_count.lock().unwrap() += 1;
        let maybe_fail = {
            let mut guard = self.failures.lock().unwrap();
            if guard.is_empty() {
                None
            } else {
                Some(guard.remove(0))
            }
        };
        if let Some(class) = maybe_fail {
            return Err(match class {
                LlmFailureClass::Auth => LlmError::Llm("stub auth failure".into()),
                LlmFailureClass::RateLimit => LlmError::Llm("stub rate limit".into()),
                LlmFailureClass::Network => LlmError::Http {
                    url: "stub".into(),
                    message: "network".into(),
                },
                LlmFailureClass::ContextExceeded => LlmError::ContextExceeded {
                    slug: "stub".into(),
                    estimated_tokens: 300_000,
                },
                LlmFailureClass::Parse => LlmError::Llm("stub: malformed".into()),
                LlmFailureClass::Other => LlmError::Llm("stub: other".into()),
            });
        }
        let canned = {
            let guard = self.canned.lock().unwrap();
            guard
                .iter()
                .find(|c| req.user.contains(&c.match_user_contains))
                .cloned()
        };
        let json = canned.map(|c| c.response).unwrap_or_else(|| {
            serde_json::json!({
                "spec": { "overview": "Stub output.", "actors": [], "acceptance_criteria": [] },
                "plan": { "steps": [] },
                "tasks": [],
                "brain_refs": []
            })
        });
        let raw = serde_json::to_string(&json).unwrap();
        Ok(CompletionResponse {
            json,
            raw,
            usage: TokenUsage {
                input_tokens: 1000,
                output_tokens: 250,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            provider: "stub".into(),
            model: "stub-model".into(),
            duration_ms: 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_req(user: &str) -> CompletionRequest {
        CompletionRequest {
            system: "sys".into(),
            user: user.into(),
            cacheable_prelude: None,
            schema: json!({}),
            schema_name: "s".into(),
            max_output_tokens: 100,
            temperature: 0.0,
            json_object: false,
        }
    }

    #[tokio::test]
    async fn returns_canned_on_substring_match() {
        let p = StubProvider::new();
        p.canned_for("post-pet", json!({ "hit": "pet" }));
        p.canned_for("get-user", json!({ "hit": "user" }));

        let r = p.complete(&sample_req("working on post-pet story")).await.unwrap();
        assert_eq!(r.json["hit"], "pet");

        let r = p.complete(&sample_req("working on get-user story")).await.unwrap();
        assert_eq!(r.json["hit"], "user");
    }

    #[tokio::test]
    async fn falls_back_to_default_scaffold() {
        let p = StubProvider::new();
        let r = p.complete(&sample_req("nothing canned for this")).await.unwrap();
        assert!(r.json["spec"]["overview"].is_string());
    }

    #[tokio::test]
    async fn failures_drain_in_order() {
        let p = StubProvider::new();
        p.queue_failures(&[LlmFailureClass::Auth, LlmFailureClass::RateLimit]);
        let e1 = p.complete(&sample_req("x")).await.unwrap_err();
        let e2 = p.complete(&sample_req("x")).await.unwrap_err();
        let _ok = p.complete(&sample_req("x")).await.unwrap();
        assert!(format!("{}", e1).contains("auth"));
        assert!(format!("{}", e2).contains("rate limit"));
        assert_eq!(p.calls(), 3);
    }
}
