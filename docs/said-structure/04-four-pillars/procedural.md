# Pillar — Procedural

Action recipes. Trigger + ordered steps + outcome. This is Voyager-style skill memory: given a condition, run these steps, expect this outcome.

## What goes here

- "How I deployed the MCP server yesterday" — for agents that want to re-run it
- Runbooks, troubleshooting guides
- Tool-use sequences that succeeded (or failed — both are informative)
- Playbooks captured mid-session

## Writer API

```rust
brain.remember_as_procedural(
    Some("proc_deploy"),
    "deploy the said-mcp server to production",
    &[
        "build --release",
        "rsync the binary to the prod box",
        "systemctl restart said-mcp",
        "run smoke test",
    ],
    Some("success — deploy took 2m3s"),
    vec!["team:platform".to_string()],
);
```

Body format (SCA still indexes this like normal text; the structure is a convention):

```
TRIGGER: deploy the said-mcp server to production
STEPS:
  1. build --release
  2. rsync the binary to the prod box
  3. systemctl restart said-mcp
  4. run smoke test
OUTCOME: success — deploy took 2m3s
```

## Tags applied automatically

- `pillar:procedural`
- `procedural:outcome=<first_word>` — parsed from the outcome string's first token. Canonical values: `success`, `failure`, `partial`, `unspecified`.
- Any `extra_tags` the caller passed (e.g. `team:platform`)

## Retrieval ranking (planned)

From the architecture spec:

```
score = task_match(query, trigger_conditions) × success_rate
```

**Current reality** — Procedural frames rank via the standard SCA + BM25 pipeline. Agents can post-filter by `procedural:outcome=success` to prefer recipes that worked. Explicit `task_match × success_rate` ranking is planned for the `rerank_by_pillar` expansion.

## Typical queries

```
said ask "how do I deploy said-mcp?"
said search pillar=procedural query="deploy"
said search pillar=procedural query="outcome=success deploy"
```

## Where Procedural frames come from today

- **Explicit** — agent realized a sequence worked, calls `remember_as_procedural`
- **Admin** — operator persists a runbook
- **Migration** — mem0 `procedure` / `plan` / `action` / `recipe` categories auto-route to Procedural

## No auto-synthesis

Like Semantic, Procedural frames are caller-written under BYO-LLM. The dream layer doesn't synthesize recipes from raw Episodic turns (it can't reliably parse "click here, then here, then here" without language understanding). An agent that wants to capture a recipe captures it explicitly.

## How to test

```rust
let fid = sf.remember_as_procedural(
    Some("proc_test"),
    "restart the build daemon when it hangs",
    &["kill -9 $(pidof builder)", "systemctl start builder", "wait for 'ready' in logs"],
    Some("success — 45 second recovery"),
    vec![],
);

let meta = sf.frames.get_meta("proc_test").unwrap();
assert_eq!(meta.pillar, Pillar::Procedural);
assert!(meta.tags.iter().any(|t| t == "pillar:procedural"));
assert!(meta.tags.iter().any(|t| t == "procedural:outcome=success"));
```

Functional verification in [`examples/pillar_writers_probe.rs`](../../../crates/sca-core/examples/pillar_writers_probe.rs).

## See also

- [Row 46 Procedural writer](../05-features/row-46-procedural.md)
- [Row 48 Migration adapters](../05-features/row-48-migration.md) — mem0 procedure → Procedural
