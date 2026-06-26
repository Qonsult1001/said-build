//! Agent steering — a core plugin (like the document/PDF plugin) that tells a coding agent WHEN/WHAT
//! to use `.said`, without ever editing CLAUDE.md.
//!
//! Design + rationale: docs/said-structure/16-agent-steering.md. In short: an MCP server alone cannot
//! steer the agent (MCP only EXPOSES tools the model may choose). Steering requires the agent's native
//! `PreToolUse` HOOK surface — the agent pipes the tool-call JSON on stdin and reads a decision on
//! stdout. We mirror nudge (attunehq/nudge, Apache-2.0) faithfully, but as a `.said` core module with
//! PER-AGENT ADAPTERS, structured so new agents are a small addition.
//!
//! Agent support today: **Claude Code only**. Codex / Cursor / Gemini CLI / Windsurf / Aider each need
//! their OWN adapter (different stdin JSON, different settings file, different tool vocabulary) — there
//! is NO universal hook layer across agents (confirmed at nudge's source: it ships hand-written
//! Claude + Codex adapters and nothing else). Those are documented as `Agent` variants + `// TODO`
//! adapters below so the extension path is explicit.
//!
//! Default behaviour (user-chosen): when the agent is about to SEARCH CODE (the `Grep` tool, or a
//! `Bash` grep/rg/ripgrep command), inject the relevant `.said` recall as `additionalContext` and LET
//! the search PROCEED (nudge's fail-open "tap-on-shoulder" pattern). Fail-open: any error → plain
//! allow, so the agent is never stuck. (OPEN EXPERIMENT — see the design doc — whether block-redirect
//! beats inject-and-proceed is to be measured, not assumed.)

use crate::said_file::SaidFile;

/// Which coding agent's hook protocol we're speaking. Only `ClaudeCode` has an adapter today; the rest
/// are declared so the per-agent extension points are visible (each needs its own parse/render — there
/// is no universal hook across agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    // TODO(adapter): Codex CLI — `.codex/hooks.json`, tool vocab `Bash|apply_patch`, different JSON.
    // TODO(adapter): Cursor — no PreToolUse command-hook today; would need its own integration.
    // TODO(adapter): GeminiCli — own hook surface/JSON.
    // TODO(adapter): Windsurf — own hook surface/JSON.
    // TODO(adapter): Aider — own hook surface/JSON.
}

impl Agent {
    pub fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" | "claude-code" | "claudecode" => Some(Agent::ClaudeCode),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self { Agent::ClaudeCode => "claude-code" }
    }
    /// The settings file the hook registers into — gitignored, so removal leaves NO git trace.
    pub fn settings_path(self) -> &'static str {
        match self { Agent::ClaudeCode => ".claude/settings.local.json" }
    }
}

/// The kind of tool call the agent is about to make, normalized across agents. We only need to know
/// "is this a code search?" to decide whether to inject `.said` recall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolAction {
    /// The native Grep/search tool — `query` is the search pattern.
    CodeSearch { query: String },
    /// A shell command — `command` is the raw line (may or may not be a grep/rg).
    Shell { command: String },
    /// The user's PROMPT (UserPromptSubmit) — `prompt` is what they asked. We recall against it and
    /// inject the result as trusted, factual context alongside the prompt.
    UserPrompt { prompt: String },
    /// Anything else (Write/Edit/Read/...). We pass these through untouched.
    Other,
}

/// Which hook surface fired. THE CHANNEL determines whether the model TRUSTS the injected recall —
/// proven by live A/B testing + Anthropic's own docs:
///   * `UserPromptSubmit` → context injected ALONGSIDE the user's prompt (the high-trust user-message
///     slot). The model USES it. This is the channel that WORKS (nudge's "Continue" outcome; also how
///     claude-mem injects). `.said`'s DEFAULT for recall.
///   * `SessionStart` → injected before the first prompt (durable project memory). Also trusted.
///   * `PreToolUse` / `PostToolUse` → context lands "next to the tool result" — the LOWEST-TRUST
///     channel Anthropic explicitly TRAINS the model to be skeptical of. Live test: the model flagged
///     it as a prompt-injection attempt and REFUSED to act on it (and PostToolUse context is often
///     dropped entirely, Claude Code #18427). We keep PreToolUse only for deterministic BLOCK/redirect
///     (nudge's actual PreToolUse use), NOT for trusted context injection.
/// CRUCIAL companion to the channel: the injected text must be FACTUAL LABELED DATA, never an
/// imperative ("use this instead of grep") — imperative framing trips the injection defense even on a
/// trusted channel (Anthropic hooks doc). See `decide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase { UserPromptSubmit, SessionStart, PreToolUse, PostToolUse }

