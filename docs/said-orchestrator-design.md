# .said as the Closed-Loop Coding Orchestrator — Design

**Status:** design only (no code yet). **Goal set by the user:** `.said` becomes the
orchestrator that drives ANY external LLM through the full Claude-Code lifecycle —
plan → design → structure → code → test → repair-loop → memory — using built-in,
editable standard prompts + project memory. Point it at any model; if the model
can't do it alone, `.said` carries the context/prompts that make up the gap.
**"We are the memory AND the playbook. The LLM is the hands. The gate is truth."**

This is the Cursor/Claude-Code workflow, made model-agnostic and portable.

## Will it work? Yes, under one boundary

`.said` orchestrates + owns memory/prompts. The LLM reasons. **The build/test gate
is the sole ground truth** — `.said` never declares code correct; the gate does.
This is the same non-negotiable from [[fix-replay-latent-research]]: memory/prompts
PROPOSE; the gate VERIFIES. It stops the loop amplifying its own mistakes.

## Two layers (keep them clean)

1. **The `.said` FILE** — the portable, single-file memory. Unchanged in spirit:
   stores iterations (10-section notes), verified fixes, project state, conventions,
   workflow commands, errors-to-avoid. Recalls by problem shape (intent separation,
   proven). This is the moat and it already works.
2. **The `.said` ORCHESTRATOR** — a thin driver (new) that runs the loop using the
   file: recall → prompt → call LLM → apply → gate → write back → repeat. The file
   stays pure data; the orchestrator is a process that reads/writes it. Do NOT bloat
   the file format with runtime concerns.

## The closed loop (the core state machine)

```
              ┌─────────────────── .said memory (file) ───────────────────┐
              │  project state · conventions · past iterations · fixes ·   │
              │  workflow cmds · errors-to-avoid · design decisions        │
              └────────────▲───────────────────────────────┬──────────────┘
                           │ recall (whole story)           │ write back (outcome)
                           │                                 │
   user task ─► [PLAN] ─► [DESIGN] ─► [CODE] ─► [TEST/GATE] ─► green? ─► [LEARN] ─► done
                  │          │          │            │          │ no
                  └── each phase: .said fills its STANDARD PROMPT with recalled
                      context, sends to the external LLM, applies the result,
                      records what happened ──────────────────┘ (repair loop)
```

Loop exit = the GATE is green (build + test pass), mirroring Claude's "verify before
complete". On failure, the error + attempt go back into `.said` and the repair prompt
(carrying "Errors & Corrections") drives the next LLM call. Bounded retries; on
exhaustion, stop and surface — never merge red.

## Phases = editable standard prompts (the "playbook")

Each phase has a default prompt (reverse-engineered from Claude Code — see
[[claude-code-reverse-engineering]]), stored in `.said` and OVERRIDABLE (the user's
"prompting must be editable in .said"). Defaults live in `sca_core::coding_memory`
(extend the existing module). Phases:

- **PLAN** — read-only exploration + todos. (Claude: plan mode, "DO NOT edit yet".)
- **DESIGN/STRUCTURE** — architecture, file layout, conventions (pull project's
  stored conventions so a NEW project gets standards, an OLD project gets ITS own).
- **CODE** — surgical edits via `said edit`; Claude's "don't over-engineer / don't
  add unasked features / read before editing" rules baked in.
- **TEST** — run the project's known build/test commands (the "Workflow" section of
  the iteration memory); this is the gate.
- **REPAIR** — feed the gate's error back; carry "Errors & Corrections" so failed
  approaches are never retried.
- **LEARN** — on green, write the 10-section iteration note back to `.said`
  (already built: `learn-fix --note-file`).

Each phase prompt is filled with: the standard prompt + recalled project memory +
the current task + prior-attempt context. This is "tell the whole story, never start
from scratch."

## What `.said` must gain to orchestrate (the new component)

The file can't do these; a thin orchestrator binary/mode must:
1. **Call an external LLM** — pluggable HTTP client (OpenAI/Groq/Ollama/any). Model
   is config, not code. This is what makes it model-agnostic.
2. **Apply actions** — already have `said edit` (surgical, no whole-file rewrite).
3. **Run the gate** — shell out to the project's build/test commands.
4. **Drive the state machine** — the loop above, with bounded retries.
5. **Read/write the memory** — already have (`recall-fix`/`learn-fix`/`ask`).

Open question for build time: is the orchestrator a new subcommand of `said`
(`said drive --task "..."`), a separate small binary, or an MCP surface the client
calls step-by-step? (Leaning: a `said drive` subcommand + expose each phase as an
MCP tool so a client can either hand `.said` the wheel OR step it manually.)

## Why this beats Cursor/Claude for the user

- **Model-agnostic:** point it at any LLM. Cursor/Claude are locked to their model.
- **Portable memory:** the whole project's story is one file, moves anywhere.
- **The method, not the model:** even a weak LLM gets Claude-quality workflow because
  `.said` supplies the plan/design/code/test prompts + the full context.
- **Closed loop with real ground truth:** the gate, not vibes, ends the loop.

## Honest risks / boundaries

- `.said`-the-file must NOT become `.said`-the-runtime. Keep the orchestrator a
  separate concern that USES the file. The portable-memory moat dies if the format
  absorbs runtime state.
