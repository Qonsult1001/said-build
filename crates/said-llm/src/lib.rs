//! `said-llm` — workspace-shared BYO-LLM trait + providers.
//!
//! One trait (`LlmProvider`) + two real providers (`AnthropicProvider`,
//! `OpenAICompatibleProvider`) + one test stub (`StubProvider`). Used by
//! every SAID component that needs to call out to a customer-configured LLM
//! — `said-forge`, the LongMemEval harness, and the future `said-think`
//! agent.
//!
//! Architecture rule: **`sca-core` and `said-mcp` MUST NOT depend on this
//! crate.** The BYO-LLM contract is that the core retrieval engine and the
//! MCP server are LLM-free; only opt-in components reach for `said-llm`.

pub mod config;
pub mod error;
pub mod providers;
pub mod types;

pub use config::{LlmConfig, LlmProviderKind};
pub use error::{LlmError, LlmFailureClass, LlmResult};
pub use providers::{
    anthropic::AnthropicProvider,
    claude_cli::ClaudeCodeCliProvider,
    openai_compat::OpenAICompatibleProvider,
};
pub use types::{
    CompletionRequest, CompletionResponse, LlmCapabilities, LlmProvider, TokenUsage,
};

#[cfg(any(test, feature = "stub-llm"))]
pub use providers::stub::StubProvider;

/// Factory: build the configured provider from `LlmConfig`.
pub fn provider_from_config(cfg: &LlmConfig) -> LlmResult<Box<dyn LlmProvider>> {
    match cfg.provider {
        LlmProviderKind::Anthropic => Ok(Box::new(AnthropicProvider::from_config(cfg)?)),
        LlmProviderKind::OpenAiCompatible => {
            Ok(Box::new(OpenAICompatibleProvider::from_config(cfg)?))
        }
        LlmProviderKind::ClaudeCli => Ok(Box::new(ClaudeCodeCliProvider::from_config(cfg)?)),
    }
}
