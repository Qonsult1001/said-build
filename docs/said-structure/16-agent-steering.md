# Agent steering — telling Claude when/what to use `.said` (nudge-faithful, removal-safe)

**Status:** DESIGNED, not yet built. This records the mechanism, why it's removal-safe, how it bundles,
the end-to-end test plan, and one open experiment to run. Source of truth before writing code.

## The problem

`.said` is a memory/retrieval tool. For it to earn its keep, the coding agent (Claude Code) must
actually *use it at the right moment* — e.g. before grepping the codebase, query `.said` to find code
by meaning + recall past fixes (proven 99.6× leaner than grep+read in `test_bug_location_e2e.rs`). But:

1. **An MCP server alone cannot steer the agent.** MCP only *exposes* tools the model *may choose* to
   call. It can describe itself, but it can't intercept "the agent is about to grep" and act.
2. **CLAUDE.md is the wrong mechanism** and is DISQUALIFIED: once `.said` guidance is committed to
   `./CLAUDE.md` it lives in git history forever — deleting the line still leaves it in past commits.
   It cannot be cleanly removed when `.said` is uninstalled. (User's explicit requirement: steering
   must leave NO trace when `.said` is removed.)
3. **Start-of-session instructions fade.** nudge's measured insight: as a session runs long, an
   instruction from the system prompt / skill becomes harder for the agent to remember and follow.
   Enforcement must surface *at the decision point*, not once at the start.

## The mechanism — a PreToolUse hook (faithful to nudge)

Investigated nudge (attunehq/nudge, Apache-2.0) at the source level. Key facts that shape this design:

- **nudge is NOT an MCP server.** It's a standalone binary the agent invokes as a `PreToolUse` /
  `UserPromptSubmit` **subprocess hook**: the agent pipes the tool-call JSON on stdin, the binary
  returns a decision JSON on stdout (`permissionDecision: deny | allow` + optional `additionalContext`
  / `updatedInput`). Steering happens OUTSIDE MCP, at the hook surface.
- **Supports Claude Code + Codex only**, via two hand-written per-agent adapters (different stdin JSON,
  different settings file). No universal layer — Cursor/Gemini/Windsurf/Aider would each need a new
  adapter. (We do **Claude Code first**.)
- **Removal-safe by construction:** registers the hook into `.claude/settings.local.json` (gitignored,
  backed up to `*.bak`), and **never edits CLAUDE.md/AGENTS.md** — bootstrap guidance lives in a
  bundled *skill*. There is no `uninstall` command; removal is a documented manual file edit, but it
  leaves NO git trace.

### `.said`'s design (Claude Code first)

