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
        // STRUCTURED + UNSTRUCTURED "just works" (global tool standard): the model
        // may return clean JSON (json_object/json_schema mode) OR prose (a weaker
        // model, or our format-rejection fallback that dropped response_format).
        // Resolve to a usable JSON value either way instead of hard-erroring:
        //   1. whole content parses as JSON  -> use it
        //   2. a JSON object/array is embedded in prose -> extract it
        //   3. otherwise -> wrap the prose as {"output": "<text>"} so prose phases
        //      (plan/design/test) and any string-output caller still get their answer.
        let parsed_json: serde_json::Value = serde_json::from_str(content)
            .ok()
            .or_else(|| extract_embedded_json(content))
            .unwrap_or_else(|| serde_json::json!({ "output": content }));
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
        // RATE-LIMIT BACKOFF: rate-limited tiers (Groq gpt-oss-20b: 150K TPM / 500 RPM)
        // 429 mid-run when orchestrator phases (plan/design/code/repair) fire in quick
        // succession. A 429 is TRANSIENT — the budget refills on a sliding window — so
        // sleep the server-advised `retry-after` (or exponential fallback) and retry
        // instead of failing the whole run. Tunable: SAID_LLM_RATE_RETRIES (default 5),
        // SAID_LLM_RATE_MAX_WAIT seconds cap per sleep (default 65, just over a minute
        // so a full TPM window can refill).
        let max_retries: u32 = std::env::var("SAID_LLM_RATE_RETRIES")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(5);
        let max_wait: f64 = std::env::var("SAID_LLM_RATE_MAX_WAIT")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(65.0);
        let mut attempt: u32 = 0;
        loop {
            match self.complete_once(req).await {
                Err(LlmError::Llm(msg)) if is_rate_limit(&msg) && attempt < max_retries => {
                    // Honor server `retry_after=<secs>` if present; else exponential
                    // (2s, 4s, 8s, 16s, 32s) — all clamped to max_wait.
                    let advised = parse_retry_after(&msg);
                    let backoff = advised.unwrap_or((2u64.pow(attempt + 1)) as f64).min(max_wait);
                    if std::env::var("SAID_LLM_DEBUG").is_ok() {
                        eprintln!("[llm] rate limit (attempt {}/{}) -> sleeping {:.1}s",
                            attempt + 1, max_retries, backoff);
                    }
                    tokio::time::sleep(std::time::Duration::from_secs_f64(backoff)).await;
                    attempt += 1;
                    continue;
                }
                other => return other,
            }
        }
    }
}

impl OpenAICompatibleProvider {
    /// One full completion attempt (build body + post + format-rejection fallback).
    /// `complete()` wraps this in a rate-limit backoff loop.
    async fn complete_once(&self, req: &CompletionRequest) -> LlmResult<CompletionResponse> {
        let body = self.build_body(req);
        match self.post_once(req, body.clone()).await {
            Ok(r) => Ok(r),
            // ROBUSTNESS for weaker models: smaller models (e.g. gpt-oss-20b) don't
            // reliably honor server-side `response_format` JSON validation OR they
            // emit an unsolicited tool call — Groq 400s with `json_validate_failed` /
            // `json_generate` / `tool_use_failed` BEFORE returning content, so the
            // null-content fallback can't help. Retry ONCE without `response_format`
            // (plain text): prose phases ARE the answer, and change-set phases extract
            // JSON from prose downstream. The 120b never needs this; the 20b does.
            Err(LlmError::Llm(msg)) if is_format_rejection(&msg) => {
                let mut relaxed = body;
                if let Some(obj) = relaxed.as_object_mut() {
                    obj.remove("response_format");
                    obj.remove("tools");
                    obj.remove("tool_choice");
                }
                if std::env::var("SAID_LLM_DEBUG").is_ok() {
                    eprintln!("[llm] format-rejection -> retrying without response_format (plain text)");
                }
                self.post_once(req, relaxed).await
            }
            Err(e) => Err(e),
        }
    }
}

