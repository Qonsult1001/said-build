//! Claude Code CLI provider — uses the local `claude` binary instead of an
//! API key.
//!
//! Why: customers who already have a Claude Code subscription don't need
//! an API key for opt-in LLM features. The CLI inherits the authenticated
//! session.
//!
//! Pattern modeled on [`crates/sca-core/examples/locomo_f1.rs`] which has
//! battle-tested this approach against Claude Code's session-scoped rate
//! limits:
//!
//!   - 200 ms pacing between sequential calls (~5 qps ceiling)
//!   - 3-attempt retry with exponential backoff (1 s, 2 s, 4 s)
//!   - Auto-resolve the CLI binary on Windows + nvm4w
//!
//! The Claude Code CLI does NOT enforce JSON-schema output the way the
//! Anthropic Messages API or OpenAI Chat Completions do, so we run a
//! defensive JSON extractor on the raw stdout (handles fenced code blocks,
//! preamble text, etc.).

use crate::config::LlmConfig;
use crate::error::{LlmError, LlmResult};
use crate::types::{
    CompletionRequest, CompletionResponse, LlmCapabilities, LlmProvider, TokenUsage,
};
use async_trait::async_trait;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Default pacing between sequential calls. Matches the proven 200 ms cadence
/// from locomo_f1.rs ("~5 qps ceiling").
const DEFAULT_PACING_MS: u64 = 200;

/// Default retry policy.
const DEFAULT_MAX_RETRIES: u32 = 3;

pub struct ClaudeCodeCliProvider {
    /// The base command — typically "claude" or
    /// `node /c/.../@anthropic-ai/claude-code/cli.js` on Windows + nvm4w.
    base_cmd: String,
    /// Optional model name passed via `--model` flag.
    model: Option<String>,
    /// Pacing between sequential calls (rate-limit avoidance).
    pacing_ms: u64,
    /// Max retry attempts on transient failures.
    max_retries: u32,
    /// Tracks the timestamp of the last call so we can enforce pacing.
    last_call: Mutex<Option<Instant>>,
}

impl ClaudeCodeCliProvider {
    pub fn from_config(cfg: &LlmConfig) -> LlmResult<Self> {
        // Resolve the CLI binary. `CLAUDE_CMD` env var wins. Otherwise:
        //   - on Windows + nvm4w, fall back to invoking node with the native
        //     Windows path so it resolves under cmd.exe / direct subprocess
        //     spawn. (The bash-style `/c/...` form only works when invoked
        //     from inside bash.)
        //   - otherwise: just `claude` (assumes it's on PATH)
        let base_cmd = std::env::var("CLAUDE_CMD").unwrap_or_else(|_| {
            #[cfg(windows)]
            {
                let win_cli =
                    "C:\\nvm4w\\nodejs\\node_modules\\@anthropic-ai\\claude-code\\cli.js";
                if std::path::Path::new(win_cli).exists() {
                    return format!("node {}", win_cli);
                }
            }
            "claude".to_string()
        });

        // The model field is optional for the CLI provider. If unset, the CLI
        // uses whatever is configured in the user's session.
        let model = if cfg.model.is_empty() {
            None
        } else {
            Some(cfg.model.clone())
        };

        Ok(Self {
            base_cmd,
            model,
            pacing_ms: DEFAULT_PACING_MS,
            max_retries: DEFAULT_MAX_RETRIES,
            last_call: Mutex::new(None),
        })
    }

    /// Override the pacing delay between calls. Mostly for tests.
    pub fn with_pacing_ms(mut self, ms: u64) -> Self {
        self.pacing_ms = ms;
        self
    }

