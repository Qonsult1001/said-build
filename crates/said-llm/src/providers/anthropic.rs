//! Anthropic Messages API provider.
//!
//! Uses a `tools` array with a single tool whose `input_schema` matches the
//! requested JSON schema. The LLM calls the tool, we parse `tool_use.input`.
//! Supports `cache_control: ephemeral` on the `cacheable_prelude` so large
//! repeated preludes reuse at ~90% discount across a batch.

use crate::config::LlmConfig;
use crate::error::{LlmError, LlmResult};
use crate::types::{
    CompletionRequest, CompletionResponse, LlmCapabilities, LlmProvider, TokenUsage,
};
use async_trait::async_trait;
use serde_json::json;
use std::time::Instant;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn from_config(cfg: &LlmConfig) -> LlmResult<Self> {
        let api_key = cfg
            .api_key
            .clone()
            .ok_or_else(|| LlmError::Config("anthropic provider requires `api_key`".into()))?;
        Ok(Self {
            api_key,
            model: cfg.model.clone(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .map_err(|e| LlmError::Config(format!("reqwest client: {}", e)))?,
        })
    }

    pub(crate) fn build_body(&self, req: &CompletionRequest) -> serde_json::Value {
        let tool = json!({
            "name": req.schema_name,
            "description": "Record the structured generation output.",
            "input_schema": req.schema,
        });
        let mut system_blocks: Vec<serde_json::Value> = Vec::new();
        if let Some(prelude) = &req.cacheable_prelude {
            system_blocks.push(json!({
                "type": "text",
                "text": prelude,
                "cache_control": { "type": "ephemeral" }
            }));
        }
        system_blocks.push(json!({
            "type": "text",
            "text": req.system,
        }));
        json!({
            "model": self.model,
            "max_tokens": req.max_output_tokens,
            "temperature": req.temperature,
            "system": system_blocks,
            "tools": [tool],
            "tool_choice": { "type": "tool", "name": req.schema_name },
            "messages": [
                { "role": "user", "content": req.user }
            ]
        })
    }

    pub(crate) fn parse_body(
        &self,
        body: &serde_json::Value,
        raw: &str,
        elapsed_ms: u64,
    ) -> LlmResult<CompletionResponse> {
        let content = body
            .get("content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                LlmError::Llm(format!("anthropic response missing `content` array: {}", raw))
            })?;
        let tool_use = content
            .iter()
            .find(|blk| blk.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
            .ok_or_else(|| LlmError::Llm(format!("no tool_use block in response: {}", raw)))?;
        let input = tool_use
            .get("input")
            .cloned()
            .ok_or_else(|| LlmError::Llm("tool_use block missing input".into()))?;
        let usage = body.get("usage");
        let tok = TokenUsage {
            input_tokens: usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: usage
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_read_tokens: usage
                .and_then(|u| u.get("cache_read_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_write_tokens: usage
                .and_then(|u| u.get("cache_creation_input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        };
        Ok(CompletionResponse {
            json: input,
            raw: raw.to_string(),
            usage: tok,
            provider: "anthropic".into(),
            model: self.model.clone(),
            duration_ms: elapsed_ms,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities {
            supports_structured_output: true,
            supports_prompt_caching: true,
            max_context_window: 200_000,
            default_max_output_tokens: 8_192,
        }
    }

    async fn complete(&self, req: &CompletionRequest) -> LlmResult<CompletionResponse> {
        let body = self.build_body(req);
        let start = Instant::now();
        let resp = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Http {
                url: API_URL.into(),
                message: e.to_string(),
            })?;
        let status = resp.status();
        let raw = resp.text().await.map_err(|e| LlmError::Http {
            url: API_URL.into(),
            message: e.to_string(),
        })?;
        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(LlmError::Llm(format!("anthropic auth failure: {}", raw)));
            }
            if status.as_u16() == 429 {
                return Err(LlmError::Llm(format!("anthropic rate limit: {}", raw)));
            }
            return Err(LlmError::Llm(format!("anthropic HTTP {}: {}", status, raw)));
        }
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| LlmError::Llm(format!("anthropic JSON parse: {}", e)))?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        self.parse_body(&parsed, &raw, elapsed_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmProviderKind;

    fn provider() -> AnthropicProvider {
        let cfg = LlmConfig {
            provider: LlmProviderKind::Anthropic,
            model: "claude-opus-4-7".into(),
            api_key: Some("sk-test".into()),
            base_url: None,
        };
        AnthropicProvider::from_config(&cfg).unwrap()
    }

    fn sample_req() -> CompletionRequest {
        CompletionRequest {
            system: "You are a generator.".into(),
            user: "POST /pet".into(),
            cacheable_prelude: Some("Grounding frames follow: ... (long)".into()),
            schema: json!({ "type": "object", "properties": { "spec": { "type": "object" } } }),
            schema_name: "record_generation".into(),
            max_output_tokens: 4000,
            temperature: 0.2,
        }
    }

    #[test]
    fn body_includes_model_and_tools() {
        let p = provider();
        let body = p.build_body(&sample_req());
        assert_eq!(body["model"], "claude-opus-4-7");
        assert_eq!(body["max_tokens"], 4000);
        assert_eq!(body["tools"][0]["name"], "record_generation");
        assert_eq!(body["tool_choice"]["name"], "record_generation");
    }

    #[test]
    fn cacheable_prelude_marked_ephemeral() {
        let p = provider();
        let body = p.build_body(&sample_req());
        let system = body["system"].as_array().unwrap();
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
        assert!(system[0]["text"].as_str().unwrap().contains("Grounding"));
        assert!(system[1].get("cache_control").is_none());
    }

    #[test]
    fn no_prelude_means_single_system_block() {
        let mut req = sample_req();
        req.cacheable_prelude = None;
        let p = provider();
        let body = p.build_body(&req);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
    }

    #[test]
    fn parse_body_extracts_tool_input() {
        let p = provider();
        let raw = r#"{
          "content": [
            { "type": "tool_use", "id": "x", "name": "record_generation",
              "input": { "spec": { "overview": "Create account" } } }
          ],
          "usage": { "input_tokens": 120, "output_tokens": 45,
                     "cache_read_input_tokens": 800, "cache_creation_input_tokens": 0 }
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
        let resp = p.parse_body(&parsed, raw, 500).unwrap();
        assert_eq!(resp.json["spec"]["overview"], "Create account");
        assert_eq!(resp.usage.input_tokens, 120);
        assert_eq!(resp.usage.cache_read_tokens, 800);
        assert!((resp.usage.cache_hit_ratio() - (800.0 / 920.0)).abs() < 1e-3);
    }

    #[test]
    fn parse_body_errors_when_no_tool_use() {
        let p = provider();
        let raw = r#"{ "content": [{ "type": "text", "text": "I refused to use the tool" }] }"#;
        let parsed: serde_json::Value = serde_json::from_str(raw).unwrap();
        let err = p.parse_body(&parsed, raw, 10).unwrap_err();
        assert!(matches!(err, LlmError::Llm(_)));
    }

    #[test]
    fn capabilities_advertise_cache_and_200k_window() {
        let p = provider();
        let caps = p.capabilities();
        assert!(caps.supports_prompt_caching);
        assert!(caps.supports_structured_output);
        assert_eq!(caps.max_context_window, 200_000);
    }
}