- Orchestrating = `.said` now executes edits + shell. The `said edit` "no whole-file
  rewrite" guarantee + the gate are the safety rails; keep them absolute.
- A weak LLM still fails some tasks. `.said` raises the floor (context + prompts +
  replay of known fixes); it does not make a bad model infallible. The gate catches
  the failures; the loop retries or stops. No false "done".
- Scope: this is the biggest piece yet. Build in stages — playbook prompts first,
  then one vertical (bug-fix) end-to-end with a single LLM provider, then generalize.

## Staged build (proposed, for after this doc is agreed)

1. Phase-prompt library in `.said` (editable defaults for plan/design/code/test/repair).
2. LLM client abstraction (one provider first, e.g. Groq/OpenAI-compatible).
3. `said drive --task` running ONE vertical (bug fix) closed-loop on a real repo,
   gate-verified, learning the iteration on green.
4. Generalize to new-project / existing-project / design / feature workflows.
5. MCP surface so external clients can step the loop or hand over the wheel.

Ground truth stays the gate at every stage. Memory + prompts only propose.

---

## Implementation status & LIVE RESULTS (built)

The full loop is BUILT and LIVE-PROVEN. Crates (clean separation, conforms to
docs/said-structure/13-integrations.md Rule 2 — core `.said` never calls an LLM):

- **said-prompts** (`coding::`) — the playbook: phase prompts (plan/design/code/
  test/repair) + the 10-section iteration template + the LEARN extraction prompt.
- **sca-core** — project memory (recall the verified iteration; intent-isolated).
- **said-orchestration** (separate process, the `said-think` pattern) — the phases
  (one file per step under `steps/`), the loop, `gate.rs` (build/test = sole
  judge), `apply.rs` (anchored edits, no whole-file rewrite), `source.rs` (real
  source + vetted anchor menu), `compress.rs` (Claude-style note compression),
  `config.rs` (`[llm]` TOML, `${ENV}` keys). Binary: `said-orchestrate`.

### Staged build — status
1. Phase-prompt library — ✅ done (in said-prompts).
2. LLM client — ✅ via said-llm (BYO: Anthropic / OpenAI-compatible / Claude-CLI),
   incl. a `json_object` mode added so free-form coding output works on Groq/
   OpenRouter (strict `json_schema` is rejected there for arbitrary code).
3. One vertical, gate-verified, learn-on-green — ✅ LIVE-PROVEN.
4. Generalize to other workflows — ✅ Claude itself uses ONE loop + model
   adaptation (reverse-engineered, see memory/claude-code-reverse-engineering),
   so the single loop generalizes; proven on a NEW task shape (HTML/JS, below).
5. MCP surface — ⏳ not yet (the phases could be exposed via said-mcp `prompts/*`).

### How anchor hallucination is prevented (the autonomy fix)
Live testing showed the only real autonomy gap was edit anchors. Mirrors Claude
Code's edit safety (Read-before-Edit + exact-match) + Advisory's RealEndpointAnchors:
- `source.rs` injects the ACTUAL target file (line-numbered) into code/repair
  context, PLUS a vetted ANCHOR MENU of safe, complete statement-ending lines —
  the model picks a real line instead of reconstructing (and truncating) one.
- `apply.rs` resolves anchors whitespace-tolerantly and across MULTI-LINE
  statements (maps a collapsed statement to its ending line), and REJECTS
  mid-statement insert-after (would split a statement → build break). Apply
  failures feed the repair loop instead of aborting.

### LIVE model matrix — all GREEN, fully autonomous (no anchor handed)
Task: add an anonymous GET endpoint to a copy of Advisory's C# Program.cs; gate =
`dotnet build`. Five models across two providers:

| Model | Provider | Result |
|---|---|---|
| claude-opus-4.8 | OpenRouter | ✅ green, 1st attempt |
| kimi-k2.5 (Composer's open base) | OpenRouter | ✅ green, 1st attempt |
| kimi-k2.7-code | OpenRouter | ✅ green (reliable once the anchor menu landed) |
| gpt-oss-120b | Groq | ✅ green, 1st attempt |
| gpt-oss-20b | Groq | ✅ green, 1st attempt |

**The orchestrator drives open AND closed models to a green gate, autonomously.**
The vetted-anchor menu made the variable models as reliable as Opus 4.8.

### Task-shape generality — proven
Same orchestrator, totally different artifact + gate, ZERO code change: an HTML/JS
Pong game where the model implements `stepBall` physics; gate = `node pong.test.js`
(6 logic assertions). gpt-oss-120b → green, 1st attempt, independently verified.
Only `--build` and `--task` differ from the C# runs.

### Key management
`said-orchestrate --llm-config <toml>` reads provider/model/base_url/api_key, with
`api_key = "${ENV_VAR}"` placeholder expansion (forge's pattern). Keys live in env/
secret store, never plaintext in the file. Falls back to env vars.

### Next: K2.5 vs Cursor Composer (controlled)
The pre-registered protocol (Advisory docs/k2.5-said-vs-composer-test.md) holds the
base model constant (Kimi K2.5) and varies only the harness: `.said` orchestration
(no RL) vs Composer (K2.5 + Cursor's RL). The gate judges. A tie or win for `.said`
validates portable, model-agnostic memory-orchestration as a Composer alternative.
