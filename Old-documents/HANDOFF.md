# HANDOFF — session marker 2026-04-23

This file is overwritten at each inter-agent handoff. It is the single source of truth for "what just happened / what's still open / where to pick up."

## What just committed

Three commits landed on `claude/echo-personality-evolution-GpQ6v`:

| Hash | Type | Summary |
|---|---|---|
| `bfb64e4` | workspace | `Cargo.toml` — excluded `research/memvid` (bit-rotted edition-2024 crate, 29 errors in code nothing depends on). `Cargo.lock` — forge-phase dep resolution. |
| `a4b3c53` | docs | **The coherent `docs/said-structure/` commit**. Full 14-chapter blueprint: overview, file format, core subsystems (incl 3.8 latent space with measured 0.08 ms encode + 0.07-2 ms search, 3.9 graph layer), four pillars, feature catalogue (rows 30-50 + forge), ingestion plugins (docs/ocr/whisper/code/lsp + openapi), CLI reference (all verbs + snapshot/sandbox/forge-* deep docs), MCP reference (all 31 tools + forge-tools), Cargo features (+ forge), benchmarks (MTEB/LoCoMo/BEIR/chambers/realworld/competitor-matrix), known limitations, roadmap (incl integrations Q2/Q3/Q4 + horizon + forge follow-ups), chapter 13 integrations architecture, chapter 14 novel mechanisms (9 shipped incl 14.14 byte-exact restore + 5 horizon). Plus `SAID_MVP_PLAN.md` updates. |
| `54925bc` | cleanup | Removed `test_code_modules/`, `test_vivier/`, `docs/superpowers/test/`, `dream_test.said`, `test_art_rescue.py`, `test_proximity.py`. Unrelated repo hygiene. |

## What's still uncommitted (deferred — your call)

Left as-is because the intent isn't unambiguous. You decide commit vs gitignore vs delete.

### Root-level marketing artifacts
- `DEMO.md`
- `SAID_ENTERPRISE_PITCH.md`
- `SAID_FINAL_RESULTS.md`

These look like one-off marketing docs. Move to `docs/marketing/`? Delete? Gitignore? Your call.

### Placeholder / runtime state
- `.brain.said`, `brain.said` — onboarding placeholders created by MCP. Gitignore.
- `.claude/scheduled_tasks.lock` — Claude Code runtime lockfile. Gitignore.
- `__pycache__/` — Python cache. Gitignore.
- `check_errors.txt`, `router_full.log` — transient output. Gitignore or delete.
- `out/` — build / test output. Gitignore.

### Orphan docs
- `docs/SAID_FEATURE_CATALOGUE.md` — the older monolithic catalogue we partially migrated into `docs/said-structure/`. Delete (content absorbed) or keep as legacy?
- `docs/competitor_benchmark.json` — raw output from `competitor_bench`. Probably move to `docs/said-structure/10-benchmarks/data/` if worth keeping.
- `docs/said-memory-architecture.md` — unclear provenance. Check whether it overlaps with `docs/said-structure/` and delete / migrate.

### Possibly unintentional
- `SAID-LAM-private` submodule — modified (`m`). Check if a submodule bump is intentional.
- `research/final_solution_formula_final.py` — modified. Revert or keep?
- `research/Untitled`, `research/locomo/`, `research/mem0/` — untracked. These look like big research dumps; decide whether any are meant to be committed or gitignored.
- `docs/superpowers/vivere/` — untracked, likely a paste of test artifacts that shouldn't be in the repo.

## Stash status — unchanged

```
stash@{0}  On main: parallel agent WIP — preserve for later pop
stash@{1}  On main: pre-path1-revert-point: streaming + fast compact baseline
```

`stash@{0}` is still there. HEAD sca-core is clean. The stash's 3-4 known method-resolution errors (`index_batch`, `BrainMode`/`mode`, `remember_with_pillar`) have not been resolved. Same coordination rules:

1. Do NOT pop until your current phase ships and HEAD is green.
2. When you're ready, follow the playbook in the discussion log / session memory — verify HEAD green, inspect stash with `git stash show stash@{0} --stat -p`, pop, expect the 3-4 errors, fix them, run `cargo test -p said-forge` to confirm nothing forge-related broke, then commit.

## Forge status

`main` has your forge MVP complete and end-to-end tested at commits `546f7a3` through `dd25d93`. Nothing in my three commits touched forge code. The docs commit DOES include your forge-specific pages:

- `docs/said-structure/05-features/forge.md`
- `docs/said-structure/06-ingestion-plugins/openapi.md`
- `docs/said-structure/07-cli-reference/forge-{load,list,show,status,run,reset}.md`
- `docs/said-structure/08-mcp-reference/forge-tools.md`
- References inside `09-cargo-features.md`, `11-known-limitations.md`, `12-roadmap.md`, top-level README

Plus forge follow-ups are tracked in [`12-roadmap.md` § Forge](said-structure/12-roadmap.md#forge):
- Forge MCP run (tokio::Mutex port)
- Forge edit preservation
- Forge hard confirm gate
- Forge multi-directive
- Forge MCP tag filter
- Milestone C — sandbox/runtime

## Open threads you may want to tackle

In no particular order:

1. **Integrations Q2 kickoff.** `said-watch` is the first planned crate per [13-integrations.md](said-structure/13-integrations.md#q2--six-offline-integrations-now--3-months). LEANN's [`apps/`](../research/LEANN/apps/) is the porting reference. Offline-first; no OAuth. ~1 week.
2. **Forge follow-ups.** Roadmap items above. Pick whichever matches the current phase.
3. **Horizon mechanisms.** Four promoted into roadmap from chapter 14 — latent PageRank (smallest effort), DP (most publishable), holographic K-view (infra hook exists), crystalline annealing (most elegant).
4. **Stash pop.** When safe.
5. **Decide on the ambiguous files above.**

## Branch context

Current: `claude/echo-personality-evolution-GpQ6v`. Check whether you intended to push to `main` directly or merge via PR. I did not push — your call.

## Who to talk to if confused

- [docs/said-structure/README.md](said-structure/README.md) — the master index for the 14 chapters
- [docs/said-structure/12-roadmap.md](said-structure/12-roadmap.md) — what's planned
- [docs/said-structure/11-known-limitations.md](said-structure/11-known-limitations.md) — what's broken / paired with roadmap items
- [docs/said-structure/13-integrations.md](said-structure/13-integrations.md) — integration architecture + Q2/Q3/Q4 plan
- Memory system under `C:\Users\Carter\.claude\projects\g--development-SAID-ECHO\memory\` — longer-term project history

## Next handoff

When you wrap your next phase, overwrite this file with the new state. Keep the same shape: committed / uncommitted / stash / open threads.
