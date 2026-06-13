//! OpenAI-compatible Chat Completions provider.
//!
//! Works against OpenAI, Azure OpenAI, OpenRouter, LiteLLM proxy, Ollama
//! (which exposes a `/v1/chat/completions` shim), and any other provider that
//! honours OpenAI's Chat Completions API.
//!
//! Uses `response_format: { type: "json_schema", strict: true, ... }` for
//! structured output.

use crate::config::LlmConfig;
use crate::error::{LlmError, LlmResult};
use crate::types::{
    CompletionRequest, CompletionResponse, LlmCapabilities, LlmProvider, TokenUsage,
};
use async_trait::async_trait;
use serde_json::json;
use std::time::Instant;

pub struct OpenAICompatibleProvider {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAICompatibleProvider {
    pub fn from_config(cfg: &LlmConfig) -> LlmResult<Self> {
        let api_key = cfg.api_key.clone().ok_or_else(|| {
            LlmError::Config("openai-compatible provider requires `api_key`".into())
        })?;
        let base_url = cfg.base_url.clone().ok_or_else(|| {
            LlmError::Config("openai-compatible provider requires `base_url`".into())
        })?;
        Ok(Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .map_err(|e| LlmError::Config(format!("reqwest: {}", e)))?,
        })
    }

    pub(crate) fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let mut system = String::new();
        if let Some(prelude) = &req.cacheable_prelude {
            system.push_str(prelude);
            system.push_str("\n\n");
        }
        system.push_str(&req.system);
        json!({
            "model": self.model,
            "temperature": req.temperature,
            "max_tokens": req.max_output_tokens,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": req.schema_name,
                    "strict": true,
                    "schema": req.schema,
                }
            },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": req.user }
            ]
        })
    }

    pub(crate) fn parse_body(
        &self,
        body: &serde_json::Value,
        raw: &str,
        elapsed_ms: u64,
    ) -> LlmResult<CompletionResponse> {
        let choices = body
            .get("choices")
            .and_then(|v| v.as_array())
            .ok_or_else(|| LlmError::Llm(format!("no choices in response: {}", raw)))?;
        let first = choices
            .first()
            .ok_or_else(|| LlmError::Llm("empty choices".into()))?;
        let message = first
            .get("message")
            .ok_or_else(|| LlmError::Llm("no message".into()))?;
        let content = message
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LlmError::Llm("no message content".into()))?;
        let parsed_json: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| LlmError::Llm(format!("json parse: {}", e)))?;
        let usage = body.get("usage");
        let tok = TokenUsage {
            input_tokens: usage
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: usage
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_read_tokens: usage
                .and_then(|u| u.get("prompt_tokens_details"))
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_write_tokens: 0,
        };
        Ok(CompletionResponse {
            json: parsed_json,
            raw: raw.to_string(),
            usage: tok,
            provider: "openai-compatible".into(),
            model: self.model.clone(),
            duration_ms: elapsed_ms,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    fn name(&self) -> &'static str {
        "openai-compatible"
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities {
            supports_structured_output: true,
            supports_prompt_caching: false,
            max_context_window: 128_000,
            default_max_output_tokens: 4_096,
        }
    }

    async fn complete(&self, req: &CompletionRequest) -> LlmResult<CompletionResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(req);
        let start = Instant::now();
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Http {
                url: url.clone(),
                message: e.to_string(),
            })?;
        let status = resp.status();
        let raw = resp.text().await.map_err(|e| LlmError::Http {
            url: url.clone(),
            message: e.to_string(),
        })?;
        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(LlmError::Llm(format!("openai-compat auth failure: {}", raw)));
            }
            if status.as_u16() == 429 {
                return Err(LlmError::Llm(format!("openai-compat rate limit: {}", raw)));
            }
            return Err(LlmError::Llm(format!("openai-compat HTTP {}: {}", status, raw)));
        }
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| LlmError::Llm(format!("openai-compat JSON parse: {}", e)))?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        self.parse_body(&parsed, &raw, elapsed_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmProviderKind;

    fn provider() -> OpenAICompatibleProvider {
        let cfg = LlmConfig {
            provider: LlmProviderKind::OpenAiCompatible,
            model: "gpt-4o-2024-08-06".into(),
            api_key: Some("sk-test".into()),
            base_url: Some("https://api.openai.com/v1/".into()),
        };
        OpenAICompatibleProvider::from_config(&cfg).unwrap()
    }

    fn sample_req() -> CompletionRequest {
        CompletionRequest {
            system: "Rules.".into(),
            user: "Generate.".into(),
            cacheable_prelude: Some("Grounding prelude.".into()),
            schema: json!({ "type": "object", "properties": {"spec": {"type": "object"}}, "required": ["spec"] }),
            schema_name: "story_gen".into(),
            max_output_tokens: 2000,
            temperature: 0.1,
        }
    }

    #[test]
    fn base_url_trailing_slash_stripped() {
        let p = provider();
        assert!(!p.base_url.ends_with('/'));
    }

    #[test]
    fn body_uses_json_schema_strict() {
        let p = provider();
        let body = p.build_body(&sample_req());
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["response_format"]["json_schema"]["name"], "story_gen");
    }

    #[test]
    fn body_concatenates_prelude_into_system() {
        let p = provider();
        let body = p.build_body(&sample_req());
        let sys = body["messages"][0]["content"].as_str().unwrap();
        assert!(sys.starts_with("Grounding prelude."));
        assert!(sys.contains("Rules."));
    }

    #[test]
    fn parse_body_extracts_content_and_usage() {
        let p = provider();
        let raw = r#"{
          "choices": [{
            "message": {
              "role": "assistant",
              "content": "{\"spec\":{\"overview\":\"hi\"}}"
            }
          }],
          "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 30,
            "prompt_tokens_details": { "cached_tokens": 80 }
          }
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
        let resp = p.parse_body(&parsed, raw, 900).unwrap();
        assert_eq!(resp.json["spec"]["overview"], "hi");
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 30);
        assert_eq!(resp.usage.cache_read_tokens, 80);
    }

    #[test]
    fn from_config_errors_without_base_url() {
        let cfg = LlmConfig {
            provider: LlmProviderKind::OpenAiCompatible,
            model: "x".into(),
            api_key: Some("k".into()),
            base_url: None,
        };
        match OpenAICompatibleProvider::from_config(&cfg) {
            Err(e) => assert!(format!("{}", e).contains("base_url")),
            Ok(_) => panic!("expected base_url error"),
        }
    }
}