    /// Wait until at least `pacing_ms` has elapsed since the previous call.
    async fn pace(&self) {
        let wait = {
            let mut guard = self.last_call.lock().unwrap();
            let now = Instant::now();
            let wait = match *guard {
                Some(t) => {
                    let elapsed = now.duration_since(t).as_millis() as u64;
                    if elapsed < self.pacing_ms {
                        Some(self.pacing_ms - elapsed)
                    } else {
                        None
                    }
                }
                None => None,
            };
            *guard = Some(now);
            wait
        };
        if let Some(ms) = wait {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
    }

    /// Build the full prompt: system + user concatenated. The CLI takes one
    /// blob and we don't get separate roles — that's a CLI limitation.
    fn build_prompt(&self, req: &CompletionRequest) -> String {
        let mut prompt = String::new();
        if let Some(prelude) = &req.cacheable_prelude {
            prompt.push_str(prelude);
            prompt.push_str("\n\n");
        }
        if !req.system.is_empty() {
            prompt.push_str(&req.system);
            prompt.push_str("\n\n");
        }
        // Strong nudge toward JSON-only output since the CLI doesn't enforce
        // schemas. We still defensively parse the response.
        prompt.push_str(&format!(
            "Respond ONLY with a JSON object matching this schema (no preamble, no markdown, no prose):\n{}\n\n",
            serde_json::to_string_pretty(&req.schema).unwrap_or_default()
        ));
        prompt.push_str(&req.user);
        prompt
    }

    /// Single CLI invocation. Returns raw stdout.
    ///
    /// The prompt is fed via stdin (not argv) so we never hit Windows'
    /// command-line length cap (~32 KB) on big rerank prompts. This matches
    /// the pattern in `scripts/claude-answer.sh`.
    async fn invoke_cli(&self, prompt: &str) -> LlmResult<String> {
        // Compose the actual command. CLAUDE_CMD may be multi-word
        // ("node /path/to/cli.js"), so we split on whitespace and treat the
        // first token as the program and the rest as initial args.
        let parts: Vec<&str> = self.base_cmd.split_whitespace().collect();
        let (program, base_args) = parts.split_first().ok_or_else(|| {
            LlmError::Config("empty CLAUDE_CMD".into())
        })?;

        let mut cmd = Command::new(program);
        cmd.args(base_args);
        if let Some(model) = &self.model {
            cmd.arg("--model").arg(model);
        }
        cmd.arg("-p").arg("--dangerously-skip-permissions");
        // No prompt argv — stdin instead.
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| LlmError::Llm(format!(
            "claude cli spawn failed: {} (cmd was: `{}`)",
            e, self.base_cmd
        )))?;

        // Write the prompt to stdin and close it.
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| LlmError::Llm(format!("claude cli stdin write failed: {}", e)))?;
            stdin
                .shutdown()
                .await
                .map_err(|e| LlmError::Llm(format!("claude cli stdin close failed: {}", e)))?;
        }

        let output = child.wait_with_output().await.map_err(|e| LlmError::Llm(format!(
            "claude cli wait failed: {} (cmd was: `{}`)",
            e, self.base_cmd
        )))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(LlmError::Llm(format!(
                "claude cli exit {:?}: {}",
                output.status.code(),
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if stdout.trim().is_empty() {
            return Err(LlmError::Llm("claude cli returned empty stdout".into()));
        }
        Ok(stdout)
    }
}

#[async_trait]
impl LlmProvider for ClaudeCodeCliProvider {
    fn name(&self) -> &'static str {
        "claude-cli"
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities {
            // We *ask* for structured output but cannot enforce it — handled
            // via defensive parsing.
            supports_structured_output: false,
            supports_prompt_caching: false,
            max_context_window: 200_000,
            default_max_output_tokens: 8_192,
        }
    }

    async fn complete(&self, req: &CompletionRequest) -> LlmResult<CompletionResponse> {
        let prompt = self.build_prompt(req);
        let mut last_err: Option<LlmError> = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                // Exponential backoff: 1 s, 2 s, 4 s.
                let wait_ms = 1000u64 * (1u64 << (attempt - 1));
                tokio::time::sleep(Duration::from_millis(wait_ms)).await;
            }
            self.pace().await;

            let start = Instant::now();
            match self.invoke_cli(&prompt).await {
                Ok(raw) => {
                    let elapsed_ms = start.elapsed().as_millis() as u64;
                    match extract_json(&raw) {
                        Some(json) => {
                            return Ok(CompletionResponse {
                                json,
                                raw,
                                // The CLI doesn't expose token counts; leave at zero.
                                usage: TokenUsage::default(),
                                provider: "claude-cli".into(),
                                model: self
                                    .model
                                    .clone()
                                    .unwrap_or_else(|| "default".into()),
                                duration_ms: elapsed_ms,
                            });
                        }
                        None => {
                            last_err = Some(LlmError::Llm(format!(
                                "claude cli output was not parseable as JSON (attempt {}): {}",
                                attempt + 1,
                                raw.chars().take(300).collect::<String>()
                            )));
                            // Loop will retry with the same prompt.
                        }
                    }
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| LlmError::Llm("claude cli: all retries exhausted".into())))
    }
}

