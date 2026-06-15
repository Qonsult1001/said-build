//! LLM configuration for the orchestrator — the persistent, file-based answer to
//! "which model does .said drive", mirroring forge's `[forge.llm]` TOML and the
//! WASM `llmConfig`. One consistent pattern across the .said stack.
//!
//! Resolution order (first hit wins):
//!   1. An explicit `[llm]` TOML file (path given to `load`)
//!   2. Environment variables (GROQ_API_KEY / OPENAI_API_KEY+SAID_LLM_* / ANTHROPIC_*)
//!
//! Keys are NEVER stored in the file as plaintext when you use a `${ENV}`
//! placeholder for `api_key` — the value is read from the environment at load
//! time (same audit story as forge/WASM: keys live in env/secret store).
//!
//! Example `said-llm.toml`:
//! ```toml
//! [llm]
//! provider = "openai-compatible"
//! model    = "moonshotai/kimi-k2.5"
//! base_url = "https://openrouter.ai/api/v1"
//! api_key  = "${OPENROUTER_API_KEY}"
//! ```

use said_llm::{LlmConfig, LlmProviderKind};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RawFile {
    llm: Option<RawLlm>,
}

#[derive(Debug, Deserialize)]
struct RawLlm {
    provider: String,
    model: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
}

/// Expand a `${ENV_VAR}` placeholder to its env value; pass other strings
/// through unchanged. Keeps secrets out of the config file.
fn expand_env(v: String) -> String {
    if let Some(inside) = v.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
        if let Ok(resolved) = std::env::var(inside) {
            return resolved;
        }
    }
    v
}

fn parse_provider(s: &str) -> Result<LlmProviderKind, String> {
    match s {
        "anthropic" => Ok(LlmProviderKind::Anthropic),
        "openai-compatible" => Ok(LlmProviderKind::OpenAiCompatible),
        "claude-cli" => Ok(LlmProviderKind::ClaudeCli),
        other => Err(format!(
            "unknown provider '{}' — expected anthropic | openai-compatible | claude-cli",
            other
        )),
    }
}

/// Load an `[llm]` config from a TOML file (with `${ENV}` expansion on api_key).
pub fn from_file(path: &str) -> Result<LlmConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path, e))?;
    let raw: RawFile = toml::from_str(&text).map_err(|e| format!("parse {}: {}", path, e))?;
    let llm = raw.llm.ok_or_else(|| format!("{} has no [llm] section", path))?;
    let cfg = LlmConfig {
        provider: parse_provider(&llm.provider)?,
        model: llm.model,
        api_key: llm.api_key.map(expand_env),
        base_url: llm.base_url,
    };
    if cfg.provider == LlmProviderKind::OpenAiCompatible && cfg.base_url.is_none() {
        return Err("openai-compatible provider requires base_url".into());
    }
    Ok(cfg)
}

/// Build an `LlmConfig` from environment variables. Same precedence the binary
/// used before the config file: Groq → OpenAI-compatible → Anthropic.
pub fn from_env() -> Result<LlmConfig, String> {
    if std::env::var("GROQ_API_KEY").is_ok() {
        let model = std::env::var("GROQ_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".into());
        return LlmConfig::groq_from_env(&model).ok_or_else(|| "GROQ_API_KEY set but config failed".into());
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        let base = std::env::var("SAID_LLM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let model = std::env::var("SAID_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        return LlmConfig::openai_from_env(&model, &base).ok_or_else(|| "OPENAI_API_KEY set but config failed".into());
    }
    if let Ok(model) = std::env::var("ANTHROPIC_MODEL") {
        if let Some(c) = LlmConfig::anthropic_from_env(&model) { return Ok(c); }
    }
    Err("no LLM configured: provide a --llm-config TOML with [llm], or set GROQ_API_KEY \
         (+GROQ_MODEL) / OPENAI_API_KEY + SAID_LLM_BASE_URL + SAID_LLM_MODEL / \
         ANTHROPIC_API_KEY + ANTHROPIC_MODEL".into())
}

/// Resolve the LLM config: a TOML file if given, else environment.
pub fn resolve(config_path: Option<&str>) -> Result<LlmConfig, String> {
    match config_path {
        Some(p) => from_file(p),
        None => from_env(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_env_placeholder() {
        std::env::set_var("SAID_TEST_KEY_X", "secret123");
        assert_eq!(expand_env("${SAID_TEST_KEY_X}".into()), "secret123");
        assert_eq!(expand_env("literal".into()), "literal");
    }

    #[test]
    fn parses_openai_compat_toml() {
        let dir = std::env::temp_dir().join(format!("said_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("llm.toml");
        std::fs::write(&f, "[llm]\nprovider=\"openai-compatible\"\nmodel=\"m\"\nbase_url=\"https://x/v1\"\napi_key=\"literalkey\"\n").unwrap();
        let cfg = from_file(f.to_str().unwrap()).unwrap();
        assert_eq!(cfg.provider, LlmProviderKind::OpenAiCompatible);
        assert_eq!(cfg.model, "m");
        assert_eq!(cfg.api_key.as_deref(), Some("literalkey"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