/// Find the first balanced JSON object/array embedded in arbitrary text (e.g. a model
/// that wrapped its JSON in prose or ```json fences). Returns None if none parses.
fn extract_embedded_json(s: &str) -> Option<serde_json::Value> {
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'{' && b != b'[' { continue; }
        let close = if b == b'{' { b'}' } else { b']' };
        let (mut depth, mut in_str, mut esc) = (0i32, false, false);
        for (j, &c) in bytes.iter().enumerate().skip(i) {
            if in_str {
                if esc { esc = false; }
                else if c == b'\\' { esc = true; }
                else if c == b'"' { in_str = false; }
            } else if c == b'"' { in_str = true; }
            else if c == b { depth += 1; }
            else if c == close {
                depth -= 1;
                if depth == 0 {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s[i..=j]) {
                        return Some(v);
                    }
                    break;
                }
            }
        }
    }
    None
}

/// True if a Groq/OpenAI 400 is about JSON-mode / tool-call formatting (recoverable
/// by retrying as plain text), not a genuine bad request.
/// A 429 surfaced by post_once carries the literal "rate limit" marker.
fn is_rate_limit(msg: &str) -> bool {
    msg.contains("rate limit")
}

/// Extract the server-advised retry window (seconds) we tagged onto the 429 message
/// as `retry_after=<secs>`. Returns None if absent (caller falls back to exponential).
fn parse_retry_after(msg: &str) -> Option<f64> {
    let i = msg.find("retry_after=")? + "retry_after=".len();
    let tail = &msg[i..];
    let end = tail.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(tail.len());
    tail[..end].parse::<f64>().ok()
}

fn is_format_rejection(msg: &str) -> bool {
    msg.contains("json_validate_failed")
        || msg.contains("json_generate")
        || msg.contains("Failed to generate JSON")
        || msg.contains("Failed to validate JSON")
        || msg.contains("tool_use_failed")
}

impl OpenAICompatibleProvider {
    /// One POST + parse. Factored out so `complete` can retry with a relaxed body.
    async fn post_once(&self, req: &CompletionRequest, body: serde_json::Value) -> LlmResult<CompletionResponse> {
        let url = format!("{}/chat/completions", self.base_url);
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
            .map_err(|e| LlmError::Http { url: url.clone(), message: e.to_string() })?;
        let status = resp.status();
        // Groq/OpenRouter return `retry-after` (seconds) on 429. Capture BEFORE the
        // body consumes the response so complete()'s backoff loop can honor it.
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<f64>().ok());
        let raw = resp.text().await.map_err(|e| LlmError::Http { url: url.clone(), message: e.to_string() })?;
        if dbg {
            eprintln!("[llm] <- {} in {:.1}s ({} bytes)", status, start.elapsed().as_secs_f32(), raw.len());
        }
        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(LlmError::Llm(format!("openai-compat auth failure: {}", raw)));
            }
            if status.as_u16() == 429 {
                // Tag with the parseable retry-after (seconds) so complete()'s backoff
                // loop can sleep the exact server-advised window. Format: the message
                // still contains "rate limit" + the raw body for diagnostics.
                let hint = retry_after.map(|s| format!(" retry_after={:.3}", s)).unwrap_or_default();
                return Err(LlmError::Llm(format!("openai-compat rate limit{}: {}", hint, raw)));
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

    #[test]
    fn extracts_or_wraps_json() {
        // clean json
        assert_eq!(extract_embedded_json(r#"{"a":1}"#).unwrap()["a"], 1);
        // json embedded in prose / fences
        let v = extract_embedded_json("Here you go:\n```json\n{\"edits\":[1,2]}\n```\nDone").unwrap();
        assert!(v["edits"].is_array());
        // no json present -> None (caller wraps as {"output": prose})
        assert!(extract_embedded_json("just prose, no json here").is_none());
    }

    #[test]
    fn detects_format_rejections_only() {
        assert!(is_format_rejection("openai-compat HTTP 400: {\"code\":\"json_validate_failed\"}"));
        assert!(is_format_rejection("...Failed to generate JSON..."));
        assert!(is_format_rejection("...code\":\"tool_use_failed\"..."));
        // genuine bad requests must NOT be treated as recoverable
        assert!(!is_format_rejection("openai-compat HTTP 400: invalid model"));
        assert!(!is_format_rejection("openai-compat rate limit"));
    }

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
