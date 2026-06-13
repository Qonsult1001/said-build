//! MCP prompt-spec adapter.
//!
//! The MCP protocol exposes prompts as a discoverable resource:
//!
//!   - `prompts/list`  → returns a list of `{name, description, arguments}`
//!   - `prompts/get`   → returns assembled prompt content for one name
//!
//! Spec: https://modelcontextprotocol.io/specification/server/prompts
//!
//! This module returns serializable JSON values matching that spec, so
//! `said-mcp` can wire them into its handler with minimal glue. The
//! prompt content itself comes from [`crate::assemble`] — same source
//! of truth as the WASM and native callers.

use serde::{Deserialize, Serialize};

use crate::{agents, assemble, Role};

/// One entry in `prompts/list`. Spec-compliant shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDescriptor {
    /// Stable identifier the client uses with `prompts/get`. Matches
    /// [`Role::as_str`].
    pub name: String,
    /// Short, human-readable description shown in client UIs.
    pub description: String,
    /// Optional arguments the client may pass to influence assembly.
    /// Currently empty for the answerer (context comes from server
    /// state). Future roles may declare arguments here (e.g.
    /// `thoroughness`).
    pub arguments: Vec<PromptArgument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// One message in the assembled prompt. MCP's `prompts/get` returns a
/// list of these in `{ messages: [...] }`. Today the answerer is a
/// single system message; richer roles can return multi-turn skeletons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: PromptMessageRole,
    pub content: PromptMessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptMessageRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PromptMessageContent {
    Text { text: String },
}

/// Implements the MCP `prompts/list` operation: every role this crate
/// can assemble.
pub fn list_prompts() -> Vec<PromptDescriptor> {
    vec![PromptDescriptor {
        name: Role::Answerer.as_str().to_string(),
        description: "Default .said agent — reads brain content and answers with citations. \
                      Aligned with Anthropic Claude Code production prompts."
            .to_string(),
        arguments: Vec::new(),
    }]
}

/// Implements the MCP `prompts/get` operation. Returns `None` if `name`
/// is unknown so the caller can return the spec's error.
pub fn get_prompt(name: &str, ctx: &agents::answerer::Context) -> Option<Vec<PromptMessage>> {
    let role = Role::from_str(name)?;
    Some(vec![PromptMessage {
        role: PromptMessageRole::System,
        content: PromptMessageContent::Text {
            text: assemble(role, ctx),
        },
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_includes_answerer() {
        let list = list_prompts();
        assert!(list.iter().any(|p| p.name == "answerer"));
    }

    #[test]
    fn get_answerer_returns_system_message() {
        let ctx = agents::answerer::Context::new();
        let msgs = get_prompt("answerer", &ctx).expect("answerer should exist");
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0].role, PromptMessageRole::System));
        let PromptMessageContent::Text { text } = &msgs[0].content;
        assert!(text.contains("answering agent"));
    }

    #[test]
    fn get_unknown_returns_none() {
        let ctx = agents::answerer::Context::new();
        assert!(get_prompt("nonexistent", &ctx).is_none());
    }
}
