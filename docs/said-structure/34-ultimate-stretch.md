# 34 — The ultimate stretch: portable `.said` memory vs 100 MB of markdown

Every claim of the portable `.said` brain, measured on REAL brains, against the baseline the owner named:
**a coding project's memory normally lives as ~100 MB of markdown files an agent keeps re-loading into
context.** Research-grounded, honest about the one real weakness (speed at scale).

Run: `node hard-eval/beat-them/ultimate-stretch.js` → **8/8** (`ultimate-stretch-result.txt`).

## The research baseline (why 100 MB of markdown is the wrong design)

The 2026 agent-memory literature is blunt about full-context markdown:

- a **200-entry markdown store re-injects ~4,600 tokens of memory PER CALL**; indexed retrieval injects
  **~130 tokens** for top-5 ([mem0 2026](https://mem0.ai/blog/the-2026-token-optimization-playbook-cut-ai-agent-memory-costs-3%E2%80%934x)).
- naive full-context / naive-RAG runs **3–5× higher token cost** than retrieval
  ([mem0 state-of-memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026)).
- *"context windows flooding with tokens, retrieval returning the wrong memories, stale facts never pruned
  actively degrade output quality … the work of organizing/relating/compressing should happen once at
  creation time"* ([the hidden cost of context windows](https://www.aibmag.com/trending-ai-enterprise-solutions/ai-agent-memory-hidden-costs/)).

`.said` does the organizing at WRITE time (distil-not-dump, salience, OKF links) and returns a SLICE at
read time — exactly what the research says full-context markdown gets wrong.

## The eight claims — measured

| # | Claim | Result (real brains) |
|---|---|---|
| 1 | **File size at scale** | **134 MB** real Claude markdown/transcripts → **6.0 MB** `.said` = **22× smaller** (or 0.2 MB if distilled-only). |
| 2 | **Speed** | recall returns a SLICE: **842 ms** (0.2 MB brain) / **3.9 s** (6 MB / 5,000-frame brain). vs a 100 MB markdown store that **cannot fit in context at all**. |
| 3 | **Missing information** | none — `recall-memory` returns the claim + evidence links **byte-exact** (the fact-dense detail markdown holds, `.said` keeps). |
| 4 | **Tokens used + saved** | markdown-in-context = **~33.6 MILLION tok/session** (load it all) → `.said` slice = **~524 tok** = **64,000× fewer**. (The mem0 ~130-tok retrieval number, at this corpus scale.) |
| 5 | **Wipe a whole project** | `said delete --project <name>` tombstones every `project:` frame — SCOPED (project A wiped, B survives) + recoverable via `admin`. |
| 6 | **Cross-use A→B** | the `create` canon is **byte-identical** across the C# and Rust projects — federation / cross-language reuse. |
| 7 | **Parallel agents** | the safe shared-memory model — a fleet shares `.said`; per-agent brains + federation (proven 5/5, `multiagent-shared-memory.js`). |
| 8 | **Portable project-switch** | copy the `.said`, open it under a NEW `SAID_PROJECT`, ask a question → `sym` + recall work immediately. **Plug the portable brain into a new project and it just works, scoped natively, zero setup.** |

## The new capability shipped for this

`said delete --project <name> [--dry-run]` — wipe an entire project's memories on the CLI (previously
MCP-only). Tombstoned (lineage preserved, recoverable). Scoped: only `project:<name>` frames go; other
projects in a shared brain are untouched. Proven + regression 15/15 green.

## Honest weakness (stated, not hidden)

**Recall latency scales with brain size.** A 0.2 MB project brain recalls in ~0.8 s; the 6 MB / 5,000-frame
imported brain takes ~3.9 s per query (the per-query encode + frame scan grows with the corpus). This is
real and worth optimizing — but the comparison still wins decisively: a 100 MB+ markdown store **cannot be
loaded into context at all**, so the alternative isn't "faster", it's "impossible". The `.said` slice is
~500 tokens regardless of corpus size, so the agent stays inside its window; markdown does not. Speed at
scale is the optimization target ([3.5 retrieval pipeline](03-core-subsystems/3.5-retrieval-pipeline.md)),
not a correctness gap.

## What this proves about the design

The portable `.said` is the inversion of the 100 MB-markdown design: **store-time organization + read-time
slice**, in ONE file that is 22× smaller, recalls a ~500-token slice instead of flooding the window,
survives compaction, scopes + wipes per project, federates across projects/languages, and plugs into a new
project with zero setup. The research says full-context markdown is the wrong design; `.said` is the right
one — measured.

## See also

- [28 — token value + scoping](28-token-value-and-scoping.md) — the per-task token/$ economics + project scope.
- [29 — learning from Claude/Kimi memory](29-learning-from-claude-kimi-memory.md) — distil-not-dump import.
- [30 — beat-them benchmark](30-beat-them-benchmark.md) — compaction-survival + compounding.
- [31 — memory-evidence standard](31-memory-evidence-standard.md) — claim→evidence→source.
- [33 — multi-agent shared memory](33-multi-agent-shared-memory.md) — the fleet model (claim 7).
- [05-features/row-33-salience.md](05-features/row-33-salience.md) — salience scoring (what's worth keeping).
