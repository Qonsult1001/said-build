//! Provider implementations for the `LlmProvider` trait.

pub mod anthropic;
pub mod claude_cli;
pub mod openai_compat;

#[cfg(any(test, feature = "stub-llm"))]
pub mod stub;