/// A normalized hook event — what every agent adapter parses its stdin JSON INTO.
#[derive(Debug, Clone)]
pub struct HookEvent {
    pub phase: HookPhase,
    pub action: ToolAction,
}

/// The agent-agnostic decision. Adapters render this into their agent's wire JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    /// Let the tool call proceed, unchanged (no context to add).
    Passthrough,
    /// (PreToolUse) Let the tool call proceed, but inject `context` for the model to read first.
    /// NOTE: distrusted in practice (see HookPhase) — Post/Redirect is the trusted default.
    AllowWithContext { context: String },
    /// (UserPromptSubmit / SessionStart) Provide `context` as factual project data alongside the
    /// prompt — the TRUSTED channel the model actually uses. This is `.said`'s DEFAULT.
    Provide { context: String },
    /// (PostToolUse) Inject `context` next to the tool result. DISTRUSTED in practice — legacy only.
    Redirect { context: String },
    /// DENY the tool call and tell the agent to use `.said` first; `reason` carries the recall +
    /// instruction. The agent must act on `.said` instead of grepping (block-redirect mode).
    Deny { reason: String },
}

/// How the hook reacts when the agent is about to search code AND `.said` has a relevant answer.
/// The OPEN EXPERIMENT (docs/16-agent-steering): does Block beat Inject on tokens-to-locate?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerMode {
    /// Inject `.said` recall as context, LET the search proceed (fail-open default).
    Inject,
    /// DENY the search and redirect the agent to act on `.said` first (forceful, max token-saving).
    Block,
}

impl SteerMode {
    pub fn from_str_ci(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "inject" | "allow" => Some(SteerMode::Inject),
            "block" | "deny" | "redirect" => Some(SteerMode::Block),
            _ => None,
        }
    }
}

/// An agent adapter: parse this agent's PreToolUse stdin JSON into a normalized `HookEvent`, and render
/// a `HookDecision` back into this agent's stdout wire format. New agents implement this trait.
pub trait AgentAdapter {
    fn agent(&self) -> Agent;
    /// Parse the agent's PreToolUse JSON (already deserialized to a serde_json::Value) into our event.
    fn parse_event(&self, v: &serde_json::Value) -> Option<HookEvent>;
    /// Render our decision into the agent's stdout JSON. `None` means "emit nothing" (pure passthrough).
    fn render_decision(&self, d: &HookDecision) -> Option<serde_json::Value>;
}

/// Does this shell command line look like a codebase grep/search? (rg, grep, ripgrep, ag, ack, git grep)
fn is_grep_command(cmd: &str) -> bool {
    let c = cmd.trim_start();
    let first = c.split_whitespace().next().unwrap_or("");
    matches!(first, "rg" | "grep" | "ripgrep" | "ag" | "ack")
        || c.starts_with("git grep")
}

/// Extract the recall-intent string from the event. For a code search it's the pattern; for a
/// UserPromptSubmit it's the user's prompt. None = nothing to recall against (passthrough).
fn search_intent(event: &HookEvent) -> Option<String> {
    match &event.action {
        ToolAction::CodeSearch { query } => Some(query.clone()),
        ToolAction::Shell { command } if is_grep_command(command) => Some(command.clone()),
        ToolAction::UserPrompt { prompt } if prompt.trim().len() >= 3 => Some(prompt.clone()),
        _ => None,
    }
}

