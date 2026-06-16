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
        // Permissive json_object mode (no strict schema) vs strict json_schema.
        // json_object works across Groq models for free-form JSON-shaped output
        // (e.g. a coding change-set with arbitrary `content`); json_schema stays
        // the default for exact-shape extraction (forge). Proven in Advisory's
        // GroqCycle (JsonObject:true).
        let response_format = if req.json_object {
            json!({ "type": "json_object" })
        } else {
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": req.schema_name,
                    "strict": true,
                    "schema": req.schema,
                }
            })
        };
        // Provider-correct fields (researched): Groq and OpenRouter differ.
        //   - Groq: token field is `max_completion_tokens`; reasoning is the flat
        //     `reasoning_effort` string ("low"|"medium"|"high"). `max_tokens` and
        //     a `reasoning` object are REJECTED (400).
        //   - OpenRouter: token field is `max_tokens`; reasoning is the
        //     `reasoning: {enabled|effort}` object; supports `provider` pinning.
        let is_groq = self.base_url.contains("groq");
        let is_openrouter = self.base_url.contains("openrouter");
        let effort = std::env::var("SAID_LLM_REASONING_EFFORT").unwrap_or_else(|_| "medium".into());
        // "off"/"none"/"instant" disables reasoning (instant mode) — much faster
        // on thinking models like k2.5 (thinking is the latency).
        let reasoning_on = !matches!(effort.as_str(), "off" | "none" | "instant" | "");

        // Temperature: k2.5 THINKING mode wants 1.0 (official rec); 0.2 is "too
        // conservative, lower quality". SAID_LLM_TEMPERATURE overrides per run.
        let temperature = std::env::var("SAID_LLM_TEMPERATURE").ok()
            .and_then(|s| s.parse::<f32>().ok()).unwrap_or(req.temperature);
        let mut body = json!({
            "model": self.model,
            "temperature": temperature,
            "response_format": response_format,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user",   "content": req.user }
            ]
        });
        if is_groq {
            body["max_completion_tokens"] = json!(req.max_output_tokens);
            if reasoning_on { body["reasoning_effort"] = json!(effort); }
        } else {
            body["max_tokens"] = json!(req.max_output_tokens);
        }
        if is_openrouter {
            if reasoning_on {
                body["reasoning"] = json!({ "effort": effort });
            } else {
                // Instant mode: explicitly disable reasoning.
                body["reasoning"] = json!({ "enabled": false });
            }
            if let Ok(order) = std::env::var("SAID_LLM_PROVIDER_ORDER") {
                let providers: Vec<&str> = order.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                if !providers.is_empty() {
                    body["provider"] = json!({ "order": providers, "allow_fallbacks": true });
                }
            }
        }
        body
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
        // Reasoning models (Kimi K2.x, etc.) can leave `content` null — they put
        // text in `reasoning` and, if max_tokens is exhausted on reasoning, never
        // emit content. Fall back to `reasoning` so we still get the answer; only
        // error if BOTH are empty (a genuinely empty completion, e.g. length cap).
        let content = message
            .get("content")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| message.get("reasoning").and_then(|v| v.as_str()))
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                let fr = body.get("choices").and_then(|c| c.get(0))
                    .and_then(|c| c.get("finish_reason")).and_then(|v| v.as_str()).unwrap_or("?");
                LlmError::Llm(format!("empty completion (no content/reasoning; finish_reason={})", fr))
            })?;
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
        // Monitoring (SAID_LLM_DEBUG): print what we're sending + live timing so a
        // slow/hanging call is immediately visible (model, provider pin, effort).
        let dbg = std::env::var("SAID_LLM_DEBUG").is_ok();
        if dbg {
            let prov = body.get("provider").and_then(|p| p.get("order"))
                .map(|o| o.to_string()).unwrap_or_else(|| "default".into());
            eprintln!("[llm] -> POST {} model={} provider={} effort={} temp={} max={}",
                url, self.model, prov,
                body.get("reasoning").and_then(|r| r.get("effort")).and_then(|v| v.as_str())
                    .or(body.get("reasoning_effort").and_then(|v| v.as_str())).unwrap_or("-"),
                req.temperature, req.max_output_tokens);
        }
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
        if dbg {
            eprintln!("[llm] <- {} in {:.1}s ({} bytes)", status, start.elapsed().as_secs_f32(), raw.len());
        }
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
            json_object: false,
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
    fn body_uses_json_object_when_flagged() {
        let p = provider();
        let mut req = sample_req();
        req.json_object = true;
        let body = p.build_body(&req);
        // Permissive json_object mode — no strict schema (the Groq-compatible path).
        assert_eq!(body["response_format"]["type"], "json_object");
        assert!(body["response_format"].get("json_schema").is_none());
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
