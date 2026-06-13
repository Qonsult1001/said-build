//! `LlmProvider` trait + request/response types.
//!
//! The trait is the entire API surface a caller sees. Providers implement it;
//! callers only see `dyn LlmProvider`.

use crate::error::LlmResult;
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
    /// Long, repeated context that should be cached across requests when the
    /// provider supports prompt caching (Anthropic).
    pub cacheable_prelude: Option<String>,
    /// JSON schema the response must match. Providers enforce this via tools
    /// (Anthropic) or `response_format: json_schema` (OpenAI-compat).
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
        if total == 0 {
            0.0
        } else {
            self.cache_read_tokens as f32 / total as f32
        }
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> LlmCapabilities;
    async fn complete(&self, req: &CompletionRequest) -> LlmResult<CompletionResponse>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_ratio_math() {
        let u = TokenUsage {
            input_tokens: 100,
            cache_read_tokens: 900,
            output_tokens: 0,
            cache_write_tokens: 0,
        };
        assert!((u.cache_hit_ratio() - 0.9).abs() < 1e-4);

        let empty = TokenUsage::default();
        assert_eq!(empty.cache_hit_ratio(), 0.0);
    }
}