/// Defensive JSON extractor. Handles four common cases:
///   1. Pure JSON object
///   2. Fenced code block ```json ... ```
///   3. Preamble text + JSON
///   4. Multiple JSON objects (returns the first)
pub(crate) fn extract_json(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();

    // 1. Direct parse — happiest path.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(v);
    }

    // 2. Strip a fenced ```json ... ``` block.
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        // Eat optional language tag like `json\n`.
        let body_start = after_fence
            .find('\n')
            .map(|p| &after_fence[p + 1..])
            .unwrap_or(after_fence);
        if let Some(end) = body_start.find("```") {
            let body = &body_start[..end];
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body.trim()) {
                return Some(v);
            }
        }
    }

    // 3 & 4. Find the first balanced { ... } and try parsing it.
    let bytes = trimmed.as_bytes();
    let mut depth: i32 = 0;
    let mut start: Option<usize> = None;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        let candidate = &trimmed[s..=i];
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(candidate) {
                            return Some(v);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_pure_json() {
        let v = extract_json(r#"{"a":1}"#).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn extract_fenced_json() {
        let raw = "```json\n{\"a\":2}\n```";
        let v = extract_json(raw).unwrap();
        assert_eq!(v["a"], 2);
    }

    #[test]
    fn extract_fenced_json_no_lang_tag() {
        let raw = "```\n{\"a\":3}\n```";
        let v = extract_json(raw).unwrap();
        assert_eq!(v["a"], 3);
    }

    #[test]
    fn extract_with_preamble() {
        let raw = "Sure, here you go:\n\n{\"a\": 4, \"b\": [1,2,3]}\n\nLet me know if you need more.";
        let v = extract_json(raw).unwrap();
        assert_eq!(v["a"], 4);
        assert_eq!(v["b"][1], 2);
    }

    #[test]
    fn extract_multiple_objects_takes_first_complete() {
        let raw = r#"{"a":1} and also {"b":2}"#;
        let v = extract_json(raw).unwrap();
        assert_eq!(v["a"], 1);
        assert!(v.get("b").is_none());
    }

    #[test]
    fn extract_returns_none_for_garbage() {
        assert!(extract_json("just some prose, no json here").is_none());
        assert!(extract_json("").is_none());
    }

    #[test]
    fn build_prompt_includes_schema_and_user() {
        let cfg = LlmConfig {
            provider: crate::config::LlmProviderKind::ClaudeCli,
            model: "claude-opus-4-7".into(),
            api_key: None,
            base_url: None,
        };
        let p = ClaudeCodeCliProvider::from_config(&cfg).unwrap();
        let req = CompletionRequest {
            system: "You rewrite queries.".into(),
            user: "Original: what is breed of my dog?".into(),
            cacheable_prelude: None,
            schema: json!({"type":"object","properties":{"rewrites":{"type":"array"}}}),
            schema_name: "rewrite".into(),
            max_output_tokens: 200,
            temperature: 0.2,
            json_object: false,
        };
        let prompt = p.build_prompt(&req);
        assert!(prompt.contains("rewrite queries"));
        assert!(prompt.contains("rewrites"));
        assert!(prompt.contains("Original: what is breed"));
        assert!(prompt.contains("ONLY with a JSON object"));
    }
}