1. **`said hook` subcommand** — reads the Claude `PreToolUse` JSON from stdin, and on a code-search
   tool-call (the Claude `Grep` tool, or a `Bash` grep/rg/ripgrep command), queries `.said` and returns:
   - **`allow` + `additionalContext`** carrying the relevant `.said` recall (symbols/snippets/past
     fixes). The agent gets the memory *injected at the moment it was about to search* — it often no
     longer NEEDS to grep, but is **not blocked** (nudge's preferred "fail-open, tap-on-shoulder"
     pattern). This is the DEFAULT behaviour (see the open experiment below).
   - Fail-open: if `.said` errors or has nothing relevant, return plain `allow` (no context) so the
     agent is never stuck.
   - NOTE: unlike nudge (which hardcodes `Write|Edit|WebFetch|Bash` and can't match the `Grep` tool),
     `said hook` is OUR hook — its matcher can include `Grep` directly. We control the vocabulary.
2. **`said setup` (opt-in at install)** — registers the hook in `.claude/settings.local.json`
   (PreToolUse matcher `Grep|Bash`), backs up any existing file, and bundles a `said` skill under
   `.claude/skills/said/` with the bootstrap guidance ("before searching code, .said memory is queried
   automatically; you can also call the said MCP tools directly"). Idempotent, merges without clobber.
3. **MCP layer (complementary, already partly there)** — the said-mcp server's `instructions` field +
   each tool's `description` tell the model `.said` exists and when to call it directly. Auto-loaded on
   connect, auto-removed on disconnect, zero git trace. This handles the "model chooses to call .said"
   path; the hook handles the "agent was about to grep" path.

### Removal-safety summary
| Surface | Removed when `.said` uninstalled? | How |
|---|---|---|
| MCP instructions + tool descriptions | ✅ automatic | server disconnects |
| `said hook` registration | ✅ no git trace | delete from `.claude/settings.local.json` (gitignored) |
| bundled `said` skill | ⚠️ manual | documented `rm -r .claude/skills/said` |
| CLAUDE.md | n/a — **NEVER WRITTEN** | (the whole point) |

## Bundling — make installing `.said` user-friendly (nudge's model)

The binary install installs only the binary. A separate **opt-in** `said setup` (run once per project)
does the agent integration, exactly like `nudge claude setup`:
- writes `.claude/settings.local.json` (the gitignored hook registration, `*.bak` backup)
- bundles `.claude/skills/said/` (bootstrap guidance — NOT CLAUDE.md)
- prints a clear "what was installed / how to remove" summary

User flow:
```
# install said binary + MCP (existing)
# then, opt in to agent steering:
said setup            # registers the PreToolUse hook + bundles the said skill (Claude Code)
# remove:
said setup --remove   # (to build) deletes the hook entry + skill; no git trace
```

## End-to-end test plan (the 3 layers nudge uses)

1. **Deterministic** — unit tests on `said hook`'s decision logic: given a PreToolUse JSON for a
   `Grep`/`Bash`-grep call, it returns `allow` + `additionalContext` containing the right recall; given
   an unrelated tool call, it returns plain passthrough; on `.said` error it fails open.
2. **Hook simulation** — pipe a real Claude `PreToolUse` JSON to `said hook` and assert the emitted
   decision JSON (the nudge `printf '{...}' | nudge claude hook` pattern):
   ```
   printf '{"hook_event_name":"PreToolUse","tool_name":"Grep","tool_input":{"pattern":"session expiry"}}' \
     | said hook
   # expect: {"hookSpecificOutput":{"permissionDecision":"allow","additionalContext":"<.said recall>"}}
   ```
3. **Live-agent** — in a disposable repo, `claude --print --permission-mode bypassPermissions
   '<task that would normally grep>'` and verify (a) the hook fired, (b) `.said` recall was injected,
   (c) the agent reached the answer with fewer file reads / tokens than the no-hook baseline.

## OPEN EXPERIMENT (run before finalising the default)

**Does "block + redirect (query .said first)" actually beat "inject recall + let grep proceed"?**
We default to inject-and-proceed (least hostile, token-saving), but this is an empirical question:
- **inject + proceed**: agent gets `.said` context, MAY still grep → safe, but agent might ignore the
  injected context and grep anyway (no token saving).
- **block + redirect**: hard-deny grep, force the `.said` MCP call first → guarantees `.said` is used
  (max token saving) but adds a round-trip and can misfire when grep is genuinely the right tool.

Measure on the bug-location corpus: tokens-to-locate + accuracy + "did the agent actually use .said"
for each mode. Pick the default from data, not assumption. (User flagged this explicitly.)

## Why this is the right design
- **Removal-safe** — the user's hard requirement; nothing in committed git, never CLAUDE.md.
- **Faithful to nudge** — hook-shells-out-to-binary returning allow/deny+context; Apache-2.0 permits
  adapting the approach (and the approach itself isn't copyrightable).
- **Two complementary surfaces** — MCP instructions (model chooses to call) + PreToolUse hook (catches
  the about-to-grep moment the model would otherwise forget). The hook solves the long-session fade
  that instructions alone can't.
- **Claude Code first**, per-agent adapters added later (Codex next) — matches nudge's proven shape.
