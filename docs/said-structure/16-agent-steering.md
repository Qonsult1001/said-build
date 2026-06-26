# Agent steering — telling Claude when/what to use `.said` (nudge-faithful, removal-safe)

**Status:** BUILT (Claude Code). `said hook` + `said setup`/`--remove`/`--dry-run` ship in said-cli;
the decision core is `sca-core::steering`; the guidance text is `said-prompts::steering`. Tested:
core 3 unit + 4 e2e (`test_steering_e2e`), prompts 3 (`said-prompts::steering::tests`), CLI 5 e2e
(`test_steering_cli`). One open experiment (inject-vs-block, below) is still to run.

Try it:
```
said setup                 # opt-in: registers the PreToolUse hook in .claude/settings.local.json
                           # (gitignored, *.bak backup) + bundles .claude/skills/said/SKILL.md
printf '{"hook_event_name":"PreToolUse","tool_name":"Grep","tool_input":{"pattern":"…"}}' | said hook
said setup --remove        # clean removal — no git trace
```

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

## EXPERIMENT RESULT — inject vs block (RESOLVED, see `test_steering_experiment.rs`)

**Does "block + redirect" beat "inject + proceed"?** Measured on the bug-location corpus (one bug
among ~120 filler files; the symptom query shares no identifier with the bug). Both modes carry the
SAME recall (the difference is allow vs deny, not the payload):

| | INJECT (default) | BLOCK (`--mode block`) |
|---|---|---|
| Decision | allow + context | deny + redirect |
| Hook payload | ~318 chars | ~366 chars |
| Avoids the whole-project read (~10.5K chars) | only IF the agent trusts the context & skips grep | DETERMINISTICALLY |
| Worst case | agent greps anyway → missed saving (still correct) | recall was wrong → one extra round-trip |

**Decision (data-driven):** default = **INJECT** — it is FAIL-SAFE: it never blocks a legitimate
grep, and when the recall is good the agent skips the grep anyway; its only downside is a *missed
saving*, never a wrong answer. **BLOCK is an opt-in power mode** (`said hook --mode block`) for
token-critical workflows that accept the occasional extra round-trip for guaranteed `.said`-first. The
fail-open lexical-grounding gate makes BLOCK safer than a naive block — it only denies when `.said`
has a GROUNDED hit; an off-topic search passes through (never blocked) in BOTH modes.

## Why this is the right design
- **Removal-safe** — the user's hard requirement; nothing in committed git, never CLAUDE.md.
- **Faithful to nudge** — hook-shells-out-to-binary returning allow/deny+context; Apache-2.0 permits
  adapting the approach (and the approach itself isn't copyrightable).
- **Two complementary surfaces** — MCP instructions (model chooses to call) + PreToolUse hook (catches
  the about-to-grep moment the model would otherwise forget). The hook solves the long-session fade
  that instructions alone can't.
- **Claude Code first**, per-agent adapters added later (Codex next) — matches nudge's proven shape.
