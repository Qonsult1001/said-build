# Memory injection — the nudge pattern (the mistake to NEVER repeat)

How `.said` injects recalled memory into a coding agent so the agent actually **uses** it. The reference
implementation is **nudge** (attunehq/nudge), cloned locally at `SAID-ECHO/research/nudge` — **always
consult it before changing any injection code.** This doc records the exact mistake we kept making and
the proven pattern from nudge's source.

## THE RULE (one sentence)

**Inject recalled memory as PLAIN FACTS, stated as-is. Never wrap it in an authority claim or an
imperative.** ("Here is prior work: …" — yes. "This is a gate-verified solution, REUSE it exactly" —
NO.)

## The mistake (made repeatedly — stop making it)

We kept framing the injection as a strong, authoritative claim:

```
<verified_memory source=".said" match="0.84">
A previous gate-verified solution to a problem of THIS shape — it was applied and confirmed working.
IMPORTANT: REUSE the verified solution's structure and core logic EXACTLY. Do NOT re-derive it.
…
</verified_memory>
```

**Why it fails:** an assertive claim of authority ("gate-verified", "REUSE this exactly", "do NOT
re-derive") reads to the model as **a claim to verify**, not a fact to use. On the trusted
UserPromptSubmit channel this trips the model's injection-skepticism — Claude **re-investigated the
source code to confirm the claim** instead of using the recall.

**Measured, identical question + brain + registered hook:**

| Injection framing | Turns | Cost | Behaviour |
|---|---|---|---|
| `<verified_memory>` "gate-verified, REUSE this" | 17 | $0.27 | re-investigated the source to verify the claim |
| `<project_memory>` plain facts (the nudge pattern) | **1** | **$0.045** | "Based on the project memory: …" — used it directly |

~6× cheaper, and the rejection disappears — purely from dropping the authority/imperative framing.

## What nudge actually does (verbatim from the cloned source)

`packages/nudge/src/hook/response.rs` — the UserPromptSubmit "Continue" outcome:

```rust
HookOutcome::AddContext { context } => Ok(RenderedHookOutcome::Stdout(context)),
```

The injected context is the note string **printed raw to stdout** — Claude Code turns UserPromptSubmit
stdout into added context. There is **no JSON wrapper, no tag, no meta-claim**. The test fixture
(`response.rs` tests) confirms it: `AddContext { context: "remember this" }` renders to exactly
`"remember this"`.

`packages/nudge/src/hook/evaluate.rs` — how the learned note is assembled (`join_context`):

```rust
fn join_context(primary: String, secondary: Option<String>) -> String {
    match secondary {
        Some(secondary) if !secondary.trim().is_empty() => format!("{primary}\n\n{secondary}"),
        _ => primary,
    }
}
```

Just concatenation. **No framing is added.** The note's own content is the message.

(For the PreToolUse "Warning" path nudge does use a `PreToolUseResponse { system_message,
hook_specific_output: { permission_decision: "allow", additional_context } }` — but that's the
tool-adjacent channel, which the project treats as legacy/distrusted. The trusted path is
UserPromptSubmit = plain stdout context.)

## How `.said` does it now (matches nudge)

`sca-core::steering::render_verified_fix` (commit 5fabe2d) emits a plain factual block — the recalled
learning stated as prior work, no imperative, no "verified/reuse" claim:

```
<project_memory source=".said">
Prior work on a problem of this shape recorded:
<the learned note — TASK / ROOT CAUSE / LEARNINGS, as stored>
</project_memory>
```

`decide()` still does **fix-first recall** (`recall_coding_fix` before the generic `ask`, the
said-orchestration "look at fixes first" rule) — but the *framing* of whatever it injects is plain
facts, per nudge.

## Checklist before touching injection code (do this every time)

1. **Read the nudge source first**: `SAID-ECHO/research/nudge/packages/nudge/src/hook/response.rs` and
   `evaluate.rs`. It is the perfectly-documented reference; do not reinvent the framing from memory.
2. Trusted channel = **UserPromptSubmit**, rendered as `additionalContext` (Claude) / plain stdout.
   Never PreToolUse `additionalContext` for trusted injection (tool-adjacent = distrusted).
3. The injected text is **FACTS, never an imperative or a meta-claim about its own authority.** No "this
   is verified", no "REUSE this", no "do NOT re-derive". State the prior work; let the model decide.
4. Cap the payload (we cap the note at ~2400 chars) — a flood derails small models (Claude's selective-
   injection rule; matches nudge's lean notes).
5. Verify live: with the hook on, a known-answerable question should be answered in ~1 turn citing the
   memory, NOT a multi-turn re-investigation. If turns go UP, the framing is wrong — re-read this doc.
