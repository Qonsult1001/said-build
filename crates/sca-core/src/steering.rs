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
    /// SESSION END — the work is wrapping up. Carries the transcript path so the backstop can read the
    /// last context and write a journal IF the agent didn't already (the safety net in the hybrid
    /// agent-judged + backstop write model; see `backstop_session_end`).
    SessionEnd { transcript_path: Option<String>, session_id: Option<String> },
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
pub enum HookPhase { UserPromptSubmit, SessionStart, PreToolUse, PostToolUse, SessionEnd }

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
    // SESSION RESUME (point 2): on SessionStart, surface the MOST RECENT journal ("where you left off")
    // so process-state persists across sessions via .said — replacing the agent's ephemeral session
    // memory. Claude reloads its own per-project memory at start; this does the same to the durable,
    // portable, BYO-LLM store. Leads even when there's no prompt to recall against (SessionStart has
    // none). If there's no journal, fall through to the normal recall path below.
    if event.phase == HookPhase::SessionStart {
        if let Some(resume) = render_session_resume(brain) {
            return HookDecision::Provide { context: resume };
        }
    }
    let Some(intent) = search_intent(event) else { return HookDecision::Passthrough; };

    // FIX-FIRST RECALL — on BOTH the prompt channel AND the decision point (nudge's actual mechanism).
    // Nudge's research (research/nudge/docs/RESEARCH.md): injected guidance DECAYS over a session
    // (95% compliance early → 20-60% after 10+ turns / after compaction), and the model "can recite the
    // rule but ignores it in practice." Their fix is to re-inject the matching learned note AT THE
    // DECISION POINT too — `AllowPreToolUseWithContext` carries the note when the agent is about to use a
    // tool (read/grep), not just once at prompt time. `.said` previously recalled the fix ONLY on
    // UserPromptSubmit, so when the agent then went to READ/GREP the source, nothing re-surfaced the fix
    // — that's the A2 over-investigation (it had the answer but re-derived it from source anyway).
    //   • UserPromptSubmit/SessionStart → Provide the fix (trusted prompt-time context).
    //   • PreToolUse (about to read/grep) → AllowWithContext: allow the tool BUT re-inject the fix so the
    //     agent sees "you already concluded this" right as it reaches for the source. Fail-open (allow),
    //     never blocks — matches nudge's Warning/AllowWithContext, not a hard Interrupt.
    // TOP-K, not top-1 — aligned to nudge's HOOK_SEARCH_LIMIT=3 (research/nudge learn.rs): nudge injects
    // the top-3 matched notes and lets the model PICK, which rescues cases where the right note isn't
    // rank-1 (the documented top-N-vs-hard-floor principle; also docs/15-orchestration injects top-k=5).
    // `.said` previously injected only the single best fix (recall_coding_fix k=1) — a rank-2 correct fix
    // on a paraphrase was lost. recall_coding_fixes self-abstains per-candidate below the floor, so an
    // unrelated tool call still yields an empty set → passthrough.
    const HOOK_FIX_TOPK: usize = 3; // == nudge HOOK_SEARCH_LIMIT
    let fixes = crate::ask::recall_coding_fixes(brain, &intent, HOOK_FIX_TOPK, 0.45);
    if !fixes.is_empty() {
        return match event.phase {
            HookPhase::UserPromptSubmit | HookPhase::SessionStart =>
                HookDecision::Provide { context: render_verified_fixes(&fixes) },
            HookPhase::PreToolUse =>
                HookDecision::AllowWithContext { context: render_verified_fixes(&fixes) },
            HookPhase::PostToolUse =>
                HookDecision::Redirect { context: render_verified_fixes(&fixes) },
            HookPhase::SessionEnd => HookDecision::Passthrough,
        };
    }

    // BLUEPRINT-FIRST TRIGGER (the reuse-the-80% nudge). When the prompt expresses INTENT TO BUILD a
    // shape ("create/add/implement a ... endpoint/handler/service/...") AND `.said` already holds a
    // blueprint for that shape, surface it on the trusted prompt channel as FACTUAL data: "you have a
    // reusable structure for this — recall_blueprint and render it, write only the 20%." This is what
    // makes the agent REUSE instead of recreate, without being told. Prompt-channel only (it's about
    // what to build next, not a tool the agent is mid-using); fail-open when there's no build intent or
    // no matching blueprint. Same factual, non-imperative framing the channel requires.
    if matches!(event.phase, HookPhase::UserPromptSubmit | HookPhase::SessionStart)
        && looks_like_build_intent(&intent)
    {
        let bps = crate::ask::recall_blueprints(brain, &intent, 1, 0.40);
        if let Some(bp) = bps.into_iter().next() {
            return HookDecision::Provide { context: render_blueprint_nudge(&bp) };
        }
    }

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
        // SessionEnd never reaches decide() (run_hook routes it to the backstop write path before the
        // recall), but the match must be total. Passthrough is the safe no-op.
        HookPhase::SessionEnd => HookDecision::Passthrough,
    }
}

