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
    /// Anything else (Write/Edit/Read/...). We pass these through untouched.
    Other,
}

/// A normalized PreToolUse event — what every agent adapter parses its stdin JSON INTO.
#[derive(Debug, Clone)]
pub struct HookEvent {
    pub action: ToolAction,
}

/// The agent-agnostic decision. Adapters render this into their agent's wire JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    /// Let the tool call proceed, unchanged (no context to add).
    Passthrough,
    /// Let the tool call proceed, but inject `context` for the model to read first (fail-open default).
    AllowWithContext { context: String },
    // (block/redirect intentionally NOT the default — see the design doc's open experiment.)
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

/// Extract a search-intent string from the event, if it IS a code search. Returns None for non-search.
fn search_intent(event: &HookEvent) -> Option<String> {
    match &event.action {
        ToolAction::CodeSearch { query } => Some(query.clone()),
        ToolAction::Shell { command } if is_grep_command(command) => {
            // Use the raw command as the recall query — the pattern inside it carries the intent.
            Some(command.clone())
        }
        _ => None,
    }
}

/// THE DECISION — agent-agnostic, pure, testable. When the agent is about to search code, query
/// `.said` and return the recall as `additionalContext` so the model sees what `.said` already knows
/// BEFORE it greps (often it then doesn't need to). Non-search calls pass through untouched.
/// Fail-open: if `.said` has nothing relevant, passthrough (never block the agent).
pub fn decide(brain: &mut SaidFile, event: &HookEvent) -> HookDecision {
    let Some(intent) = search_intent(event) else { return HookDecision::Passthrough; };
    // Query .said via the documented retrieval verb. Top-5 is enough to orient the agent cheaply.
    let (cands, keywords) = crate::ask::ask(brain, &intent, 5, false, None);
    if cands.is_empty() { return HookDecision::Passthrough; }
    // FAIL-OPEN guard — LEXICAL GROUNDING (COIL/Clarity, same principle as ask's existence-abstention,
    // applied inline here so the hook is thread-safe and self-contained, no global env toggle): an
    // off-topic search ("kubernetes TLS" against a Rust auth brain) returns the closest-but-irrelevant
    // note that shares NO query term. If NOTHING in the top results shares a query content-term, there
    // is no real answer — pass through and inject nothing (the agent greps normally). A binary
    // set-intersection, no magnitude threshold; a genuine hit shares at least one term.
    let grounded = cands.iter().any(|c| {
        let lc = c.content.to_lowercase();
        keywords.iter().any(|k| k.len() >= 3 && lc.contains(k.as_str()))
    });
    if !grounded { return HookDecision::Passthrough; }
    // Build a COMPACT context block (token discipline: locations + short snippets, not whole files).
    let mut ctx = String::from(".said memory — relevant before searching the codebase:\n");
    for (i, c) in cands.iter().take(5).enumerate() {
        let loc = c.location.as_deref().unwrap_or("");
        let snippet: String = c.content.chars().take(200).collect();
        ctx.push_str(&format!("{}. [{:.2}][{}] {} {}\n   {}\n",
            i + 1, c.confidence, c.kind, c.doc_id, loc, snippet.replace('\n', " ")));
    }
    ctx.push_str("(If this answers your need, you can skip the grep.)");
    HookDecision::AllowWithContext { context: ctx }
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
        // Only act on PreToolUse.
        let ev = v.get("hook_event_name").and_then(|x| x.as_str()).unwrap_or("");
        if ev != "PreToolUse" { return Some(HookEvent { action: ToolAction::Other }); }
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
        Some(HookEvent { action })
    }

    fn render_decision(&self, d: &HookDecision) -> Option<serde_json::Value> {
        match d {
            HookDecision::Passthrough => None, // emit nothing → agent proceeds unchanged
            HookDecision::AllowWithContext { context } => Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "additionalContext": context,
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
/// subcommand wires to stdin/stdout. Pure except for the `.said` query — fully testable.
pub fn run_hook(brain: &mut SaidFile, agent: Agent, stdin_json: &serde_json::Value)
    -> Option<serde_json::Value>
{
    let adapter = adapter_for(agent);
    let event = adapter.parse_event(stdin_json)?;
    let decision = decide(brain, &event);
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
