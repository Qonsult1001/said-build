# .said — Measured Scorecard (African Bank + Wonga, 2026-07-02)

Every number below is measured on this machine against a real enterprise C#/T-SQL
codebase. No projections unless explicitly marked.

## The two real fixes (root-caused by input isolation, not code-guessing)

| Axis | Before | After | Fix | Commit |
|---|---|---|---|---|
| Ingest memory (37k Wonga) | 920 MB | **505 MB** | Exclude `.csv` data-dump type | `eef73c0` |
| Ingest speed (37k Wonga) | 458 s | **124 s** | O(1) pending lookup via `doc_id_map` (was O(N²) linear scan) | `ea6564f` |
| Encode phase | 436 s | **17.8 s** | (same O(1) fix) | — |
| Budget code | 3-way accountant | **one constant (−19 lines)** | proven no-op on peak; kept mobile spill floor | `61a3f0c` |

The wasted-2-days lesson: a **stray leftover process** inflated an external memory poller into
a fake "1.4 GB floor," sending several fixes (SPIMI spill, thread caps, mimalloc, passage
batching) down the wrong path — all reverted. In-process measurement + per-directory
isolation found the two real bugs in minutes each.

## Mobile viability (real OS-enforced ceiling, not a projection)

- Windows **Job Object hard cap = 256 MB** (kills the process on exceed — same as a phone OS):
  African Bank ingest → **exit 0, brain saved (10 MB), recall works**.
- Soft 64 MB budget: peak 182 MB, ingest 18 s.
- Steady-state recall reads the mmap'd 10 MB file — tiny resident footprint.

## Code recall — .said vs an agent without persistent memory

Ground-truth: 20 real symbols (C# classes + T-SQL tables) → their known defining file.

| | Accuracy | Method | Cost per query |
|---|---|---|---|
| **.said** (CLI symbol index) | **20 / 20** | direct index lookup | ~477 ms, **0 tokens**, 0 file reads |
| **Agent without .said** (in-harness Explore subagents) | **8 / 8** | grep + read the repo | ~2.6 s, **~18,500 tokens/query**, repeated every session |

Both find the right file — the code is there to be found. The difference is **cost**: the agent
burned **148,231 tokens + 21 tool calls for 8 questions**; .said returns the same answers from a
pre-built index with essentially zero tokens and no repeated scanning. That recurring
grep-and-read cost — paid every session, every question — is exactly what .said eliminates.

## Full coding-process suite (hard-eval/beat-them/full-suite.js)

**8 / 8 axes pass** on the fixed binary:
coding-brain (sym + harvested blueprints), effort-decay/canon reuse, cross-project federation,
consolidation (keep-first), abstention (no confabulation), compaction-survival (re-grounded 3/3
exact decisions), update-on-the-fly, and **18,345× fewer tokens** to recover context after a
compaction vs a full dump.

## SQL — first-class via symbols (the design decision)

T-SQL is indexed per table/view/proc as **symbols** (e.g. `table LEDGER.LED_LEDGERENTRY @
…led_LedgerEntry.sql::create_table`), NOT passage-dense dense-encoding. This is why memory
stays flat and ingest stays fast on schema-heavy repos — SQL rides the SYMS + wiki-link path,
resolved like any other identifier.

## Live-Claude note

One clean live-Claude data point (same question, both correct): Claude 25.2 s vs .said 282 ms
cold / 55 ms warm. A broader live-Claude battery was NOT completed here because driving the
`claude` CLI from inside a Claude Code session crashes the session; the memory-less-agent
comparison above was run with in-harness subagents instead (safe, same conclusion).