/// Render a recalled VERIFIED coding fix as PLAIN FACTUAL CONTEXT — the nudge pattern (attunehq/nudge:
/// the UserPromptSubmit "Continue" outcome emits the learned note as plain stdout, NO wrapper tag, NO
/// meta-claim). The earlier `<verified_memory>…this is gate-verified, REUSE it…>` framing was REJECTED
/// live: Claude treated the assertive claim as something to verify and re-investigated the source (17
/// turns) instead of using it. Plain factual recall — stated as what's known, not a claim about its
/// authority — is what the model assimilates. This matches the project's own steering finding: on the
/// trusted UserPromptSubmit channel the text must be FACTS, never an imperative/meta-claim, or it trips
/// the injection-skepticism defense. We surface the learning (the idea + why); the model decides.
const FIX_NOTE_MAX: usize = 2400; // ~600 tokens of learning per fix — the gotchas, not the codebase

fn cap_note(s: &str, max: usize) -> String {
    if s.len() <= max { return s.to_string(); }
    let mut kept = String::new();
    for line in s.lines() {
        if kept.len() + line.len() + 1 > max { break; }
        kept.push_str(line); kept.push('\n');
    }
    kept.push_str("…\n");
    kept
}

/// Render the top-K recalled VERIFIED fixes as one `<project_memory>` block. Mirrors nudge end-to-end:
///  * top-K (== HOOK_SEARCH_LIMIT=3), not top-1 — the model PICKS the fitting one, rescuing a rank-2
///    correct fix on a paraphrase (nudge learn.rs; docs/15-orchestration top-k=5).
///  * nudge's EXACT lead line (learn.rs::hook_context_for_query): a plain factual note + the one
///    behavioral directive that makes the agent consult memory FIRST — "Read this before repeating old
///    debugging work." NOT an authority claim ("verified, REUSE this" — that tripped the injection
///    defense, rejected live at 17 turns); NOT pure passive facts either (that over-corrected → the agent
///    treated it as optional and still re-read source). The directive is what curbs over-investigation.
/// SESSION RESUME context (point 2): the most recent `kind:journal` frame, rendered as plain-facts
/// prior-session state (nudge framing — labeled data, no imperative). Returns None when there's no
/// journal. This is what lets "where I left off" persist to .said and resume on the next SessionStart.
fn render_session_resume(brain: &mut SaidFile) -> Option<String> {
    // newest active frame tagged kind:journal (by created_at)
    let mut best: Option<(u64, String)> = None;
    for did in brain.frames.active_doc_ids().into_iter().map(|s| s.to_string()).collect::<Vec<_>>() {
        if let Some(m) = brain.frames.get_meta(&did) {
            if m.tags.iter().any(|t| t == "kind:journal") {
                let ts = m.created_at;
                if best.as_ref().map(|(b, _)| ts >= *b).unwrap_or(true) {
                    best = Some((ts, did.clone()));
                }
            }
        }
    }
    let (_, doc_id) = best?;
    let body = brain.get(&doc_id).unwrap_or_default();
    if body.trim().is_empty() { return None; }
    Some(format!(
        "<project_memory source=\".said\" kind=\"session-resume\">\n\
         Where the last session left off (your own journal — resume from here, don't re-plan from scratch):\n\
         {}\n</project_memory>",
        cap_note(&body, FIX_NOTE_MAX)))
}