/// THE DECISION — agent-agnostic, pure, testable. Query `.said` against the event's intent and return
/// its recall on the right channel, in the right FRAMING. The combination is what makes the model
/// actually USE the recall (proven by live A/B + Anthropic's docs):
///   * UserPromptSubmit / SessionStart → `Provide`: inject the recall as FACTUAL LABELED DATA (a
///     `<project_index>` block of `symbol — file:line — fact`) alongside the prompt. The model trusts
///     it and answers from it — NO grep, NO injection-flag. THE DEFAULT (the channel that works).
///   * PostToolUse → `Redirect` (legacy; the model DISTRUSTS tool-result-adjacent context — kept only
///     for completeness, do not rely on it).
///   * PreToolUse → `mode` Inject/Block (Block = deterministic deny+redirect, nudge's real PreToolUse
///     use; Inject is distrusted).
/// CRITICAL: the UserPromptSubmit/SessionStart text is PURE FACTS — no "use this instead of grep", no
/// "this is trusted memory" meta-claim. Imperative/meta framing trips the injection defense even on a
/// trusted channel (Anthropic hooks doc; measured). Fail-open: nothing GROUNDED → passthrough.
pub fn decide(brain: &mut SaidFile, event: &HookEvent, mode: SteerMode) -> HookDecision {
    let Some(intent) = search_intent(event) else { return HookDecision::Passthrough; };
    let (cands, keywords) = crate::ask::ask(brain, &intent, 5, false, None);
    if cands.is_empty() { return HookDecision::Passthrough; }
    // FAIL-OPEN guard. For a literal GREP pattern (tool channels) we require LEXICAL GROUNDING (the
    // pattern is exact, so a relevant hit shares a term — COIL/Clarity, kills off-topic proximity
    // artifacts). For a natural-language USER PROMPT we do NOT require lexical overlap — the user
    // paraphrases ("where is session EXPIRY handled" vs code that says `expires_at`), so `.said`'s own
    // semantic ranking + abstention is the right gate; over-filtering here silently dropped real
    // recall in the live A/B. We only require that the top hit isn't trivially weak.
    let is_prompt = matches!(event.action, ToolAction::UserPrompt { .. });
    let grounded = if is_prompt {
        cands.first().map(|c| c.confidence >= 0.20).unwrap_or(false)
    } else {
        cands.iter().any(|c| {
            let lc = c.content.to_lowercase();
            keywords.iter().any(|k| k.len() >= 3 && lc.contains(k.as_str()))
        })
    };
    if !grounded { return HookDecision::Passthrough; }

    match event.phase {
        // The TRUSTED, PROVEN channel: factual labeled-data, no imperative, no meta-claim.
        HookPhase::UserPromptSubmit | HookPhase::SessionStart => {
            let mut ctx = String::from("<project_index source=\".said\">\n");
            for c in cands.iter().take(5) {
                let loc = c.location.as_deref().unwrap_or("");
                let snippet: String = c.content.chars().take(160).collect();
                let sym = c.doc_id.rsplit("::").nth(1).unwrap_or(&c.doc_id);
                ctx.push_str(&format!("{} — {} {} — {}\n", sym, c.doc_id, loc, snippet.replace('\n', " ").trim()));
            }
            ctx.push_str("</project_index>");
            HookDecision::Provide { context: ctx }
        }
        // Legacy tool-result-adjacent channels (distrusted — see HookPhase). Kept, not recommended.
        HookPhase::PostToolUse => {
            let ctx = format!("<project_index source=\".said\">\n{}\n</project_index>", index_lines(&cands));
            HookDecision::Redirect { context: ctx }
        }
        HookPhase::PreToolUse => {
            let ctx = format!("<project_index source=\".said\">\n{}\n</project_index>", index_lines(&cands));
            match mode {
                SteerMode::Inject => HookDecision::AllowWithContext { context: ctx },
                SteerMode::Block => HookDecision::Deny { reason: ctx },
            }
        }
    }
}

/// Compact factual lines for the legacy (tool-adjacent) channels.
fn index_lines(cands: &[crate::ask::AskCandidate]) -> String {
    let mut s = String::new();
    for c in cands.iter().take(5) {
        let loc = c.location.as_deref().unwrap_or("");
        let snippet: String = c.content.chars().take(160).collect();
        let sym = c.doc_id.rsplit("::").nth(1).unwrap_or(&c.doc_id);
        s.push_str(&format!("{} — {} {} — {}\n", sym, c.doc_id, loc, snippet.replace('\n', " ").trim()));
    }
    s
}

// ── Claude Code adapter ─────────────────────────────────────────────────────────────────────────
// Claude's PreToolUse stdin JSON: { "hook_event_name":"PreToolUse", "tool_name":"Grep"|"Bash"|...,
//   "tool_input": { ...tool-specific... } }. The Grep tool's input has `pattern`; Bash has `command`.
// The decision is emitted as the newer hookSpecificOutput envelope:
//   { "hookSpecificOutput": { "hookEventName":"PreToolUse", "permissionDecision":"allow",
//     "additionalContext":"..." } }   (allow + context = our inject-and-proceed default)

pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn agent(&self) -> Agent { Agent::ClaudeCode }

    fn parse_event(&self, v: &serde_json::Value) -> Option<HookEvent> {
        let ev = v.get("hook_event_name").and_then(|x| x.as_str()).unwrap_or("");
        let phase = match ev {
            "UserPromptSubmit" => HookPhase::UserPromptSubmit,
            "SessionStart" => HookPhase::SessionStart,
            "PreToolUse" => HookPhase::PreToolUse,
            "PostToolUse" => HookPhase::PostToolUse,
            _ => return Some(HookEvent { phase: HookPhase::UserPromptSubmit, action: ToolAction::Other }),
        };
        // UserPromptSubmit carries the user's `prompt`; tool events carry `tool_name` + `tool_input`.
        if phase == HookPhase::UserPromptSubmit {
            let prompt = v.get("prompt").and_then(|x| x.as_str()).unwrap_or("");
            return Some(HookEvent { phase, action: ToolAction::UserPrompt { prompt: prompt.to_string() } });
        }
        let tool = v.get("tool_name").and_then(|x| x.as_str()).unwrap_or("");
        let input = v.get("tool_input");
        let action = match tool {
            "Grep" => {
                let q = input.and_then(|i| i.get("pattern")).and_then(|x| x.as_str()).unwrap_or("");
                ToolAction::CodeSearch { query: q.to_string() }
            }
            "Bash" => {
                let cmd = input.and_then(|i| i.get("command")).and_then(|x| x.as_str()).unwrap_or("");
                ToolAction::Shell { command: cmd.to_string() }
            }
            _ => ToolAction::Other,
        };
        Some(HookEvent { phase, action })
    }

    fn render_decision(&self, d: &HookDecision) -> Option<serde_json::Value> {
        match d {
            HookDecision::Passthrough => None, // emit nothing → agent proceeds unchanged
            // TRUSTED channel — factual project data alongside the prompt (UserPromptSubmit). The model
            // uses it; no permissionDecision (this event doesn't gate a tool).
            HookDecision::Provide { context } => Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "UserPromptSubmit",
                    "additionalContext": context,
                }
            })),
            HookDecision::AllowWithContext { context } => Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "additionalContext": context,
                }
            })),
            // PostToolUse "allow then redirect" — additionalContext injected as FEEDBACK on the result.
            HookDecision::Redirect { context } => Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": context,
                }
            })),
            HookDecision::Deny { reason } => Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": reason,
                }
            })),
        }
    }
}

/// Get the adapter for an agent (the only public entry the CLI/`said hook` needs).
pub fn adapter_for(agent: Agent) -> Box<dyn AgentAdapter> {
    match agent {
        Agent::ClaudeCode => Box::new(ClaudeCodeAdapter),
    }
}

/// End-to-end: take an agent's raw PreToolUse stdin JSON, run the decision against `brain`, and return
/// the agent's stdout JSON (or `None` for "emit nothing / passthrough"). This is what a `said hook`
/// subcommand wires to stdin/stdout. Pure except for the `.said` query — fully testable. `mode` selects
/// inject-and-proceed (default) vs block-redirect (the open experiment).
pub fn run_hook(brain: &mut SaidFile, agent: Agent, stdin_json: &serde_json::Value, mode: SteerMode)
    -> Option<serde_json::Value>
{
    let adapter = adapter_for(agent);
    let event = adapter.parse_event(stdin_json)?;
    let decision = decide(brain, &event, mode);
    adapter.render_decision(&decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grep_command_detection() {
        assert!(is_grep_command("rg 'fn main'"));
        assert!(is_grep_command("grep -r foo src/"));
        assert!(is_grep_command("git grep TODO"));
        assert!(!is_grep_command("cargo test"));
        assert!(!is_grep_command("ls -la"));
    }

    #[test]
    fn claude_parses_grep_and_bash() {
        let a = ClaudeCodeAdapter;
        let grep = serde_json::json!({
            "hook_event_name": "PreToolUse", "tool_name": "Grep",
            "tool_input": { "pattern": "session expiry" }
        });
        assert_eq!(a.parse_event(&grep).unwrap().action,
                   ToolAction::CodeSearch { query: "session expiry".into() });
        let bash = serde_json::json!({
            "hook_event_name": "PreToolUse", "tool_name": "Bash",
            "tool_input": { "command": "rg 'expires_at' src/" }
        });
        assert_eq!(a.parse_event(&bash).unwrap().action,
                   ToolAction::Shell { command: "rg 'expires_at' src/".into() });
        // a non-search tool passes through
        let write = serde_json::json!({
            "hook_event_name": "PreToolUse", "tool_name": "Write",
            "tool_input": { "file_path": "x.rs", "content": "fn x(){}" }
        });
        assert_eq!(a.parse_event(&write).unwrap().action, ToolAction::Other);
    }

    #[test]
    fn render_allow_with_context_is_claude_envelope() {
        let a = ClaudeCodeAdapter;
        let out = a.render_decision(&HookDecision::AllowWithContext { context: "hi".into() }).unwrap();
        assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(out["hookSpecificOutput"]["additionalContext"], "hi");
        // passthrough emits nothing
        assert!(a.render_decision(&HookDecision::Passthrough).is_none());
    }
}
