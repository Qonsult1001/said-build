# Row 46 — Procedural pillar writer

**Status:** ✅ shipped 2026-04-22

## What it does

`SaidFile::remember_as_procedural(trigger, steps, outcome, tags)` writes a structured action-recipe frame into the Procedural pillar. Voyager-style skill memory: trigger condition → ordered steps → outcome.

## Where it lives

[`SaidFile::remember_as_procedural`](../../../crates/sca-core/src/said_file.rs). Routes through `remember_with_pillar(Pillar::Procedural, ...)` which now uses `FrameStore::set_pillar` so the on-disk pillar byte is correct.

## Inputs

```rust
pub fn remember_as_procedural(
    &mut self,
    doc_id: Option<&str>,
    trigger: &str,
    steps: &[&str],
    outcome: Option<&str>,
    extra_tags: Vec<String>,
) -> u64;
```

Example:
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

## Outputs

Body format:
```
TRIGGER: deploy the said-mcp server to production
STEPS:
  1. build --release
  2. rsync the binary to the prod box
  3. systemctl restart said-mcp
  4. run smoke test
OUTCOME: success — deploy took 2m3s
```

Tags applied:
- `pillar:procedural`
- `procedural:outcome=<first_word>` — parsed from the outcome. Canonical values: `success`, `failure`, `partial`, `unspecified`.
- Caller-supplied tags

Persisted `FrameMeta.pillar == Pillar::Procedural`.

## How to test

From [`examples/pillar_writers_probe.rs`](../../../crates/sca-core/examples/pillar_writers_probe.rs):

```rust
let fid = sf.remember_as_procedural(
    Some("proc_test"),
    "restart the build daemon when it hangs",
    &["kill -9 $(pidof builder)", "systemctl start builder", "wait for 'ready'"],
    Some("success — 45 second recovery"),
    vec![],
);

let meta = sf.frames.get_meta("proc_test").unwrap();
assert_eq!(meta.pillar, Pillar::Procedural);
assert!(meta.tags.iter().any(|t| t == "pillar:procedural"));
assert!(meta.tags.iter().any(|t| t == "procedural:outcome=success"));
```

Shipped probe verified all assertions pass.

## How to extend

New outcome status: just use a new word as the first token. `Some("pending — waiting on approval")` produces tag `procedural:outcome=pending`. No code change needed.

New structural field (e.g. add `PRECONDITIONS:` before `STEPS:`): edit body builder in `remember_as_procedural`. Tag convention can be extended similarly.

## Known limitations

- Retrieval ranking doesn't yet do `task_match × success_rate` from the architecture spec. Procedural frames rank via standard SCA + BM25. Agents can post-filter by `procedural:outcome=success` to prefer recipes that worked.

## See also

- [Procedural pillar](../04-four-pillars/procedural.md)
- [Row 47 Code writer](row-47-code.md) — sibling writer with same pattern
- [FrameStore::set_pillar fix](../03-core-subsystems/3.4-framestore.md)
