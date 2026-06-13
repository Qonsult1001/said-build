//! `LlmProvider` trait + provider impls.
//!
//! Per spec §2.2 A-Q6 and §9.3. BYO-LLM architecture:
//! - `AnthropicProvider` uses the Messages API with tools for structured output + prompt caching.
//! - `OpenAICompatibleProvider` uses Chat Completions with `response_format: json_schema`.
//! - `StubProvider` is test-only — returns canned responses.

pub mod anthropic;
pub mod openai_compat;
#[cfg(any(test, feature = "stub-llm"))]
pub mod stub;

use crate::ForgeResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Capability matrix advertised by each provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct LlmCapabilities {
    pub supports_structured_output: bool,
    pub supports_prompt_caching: bool,
    pub max_context_window: u32,
    pub default_max_output_tokens: u32,
}

/// One completion request.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system: String,
    pub user: String,
    pub cacheable_prelude: Option<String>,
    pub schema: serde_json::Value,
    pub schema_name: String,
    pub max_output_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub json: serde_json::Value,
    pub raw: String,
    pub usage: TokenUsage,
    pub provider: String,
    pub model: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
}

impl TokenUsage {
    pub fn cache_hit_ratio(&self) -> f32 {
        let total = self.input_tokens + self.cache_read_tokens;
        if total == 0 { 0.0 } else { self.cache_read_tokens as f32 / total as f32 }
    }
}

/// Classification of failures for retry/circuit-breaker logic (spec §13.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmFailureClass {
    Network,
    RateLimit,
    Auth,
    ContextExceeded,
    Parse,
    Other,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> LlmCapabilities;
    async fn complete(&self, req: &CompletionRequest) -> ForgeResult<CompletionResponse>;
}

/// Factory: build the configured provider from `ForgeConfig`.
pub fn provider_from_config(cfg: &crate::ForgeConfig) -> ForgeResult<Box<dyn LlmProvider>> {
    match cfg.llm.provider {
        crate::LlmProviderKind::Anthropic => Ok(Box::new(anthropic::AnthropicProvider::from_config(cfg)?)),
        crate::LlmProviderKind::OpenAiCompatible => Ok(Box::new(openai_compat::OpenAICompatibleProvider::from_config(cfg)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_ratio_math() {
        let u = TokenUsage {
            input_tokens: 100, cache_read_tokens: 900,
            output_tokens: 0, cache_write_tokens: 0,
        };
        assert!((u.cache_hit_ratio() - 0.9).abs() < 1e-4);

        let empty = TokenUsage::default();
        assert_eq!(empty.cache_hit_ratio(), 0.0);
    }
}

// `CompletionRequest` derives Clone at the struct definition.