fn render_verified_fixes(fixes: &[crate::ask::RecalledFix]) -> String {
    let mut body = String::from(
        "<project_memory source=\".said\">\nFound prior work that may apply. Read this before \
         repeating old debugging work — if it answers the question, use it and don't re-investigate.");
    if fixes.len() > 1 {
        body.push_str(&format!(" {} candidates, most relevant first; pick the matching one:", fixes.len()));
    }
    body.push('\n');
    for (i, fix) in fixes.iter().enumerate() {
        if fixes.len() > 1 { body.push_str(&format!("\n--- candidate {} ---\n", i + 1)); }
        body.push_str(&cap_note(&fix.note, FIX_NOTE_MAX));
        body.push('\n');
    }
    body.push_str("</project_memory>");
    body
}

/// Does the prompt express intent to BUILD a new shape (so a blueprint would help)? A verb of creation
/// plus a structural noun. Deliberately conservative — a question ("how does X work") is NOT build
/// intent, so we never nudge a blueprint at someone who's just reading. No hardcoded shape list; these
/// are generic creation/structure words, not entity names.
fn looks_like_build_intent(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    const VERBS: [&str; 6] = ["create", "add", "implement", "build", "scaffold", "write"];
    const NOUNS: [&str; 9] = ["endpoint", "handler", "controller", "service", "route",
                              "api", "resource", "command", "crud"];
    VERBS.iter().any(|v| p.contains(v)) && NOUNS.iter().any(|n| p.contains(n))
}

