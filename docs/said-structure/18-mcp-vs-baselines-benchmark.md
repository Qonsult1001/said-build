# `.said` vs baselines — end-to-end coding-agent benchmark (large project)

**What this measures.** Whether a real coding agent (Claude Code, driven headless) answers real
developer questions about a **large** codebase more cheaply and more correctly **with `.said`** than
without it. Three arms, all driven live with `claude --print --output-format json`, so every number is
a real run — turns, USD cost, cache-read tokens, and a correctness check against a gold oracle.

**Codebase under test.** `.said`'s own workspace — **322 Rust files, ~125K lines**, indexed to **4,372
memories**. Not a toy: the agent has to find the right file/function among hundreds.

**Arms.**
| Arm | What the agent has |
|---|---|
| **grep** | Claude Code with only built-in `Grep`/`Read` — the honest baseline |
| **mcp** | + the `said-mcp` server registered (34 tools: `ask`, `sym`, `search`, `code_calls`, …) |
| **hook** | + the `said` `UserPromptSubmit` hook — recall injected as a factual `<project_index>` |

**Task suite.** 14 tasks spanning novice→expert and every retrieval nuance: find-symbol, locate-logic,
cross-file, algorithm, past-fix, multi-hop, abstention, code-graph, deep-internals. Each has a gold
regex oracle matched against the agent's final answer.

**Cursor (documented, not live).** Cursor cannot be driven headless here, so it is not in the live
numbers. Qualitatively: Cursor indexes the repo into a vector DB and injects semantically-retrieved
chunks into context. That is closest to the **hook** arm in spirit (retrieval injected as context) — but
it is a *general embedding* index with no symbol table, no exact-symbol `sym`, no code call-graph, no
threshold-free abstention, and no learned-fix replay. The capabilities below marked ✗ for "generic
embedding RAG" are what `.said` adds over that model.

---

## Results (real runs)

### Aggregate, 14 tasks per arm

| Arm | Correct | Total turns | Total cost | Avg cost/task | Cache-read |
|---|---|---|---|---|---|
| grep | 12/14 | 76 | $1.371 | $0.0979 | 513,304 |
| mcp | 12/14 | 97 | $1.576 | $0.1126 | 668,417 |
| **hook** | **12/14** | **74** | **$1.140** | **$0.0814** | 503,714 |

### Cost vs grep, on the 12 tasks where both arms answered correctly

| Arm | Cost vs grep |
|---|---|
| mcp | **−1.6%** (roughly break-even) |
| **hook** | **−19.7%** (clear win) |

### Cheapest correct arm, per task (14 tasks)

| Arm | Tasks where it was cheapest |
|---|---|
| **hook** | **7** |
| grep | 4 |
| mcp | 1 |
| (2 tasks all-wrong, excluded) | — |

### Excluding the two tasks every arm got wrong (T06, T12)

| Arm | Cost (12 tasks) | Turns |
|---|---|---|
| grep | $1.277 | 70 |
| mcp | $1.256 | 67 |
| **hook** | **$1.025** | **66** |

---

## Honest reading of the data

**The `UserPromptSubmit` hook is the winning integration: ~20% cheaper than grep with identical
correctness (12/14), fewest turns, and cheapest on half the tasks.** This matches the small-project
result and the channel-trust finding (docs 16): recall injected as factual context on the trusted
channel is *used*, so the agent skips redundant grepping.

**The raw MCP-tools arm is roughly break-even, not a clear win.** Exposing 34 tools lets the model
*choose* to call `.said`, but it doesn't always — and when it explores tools it can spend more turns.
One task (T12) blew up to **25 turns / $0.257** in the MCP arm (tool-exploration runaway) — that single
outlier is most of the MCP arm's extra cost. The lesson is the same one the steering doc already makes:
**delivery channel matters more than tool availability.** The hook (push) beats the MCP tools (pull).

**Two tasks every arm got wrong — reported, not hidden:**

- **T06 (past-fix).** "Which function recalls a past coding fix?" All three arms failed. This is a
  **test-setup gap, not a missing capability**: the benchmark brain was built with `said init` (which
  indexes *code*) and no fix was ever stored via `learn_fix`, so the fix-store was empty and `recall_fix`
  correctly abstained. Fix-replay is a first-class, shared capability (core + CLI + MCP + SDK +
  orchestration) and is proven separately by `hard-eval/recall-measure.sh`: an LRU target is recalled at
  **0.73–0.75** and the semantic fingerprint separates the adversarial LFU twin. See FIXES-LOG /
  `recall_coding_fix`.
- **T12 (deep-internals).** "Which section must be written so `sym()` works after reopen?" — answer
  `TRGM`. No arm named it; this is genuinely deep internal knowledge that lives in the code + design docs
  (it's literally the bug fixed in FIXES-LOG #1), not something recall surfaces from a code index. An
  honest "the agent couldn't infer this from the codebase" result.

**Where grep already wins (and `.said` shouldn't pretend otherwise):** trivial single-symbol lookups
(T01, T09, T13) are ties — grep finds a unique identifier in one pass, and there's no recall saving to
be had. `.said`'s edge shows up on **harder locate-logic / multi-hop questions** where grep needs
several rounds: T02 (grep 9 turns/$0.275 → hook 8/$0.128, **−54%**), T11 (grep 10/$0.234 → hook
8/$0.135, **−42%**), T07 multi-hop (grep 5 turns → hook 3).

## Capability comparison (what each approach can even attempt)

| Capability | grep | generic embedding RAG (≈ Cursor) | `.said` (mcp + hook) |
|---|---|---|---|
| Literal pattern search | ✓ | ✓ | ✓ |
| Semantic / paraphrase recall | ✗ | ✓ | ✓ |
| Exact symbol table (`sym`) | ✗ | ✗ | ✓ |
| Code call-graph (`code_calls`/`code_callers`) | ✗ | ✗ | ✓ |
| Threshold-free abstention ("does X exist?") | ✗ | partial | ✓ |
| Multi-hop wikilink/concept traversal | ✗ | ✗ | ✓ |
| Learned-fix replay (`learn_fix`/`recall_fix`) | ✗ | ✗ | ✓ |
| Pushed at the decision point (no model opt-in) | ✗ | ✓ (context inject) | ✓ (hook) |
| Single portable file, offline, no server/DB | ✓ | ✗ | ✓ |

## Bottom line

On a large real codebase, **`.said` via the `UserPromptSubmit` hook is ~20% cheaper than grep at equal
correctness, with the biggest wins on the hard multi-round questions** — and it brings retrieval
capabilities (exact symbols, call-graph, abstention, fix-replay) that neither grep nor a generic
embedding index has. The raw MCP-tools arm is break-even on cost (the model doesn't always reach for the
tools) — confirming the project's standing finding that **the delivery channel, not tool availability,
is what makes an agent actually use its memory.** Claims are reported with their failures (T06 setup
gap, T12 deep-internals miss) rather than cherry-picked.

*Reproduce:* the harness drives `claude --print --permission-mode bypassPermissions --output-format
json` across the three arm configs (grep = no `.said`; mcp = `said-mcp` in `.mcp.json`; hook =
`said setup`'s `UserPromptSubmit` registration) over the 14-task suite, parsing `num_turns` /
`total_cost_usd` / `cache_read_input_tokens` and grading the final answer against each task's gold regex.
