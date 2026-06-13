//! `LlmError` — the error type for BYO-LLM operations.
//!
//! Designed to be classifiable for retry / circuit-breaker logic without
//! requiring callers to pattern-match on stringly-typed messages.

use thiserror::Error;

pub type LlmResult<T> = Result<T, LlmError>;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("config: {0}")]
    Config(String),

    #[error("http call to {url} failed: {message}")]
    Http { url: String, message: String },

    #[error("llm: {0}")]
    Llm(String),

    #[error("context exceeded for {slug}: estimated {estimated_tokens} tokens")]
    ContextExceeded { slug: String, estimated_tokens: u32 },
}

/// Classification of failures for retry / circuit-breaker logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmFailureClass {
    Network,
    RateLimit,
    Auth,
    ContextExceeded,
    Parse,
    Other,
}