/// Factual blueprint nudge for the trusted prompt channel — NOT an imperative against grep, just "you
/// already have this structure; recall + render it." Carries the shape + the sections so the model can
/// act without a second round-trip, and names the tool so it can pull the authoritative copy.
fn render_blueprint_nudge(bp: &crate::ask::RecalledBlueprint) -> String {
    format!(
        "<project_memory source=\".said\" kind=\"blueprint\">\n\
         You have a reusable structure for this shape from earlier work: \"{}\". Render these sections \
         in the active language and write only the entity-specific parts — don't recreate the structure. \
         Call recall_blueprint for the authoritative copy.\n\
         Sections: {}\n\
         </project_memory>",
        bp.shape, cap_note(&bp.sections_json, FIX_NOTE_MAX),
    )
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
            // SessionEnd (and Stop) wrap up the session — route to the backstop write path. Claude's
            // SessionEnd JSON carries `transcript_path` + `session_id` (+ `reason`).
            "SessionEnd" | "Stop" => {
                let transcript_path = v.get("transcript_path").and_then(|x| x.as_str()).map(String::from);
                let session_id = v.get("session_id").and_then(|x| x.as_str()).map(String::from);
                return Some(HookEvent {
                    phase: HookPhase::SessionEnd,
                    action: ToolAction::SessionEnd { transcript_path, session_id },
                });
            }
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
            // A Read of a source file is an INVESTIGATION — recall against the file path so a verified
            // fix that already covers it is re-surfaced at the decision point (nudge's PreToolUse
            // learned-note re-injection). The path is the recall intent; the fix store matches on the
            // problem text + the file in the change-set.
            "Read" => {
                let path = input.and_then(|i| i.get("file_path")).and_then(|x| x.as_str()).unwrap_or("");
                ToolAction::CodeSearch { query: path.to_string() }
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
    // SessionEnd is a WRITE path, not an inject path: the backstop captures the last context as a
    // journal IF the agent didn't already (hybrid model). It returns no decision to render.
    if let ToolAction::SessionEnd { transcript_path, session_id } = &event.action {
        backstop_session_end(brain, transcript_path.as_deref(), session_id.as_deref());
        return None;
    }
    let decision = decide(brain, &event, mode);
    adapter.render_decision(&decision)
}

/// SessionEnd BACKSTOP (the safety net in the agent-judged + backstop write model). The PRIMARY path is
/// the agent itself calling `journal`/`remember`/`learn_fix` when it concludes something durable (the
/// model decides what's worth keeping — see said-prompts::steering RECORD-WHAT-YOU-CONCLUDE). This
/// backstop only fires when that did NOT happen this session, so we never double-write a session the
/// agent already summarized — quality-first, capture-guaranteed (mirrors claude-mem's Stop hook as a
/// net under Claude Code's model-judged writes).
///
/// "Did the agent journal this session?" is detected by a `session:<id>` tag on any frame written this
/// session. If absent, we distil a compact last-context summary from the transcript tail and store it as
/// a journal frame, tagged `kind:journal`, `source:backstop`, and `session:<id>` (so a later end-event
/// for the same session is idempotent). Best-effort: any read/parse failure is a silent no-op — a
/// backstop must never break session teardown.
fn backstop_session_end(brain: &mut SaidFile, transcript_path: Option<&str>, session_id: Option<&str>) {
    let sid = session_id.unwrap_or("unknown");
    let session_tag = format!("session:{}", sid);

    // Idempotence + "did the agent already write this session?" — if ANY frame carries this session's
    // tag (a journal/remember/learn_fix the agent made, OR a prior backstop for the same session), do
    // nothing. The agent's own distilled write is always preferred over a transcript-tail summary.
    if brain.frames.active_doc_ids().iter().any(|d| {
        brain.frames.get_meta(d).map(|m| m.tags.iter().any(|t| t == &session_tag)).unwrap_or(false)
    }) {
        return;
    }

    // Distil a compact last-context summary from the transcript tail. We deliberately keep it SMALL
    // (the research lesson: distil, don't dump — a raw transcript pollutes recall). Take the last few
    // user/assistant text lines, not the whole file.
    let summary = match transcript_path.and_then(|p| transcript_tail_summary(p)) {
        Some(s) if !s.trim().is_empty() => s,
        // Nothing useful to capture (no transcript / empty) → no-op. Don't write an empty journal.
        _ => return,
    };

    let date = civil_date_today();
    let doc_id = format!("mem/{}/session-{}", date, sid);
    let title = format!("Session backstop ({})", date);
    brain.remember_as(&doc_id, &summary, Some(&title));
    brain.add_tag(&doc_id, "kind:journal");
    brain.add_tag(&doc_id, "source:backstop");
    brain.add_tag(&doc_id, &session_tag);
    brain.add_tag(&doc_id, &format!("date:{}", date));
    let _ = brain.build_index();
    let _ = brain.save();
}

/// Machine-local civil date `YYYY-MM-DD` (matches the MCP `journal` tool's doc_id convention).
fn civil_date_today() -> String {
    let secs = crate::time_compat::unix_secs() as i64;
    let days = secs.div_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm) — no chrono dependency.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Read the transcript JSONL tail and distil a compact last-context summary. Best-effort: returns None
/// on any read/parse failure. Keeps only the last few human-readable text lines — small by design.
fn transcript_tail_summary(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    // Collect the last ~12 text snippets from assistant/user messages, newest last.
    let mut texts: Vec<String> = Vec::new();
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue; };
        // Claude Code transcript lines vary; pull any string `text` fields from message content.
        let role = v.get("type").and_then(|x| x.as_str())
            .or_else(|| v.get("role").and_then(|x| x.as_str())).unwrap_or("");
        if role != "user" && role != "assistant" { continue; }
        let msg = v.get("message").unwrap_or(&v);
        if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
            for block in arr {
                if let Some(t) = block.get("text").and_then(|x| x.as_str()) {
                    let t = t.trim();
                    if t.len() >= 8 { texts.push(format!("{}: {}", role, t.chars().take(280).collect::<String>())); }
                }
            }
        } else if let Some(t) = msg.get("content").and_then(|x| x.as_str()) {
            let t = t.trim();
            if t.len() >= 8 { texts.push(format!("{}: {}", role, t.chars().take(280).collect::<String>())); }
        }
    }
    if texts.is_empty() { return None; }
    let tail: Vec<String> = texts.iter().rev().take(8).rev().cloned().collect();
    Some(format!("LAST SESSION CONTEXT (backstop — agent did not journal):\n{}", tail.join("\n")))
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
