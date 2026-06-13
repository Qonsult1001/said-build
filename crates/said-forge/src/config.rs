//! `ForgeConfig` — parses the `[forge]`, `[forge.llm]`, `[forge.grounding]`,
//! `[forge.markdown]`, `[forge.costs]` sections from `~/.said/config.toml`
//! or project-local `.said/config.toml`. Env vars override TOML.
//!
//! Per spec §12.

use crate::{ForgeError, ForgeResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LlmProviderKind {
    Anthropic,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    forge: Option<RawForge>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawForge {
    default_editor: Option<String>,
    concurrency: Option<u32>,
    halt_after: Option<u32>,
    llm: Option<RawLlm>,
    grounding: Option<RawGrounding>,
    markdown: Option<RawMarkdown>,
    costs: Option<RawCosts>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLlm {
    provider: String,
    model: String,
    api_key: Option<String>,
    base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGrounding {
    max_frames: Option<u32>,
    per_pillar_cap: Option<u32>,
    inline_top_n: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMarkdown {
    heading_level: Option<u8>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCosts {
    input_usd_per_mtok: Option<f64>,
    output_usd_per_mtok: Option<f64>,
    cached_usd_per_mtok: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ForgeConfig {
    pub default_editor: String,
    pub concurrency: u32,
    pub halt_after: u32,
    pub llm: LlmConfig,
    pub grounding: GroundingConfig,
    pub markdown: MarkdownConfig,
    pub costs: CostsConfig,
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
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroundingConfig {
    pub max_frames: u32,
    pub per_pillar_cap: u32,
    pub inline_top_n: u32,
}

#[derive(Debug, Clone)]
pub struct MarkdownConfig {
    pub heading_level: u8,
}

#[derive(Debug, Clone)]
pub struct CostsConfig {
    pub input_usd_per_mtok: f64,
    pub output_usd_per_mtok: f64,
    pub cached_usd_per_mtok: f64,
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self {
            default_editor: "claude".into(),
            concurrency: 4,
            halt_after: 5,
            llm: LlmConfig {
                provider: LlmProviderKind::Anthropic,
                model: "claude-opus-4-7".into(),
                api_key: None,
                base_url: None,
            },
            grounding: GroundingConfig {
                max_frames: 24,
                per_pillar_cap: 8,
                inline_top_n: 8,
            },
            markdown: MarkdownConfig { heading_level: 2 },
            costs: CostsConfig {
                input_usd_per_mtok: 15.0,
                output_usd_per_mtok: 75.0,
                cached_usd_per_mtok: 1.5,
            },
        }
    }
}

impl ForgeConfig {
    pub fn from_toml_str(s: &str) -> ForgeResult<Self> {
        let raw: RawConfig =
            toml::from_str(s).map_err(|e| ForgeError::Config(format!("toml parse: {}", e)))?;
        let mut cfg = ForgeConfig::default();
        let Some(f) = raw.forge else {
            return Ok(cfg);
        };
        if let Some(v) = f.default_editor {
            cfg.default_editor = v;
        }
        if let Some(v) = f.concurrency {
            cfg.concurrency = v;
        }
        if let Some(v) = f.halt_after {
            cfg.halt_after = v;
        }
        if let Some(llm) = f.llm {
            cfg.llm.provider = parse_provider(&llm.provider)?;
            cfg.llm.model = llm.model;
            cfg.llm.api_key = llm.api_key.map(expand_env_placeholder);
            cfg.llm.base_url = llm.base_url;
            if cfg.llm.provider == LlmProviderKind::OpenAiCompatible && cfg.llm.base_url.is_none() {
                return Err(ForgeError::Config(
                    "openai-compatible provider requires base_url".into(),
                ));
            }
        }
        if let Some(g) = f.grounding {
            if let Some(v) = g.max_frames {
                cfg.grounding.max_frames = v;
            }
            if let Some(v) = g.per_pillar_cap {
                cfg.grounding.per_pillar_cap = v;
            }
            if let Some(v) = g.inline_top_n {
                cfg.grounding.inline_top_n = v;
            }
        }
        if let Some(m) = f.markdown {
            if let Some(v) = m.heading_level {
                cfg.markdown.heading_level = v;
            }
        }
        if let Some(c) = f.costs {
            if let Some(v) = c.input_usd_per_mtok {
                cfg.costs.input_usd_per_mtok = v;
            }
            if let Some(v) = c.output_usd_per_mtok {
                cfg.costs.output_usd_per_mtok = v;
            }
            if let Some(v) = c.cached_usd_per_mtok {
                cfg.costs.cached_usd_per_mtok = v;
            }
        }
        Ok(cfg)
    }

    pub fn from_file(path: &Path) -> ForgeResult<Self> {
        let s = std::fs::read_to_string(path).map_err(|cause| ForgeError::Io {
            path: path.display().to_string(),
            cause,
        })?;
        Self::from_toml_str(&s)
    }

    pub fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("SAID_FORGE_CONCURRENCY") {
            if let Ok(n) = v.parse::<u32>() {
                self.concurrency = n;
            }
        }
        if let Ok(v) = std::env::var("SAID_FORGE_HALT_AFTER") {
            if let Ok(n) = v.parse::<u32>() {
                self.halt_after = n;
            }
        }
        if let Ok(v) = std::env::var("SAID_FORGE_PROVIDER") {
            if let Ok(p) = parse_provider(&v) {
                self.llm.provider = p;
            }
        }
        if let Ok(v) = std::env::var("SAID_FORGE_MODEL") {
            self.llm.model = v;
        }
        if let Ok(v) = std::env::var("SAID_FORGE_API_KEY") {
            self.llm.api_key = Some(v);
        }
        if let Ok(v) = std::env::var("SAID_FORGE_BASE_URL") {
            self.llm.base_url = Some(v);
        }
    }
}

fn parse_provider(s: &str) -> ForgeResult<LlmProviderKind> {
    match s {
        "anthropic" => Ok(LlmProviderKind::Anthropic),
        "openai-compatible" => Ok(LlmProviderKind::OpenAiCompatible),
        other => Err(ForgeError::Config(format!(
            "unknown provider '{}' — expected 'anthropic' or 'openai-compatible'",
            other
        ))),
    }
}

/// `"${VAR}"` → `env::var("VAR")`; anything else passed through.
fn expand_env_placeholder(v: String) -> String {
    if let Some(inside) = v.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
        if let Ok(resolved) = std::env::var(inside) {
            return resolved;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_TOML: &str = r#"
[forge]
default_editor = "claude"
concurrency = 8
halt_after = 10

[forge.llm]
provider = "anthropic"
model    = "claude-opus-4-7"
api_key  = "sk-test-123"

[forge.grounding]
max_frames     = 32
per_pillar_cap = 10
inline_top_n   = 12

[forge.markdown]
heading_level = 3

[forge.costs]
input_usd_per_mtok  = 15.0
output_usd_per_mtok = 75.0
cached_usd_per_mtok = 1.5
"#;

    #[test]
    fn parses_complete_toml() {
        let cfg = ForgeConfig::from_toml_str(FULL_TOML).unwrap();
        assert_eq!(cfg.default_editor, "claude");
        assert_eq!(cfg.concurrency, 8);
        assert_eq!(cfg.halt_after, 10);
        assert_eq!(cfg.llm.provider, LlmProviderKind::Anthropic);
        assert_eq!(cfg.llm.model, "claude-opus-4-7");
        assert_eq!(cfg.llm.api_key.as_deref(), Some("sk-test-123"));
        assert_eq!(cfg.grounding.max_frames, 32);
        assert_eq!(cfg.grounding.per_pillar_cap, 10);
        assert_eq!(cfg.grounding.inline_top_n, 12);
        assert_eq!(cfg.markdown.heading_level, 3);
        assert_eq!(cfg.costs.input_usd_per_mtok, 15.0);
    }

    #[test]
    fn defaults_apply_when_toml_is_empty() {
        let cfg = ForgeConfig::from_toml_str("").unwrap();
        assert_eq!(cfg.default_editor, "claude");
        assert_eq!(cfg.concurrency, 4);
        assert_eq!(cfg.halt_after, 5);
        assert_eq!(cfg.grounding.max_frames, 24);
        assert_eq!(cfg.grounding.per_pillar_cap, 8);
        assert_eq!(cfg.grounding.inline_top_n, 8);
        assert_eq!(cfg.markdown.heading_level, 2);
    }

    #[test]
    fn env_overrides_toml() {
        let mut cfg = ForgeConfig::from_toml_str(FULL_TOML).unwrap();
        std::env::set_var("SAID_FORGE_CONCURRENCY", "16");
        std::env::set_var("SAID_FORGE_MODEL", "claude-haiku-4-5-20251001");
        cfg.apply_env();
        std::env::remove_var("SAID_FORGE_CONCURRENCY");
        std::env::remove_var("SAID_FORGE_MODEL");
        assert_eq!(cfg.concurrency, 16);
        assert_eq!(cfg.llm.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn api_key_expands_env_placeholder() {
        std::env::set_var("FORGE_TEST_KEY", "sk-expanded-456");
        let toml = r#"
[forge.llm]
provider = "anthropic"
model = "claude-opus-4-7"
api_key = "${FORGE_TEST_KEY}"
"#;
        let cfg = ForgeConfig::from_toml_str(toml).unwrap();
        std::env::remove_var("FORGE_TEST_KEY");
        assert_eq!(cfg.llm.api_key.as_deref(), Some("sk-expanded-456"));
    }

    #[test]
    fn openai_compat_requires_base_url() {
        let toml = r#"
[forge.llm]
provider = "openai-compatible"
model = "claude-opus-4.7"
api_key = "test"
"#;
        let err = ForgeConfig::from_toml_str(toml).unwrap_err();
        assert!(format!("{}", err).contains("base_url"));
    }

    #[test]
    fn bad_provider_name_errors() {
        let toml = r#"
[forge.llm]
provider = "bedrock"
model = "something"
api_key = "x"
"#;
        let err = ForgeConfig::from_toml_str(toml).unwrap_err();
        assert!(format!("{}", err).contains("provider"));
    }
}
