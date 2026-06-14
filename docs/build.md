# Advisory — Build, Rebuild `.said` Brain, and Session Fix Log

This document covers two things:

1. **How to rebuild the `.said` brain so it works 100%** (every step) — the brain that powers
   token-efficient recall for the agents.
2. **Every fix made in the mutation/Groq/worker work** — so nothing is lost and the same traps don't
   bite again.

---

## PART 1 — Rebuild the `.said` brain (100%, every step)

### Why this matters
`.said` (`Advisory.said`) is the portable brain agents query for context instead of re-reading the whole
repo. A **bad/partial brain** is the #1 cause of "recall returns nothing":

- If built with the wrong/old binary or without the right features, you get **symbols only** (classes,
  methods) but **`grep`/`get` return empty** (no document bodies). Stats show `index_docs: 0`,
  `tracked_docs: 0` — that brain is useless for code recall.
- A **good** brain returns real code from `grep`/`ask`/`get`. Stats show non-zero `active_frames` AND
  the queries below return content.

### The binaries
- **Windows:** `tools/said/said.exe` (used by the local worker on the host).
- **Linux:** `tools/said/said-linux` → baked into the API image as `/app/said` (the in-container Groq
  cycle uses this; the Windows `.exe` CANNOT run in the Linux container).
- Source of binaries: `G:\development\said-build\dist-binaries\` (`said-linux-x64/said`,
  `said-windows-x64/said.exe`, …). Built from `SAID-ECHO/crates` with:
  ```
  cargo build --release -p said-cli --features "code,docs"   # the 'code' feature = AST chunking (REQUIRED)
  ```
  **Without `code`, `said init` won't AST-chunk source and the symbol table stays empty.**

### Step-by-step rebuild (the way that works 100%)

The brain MUST be built with the **same binary version that will query it** (index format must match).
Because the in-container cycle queries with the **Linux** binary, build the brain with the **Linux**
binary too. The reliable path is to build it **inside the running API container** (which has the Linux
`said` + the source mounted read-only at `/workspace`), then copy the result back to the host.

> Note on shells: when running `docker exec` from **Git Bash**, prefix with `MSYS_NO_PATHCONV=1` or
> Git Bash rewrites `/app/said` into `C:/Program Files/Git/app/said` and the exec fails. From
> PowerShell/WSL this isn't needed.

```bash
# 1) Make sure the API container is up (it has /app/said + /workspace mounted).
docker compose -p advisory up -d --no-deps api

# 2) Rebuild the brain INSIDE the container, from the read-only source, into writable /tmp.
#    (/workspace is mounted read-only by design, so init must write the .said elsewhere via --path.)
MSYS_NO_PATHCONV=1 docker exec advisory-api-1 sh -c '
  cd /tmp && rm -rf saidbuild && mkdir saidbuild && cd saidbuild
  cp -r /workspace/src /workspace/tests /workspace/.gitignore .    # copy the source to index
  /app/said init . --path /tmp/new.said --json
'

# 3) VERIFY the brain is good BEFORE trusting it — these MUST return real content, not empty:
MSYS_NO_PATHCONV=1 docker exec advisory-api-1 sh -c '
  echo "stats:";   /app/said stats --path /tmp/new.said --json
  echo "grep:";    /app/said grep MapGet --path /tmp/new.said --json | head -c 200    # must show code, not []
  echo "fact:";    /app/said grep Fact   --path /tmp/new.said --json | head -c 200    # must show a test
  echo "ask:";     /app/said ask "GET endpoints in Program.cs" --path /tmp/new.said --json | head -c 200
  echo "sym:";     /app/said sym GateEngine --path /tmp/new.said --json | head -c 200 # must show file:line
'
#   PASS criteria: grep/ask return "content":"...", sym returns a doc_id with start_line/end_line.
#   FAIL (rebuild is bad): grep returns {"pattern":"MapGet","results":[]} and stats show index_docs:0.

# 4) Copy the verified brain back to the host as Advisory.said.
docker cp advisory-api-1:/tmp/new.said "g:/development/Advisory/Advisory.said"

