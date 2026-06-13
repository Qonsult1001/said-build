//! `LlmConfig` — provider-agnostic configuration for the BYO-LLM trait.
//!
//! Standalone struct so any caller (forge, the LongMemEval harness, any
//! future LLM-using crate) can build an `LlmConfig` without depending on
//! `said-forge`. Forge re-exports this struct so existing call-sites keep
//! working unchanged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LlmProviderKind {
    Anthropic,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    /// Use the locally-installed Claude Code CLI instead of an API key.
    /// Inherits the user's authenticated session — no `api_key` required.
    #[serde(rename = "claude-cli")]
    ClaudeCli,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: LlmProviderKind,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

impl LlmConfig {
    pub fn provider_name(&self) -> &'static str {
        match self.provider {
            LlmProviderKind::Anthropic => "anthropic",
            LlmProviderKind::OpenAiCompatible => "openai-compatible",
            LlmProviderKind::ClaudeCli => "claude-cli",
        }
    }

    /// Convenience: build an Anthropic config from the standard env var.
    /// Returns `None` if `ANTHROPIC_API_KEY` is unset.
    pub fn anthropic_from_env(model: &str) -> Option<Self> {
        std::env::var("ANTHROPIC_API_KEY").ok().map(|api_key| Self {
            provider: LlmProviderKind::Anthropic,
            model: model.to_string(),
            api_key: Some(api_key),
            base_url: None,
        })
    }

    /// Convenience: build an OpenAI-compatible config from the standard env vars.
    /// Returns `None` if `OPENAI_API_KEY` (or compatible) is unset.
    pub fn openai_from_env(model: &str, base_url: &str) -> Option<Self> {
        std::env::var("OPENAI_API_KEY").ok().map(|api_key| Self {
            provider: LlmProviderKind::OpenAiCompatible,
            model: model.to_string(),
            api_key: Some(api_key),
            base_url: Some(base_url.to_string()),
        })
    }

    /// Convenience: build a Groq config (OpenAI-compatible endpoint).
    /// Reads `GROQ_API_KEY` from env. Default base_url is the Groq cloud
    /// endpoint, but can be overridden via `GROQ_BASE_URL` for self-hosted.
    pub fn groq_from_env(model: &str) -> Option<Self> {
        std::env::var("GROQ_API_KEY").ok().map(|api_key| Self {
            provider: LlmProviderKind::OpenAiCompatible,
            model: model.to_string(),
            api_key: Some(api_key),
            base_url: Some(
                std::env::var("GROQ_BASE_URL")
                    .unwrap_or_else(|_| "https://api.groq.com/openai/v1".into()),
            ),
        })
    }

    /// Convenience: build a Claude Code CLI config. No API key required —
    /// uses the local `claude` binary which inherits the user's session auth.
    /// `model` may be empty to use the CLI's default model selection.
    pub fn claude_cli(model: &str) -> Self {
        Self {
            provider: LlmProviderKind::ClaudeCli,
            model: model.to_string(),
            api_key: None,
            base_url: None,
        }
    }
}