# 5) Rebuild + redeploy the API image so the NEW brain is baked in (Dockerfile COPYs Advisory.said).
docker compose -p advisory build api
EVOLUTION_ENABLED=true EVOLUTION_REPO=Qonsult1001/advisory \
  PKGFW_GROQ_API_KEY=$KEY GROQ_API_KEY=$KEY \
  docker compose -p advisory up -d --no-deps api

# 6) Confirm the baked brain answers in-container (same checks as step 3, on /app/Advisory.said).
MSYS_NO_PATHCONV=1 docker exec advisory-api-1 sh -c '/app/said grep MapGet --path /app/Advisory.said --json | head -c 120'
```

### Correct query flags (these tripped us up)
- The brain-location flag is **`--path`**, NOT `--said`. (`said sym X --path Advisory.said --json`.)
- `said` auto-detects the brain if run from the dir containing it and `--path` is omitted.
- Recall the agent should prefer, in order: `said sym <Name>` (exact symbol → range) →
  `said get <doc_id>` (the body) → `said grep "<text>"` (exact) → `said ask "<concept>"` (semantic).

### Host-side (Windows worker) rebuild — for the claude-cli/cursor-cli worker
The local worker uses the Windows `said.exe`. It rebuilds the brain on its own via `build_context`:
```
tools/said/said.exe init        # single init; do NOT also `add --dir` (that corrupted the SCA index)
tools/said/said.exe stats --json # posts to the dashboard
```
Same PASS/FAIL criteria. A single `init` = correct; init + redundant `add` = broken recall.

---

## PART 2 — Every fix in this session (so nothing is lost)

Chronological, with the PR that landed each. All on `main`.

### Worker / mutation cycle reliability
| PR | Fix |
|----|-----|
| #54 | **Rate-limit-aware retry** in the worker: on a Claude rate-limit / out-of-credits marker, back off 30s → 60s, then report + reset the run cleanly (don't fake "no change"). |
| #55 | **Capture cycle output at the source**: `claude \| tee $tmp \| stream_activity` — the parser sat between claude and the capture file and could blackhole output. Move `tee` to the source so the raw stream is always captured. Plus `< /dev/null` so `claude -p` doesn't block on stdin. |
| #56 | **Bypass the mutate-ide hour-gate for clicked tickets**: `mutate-ide.sh setup` only ran at hours 0,4,8,12,16,20; off-hours it printed `SKIPPED` and did nothing. Worker now exports `FORCE_RUN=true MUTATE_HOURS="*"` so a ticket the operator clicked runs now. |
| #57 | (later reverted by #58) tried an isolated `CLAUDE_CONFIG_DIR` — it **broke** `/mutate` (skill didn't load → 0 output). |
| #58 | **Revert the isolated config** — the default Claude config correctly exposes the `/mutate` skill. |
| #59 | **Write cycle output straight to file** (decouple from the live parser) — the parser's per-line API POSTs could backpressure the pipe and leave `$tmp` empty. |
| #60 | **Use the operator's interactive Claude login when no `.env` token**: the `.env` `CLAUDE_CODE_OAUTH_TOKEN` had **expired** (401). The worker runs under WSL (different HOME) so the Windows login was invisible. Fix: point `CLAUDE_CONFIG_DIR` at the Windows `.claude` profile so WSL's claude reads the same live credentials — no token to expire. |
| #62 | **Remove the slow per-cycle self-test diagnostic** (it added a full ~30–60s claude round-trip every run); leaner `claude_ready` probe with `< /dev/null` + 401 detection. |
| #63/#68 | **Release gate**: gate on PR `.state` alone (a null `.mergeable` made jq return empty → "PR not OPEN"); retry the state read because `gh pr view` can return empty for a few seconds right after PR creation. |
| #64 | **Stop false-positive rate-limit retry on SUCCESSFUL cycles**: every Claude stream carries a routine `rate_limit_event` with `overageStatus:rejected` as plan metadata — NOT a failure. Only retry on a real error signal AND <2 assistant turns. |
| #72 | **cursor-agent**: add `~/.local/bin` to PATH (non-login shells didn't have it → "cursor-agent not found"), and keep `cursor-agent login` alive ~180s for the browser approval (the 25s timeout killed it before you could approve). |
| #76 | **Resolve `gh`/`dotnet` to FULL PATH first** in `_find_exe`: it matched the bare `gh.exe` (on PATH in the worker's login shell) but the non-login `release` subprocess didn't have that PATH entry → empty `gh pr view` → recurring "PR not OPEN". Full path is PATH-independent. **This was the real cause of every AUTO_RELEASE failure.** |
| #79 | **Fetch the queued ticket BY NUMBER**: `gh issue list --label mutation` hits GitHub's **search index**, which lags several seconds after a label is added, so a just-queued ticket showed "0 tickets". Worker exports `MUTATE_TICKET`; setup uses `gh issue view N` (immediate). **This was the recurring "SETUP OK — 0 tickets".** |

### Agent routing (Groq actually runs on Groq)
| PR | Fix |
|----|-----|
| #65 | **Skill dispatches phases to the REAL routed agent.** The `/mutate` skill delegated via the Task tool, which only spawns **Claude** subagents — so "route to Groq" silently ran on Claude. New `POST /api/admin/agent/{id}/run` runs API agents (Groq/OpenAI) via MAF and returns reply+tokens; CLI agents return `inline` (the skill runs them inline). Proven: research+planning ran on Groq (635+1580 real tokens). |

### Approval UX (the human-in-the-loop checkpoint)
| PR | Fix |
|----|-----|
| #77 | **Reject → amend ticket → restart.** Reject now requires a recommendation; the API posts it as a comment on the ticket (the next cycle reads it as a tester comment) and **restarts** the cycle. Approve/Refine proceed in place. Nothing auto-approves. |
| #81 | **Show the plan + post-decision feedback.** The plan box had a light bg over light text (invisible) — fixed to readable dark code block. Added a confirmation line after Approve/Refine/Reject ("implementing now" / "restarting"). |
| #71 | **`released` terminal status** so a merged run doesn't sit at "PR open 100%" forever; UI shows green "Released ✔". |
| #50/#59 | **Clear-runs** endpoint + dashboard button + **reset-run** (drops a run stopped for an external reason so it can be re-queued). |

### API-native Groq cycle (no worker) + `.said` in-container
| PR | Fix |
|----|-----|
| #88 | **`POST /api/evolution/groq-cycle/{ticket}`** — the whole cycle runs in the API container: fetch ticket → Groq plans → operator approves → Groq writes the change (context from **`.said` recall**, not whole files) → container clones to writable `/tmp`, builds+tests, `gh auth setup-git` + push, opens PR. The read-only `/workspace` mount is never touched. Linux `said` binary + rebuilt `Advisory.said` baked into the image. `MaxOutputTokens=16000` so JSON isn't truncated. **Proven: ticket #85 → PR #86 (Add GET /api/host), no worker.** |
| #89 | **URGENT restore of `Program.cs`** — a Groq **full-file rewrite** (back in PR #83) had returned "full new content" with only its tiny endpoint, **deleting the rest of the app** (161 → 27 lines); 24 tests failed and main was broken for several PRs. Restored the full file + all endpoints. |
| #90 | **Re-register `GroqCycle` in DI** (the Program.cs restore predated the registration → 500 on evolution endpoints). |

### The remaining must-do (documented separately)
- **`said edit` surgical-edit feature** (see `docs/said-edit-feature-spec.md`): the gutting in #89 happened
  because the cycle does **full-file rewrites**. The durable fix is a `.said` `edit` subcommand that does
  precise insert/replace at a named anchor so a whole-file delete is impossible by construction. Until
  that lands, the Groq cycle's full-file rewrites remain risky — use only on tiny isolated endpoints.
- **Container has the dotnet RUNTIME, not the SDK**, so the in-clone `dotnet build`/`test` can't run →
  the Groq cycle opens **draft** PRs (PR-only safety working). To get non-draft PRs, add the SDK to the
  runtime image or gate the self-test.

---

## Quick reference — deploy commands used throughout
```bash
# Build + redeploy API + console (pinned project name, no-deps to avoid the nexus port clash)
docker compose -p advisory build api console
EVOLUTION_ENABLED=true EVOLUTION_REPO=Qonsult1001/advisory \
  PKGFW_GROQ_API_KEY=$KEY GROQ_API_KEY=$KEY \
  docker compose -p advisory up -d --no-deps api console

# Re-seed Groq routing/key after an API restart (restart resets in-memory policy to data/policy.json)
# PUT /api/admin/settings with mutationRouting all-groq + the groq agent apiKey.
```
