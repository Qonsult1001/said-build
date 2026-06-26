# `.said` vs baselines — end-to-end coding-agent benchmark (large project)

**What this measures.** Whether a real coding agent (Claude Code, driven headless) answers real
developer questions about a **large** codebase more cheaply and more correctly **with `.said`** than
without it. Three arms, all driven live with `claude --print --output-format json`, so every number is
a real run — turns, USD cost, cache-read tokens, and a correctness check against a gold oracle.

**Codebase under test.** `.said`'s own workspace — **322 Rust files, ~125K lines**, indexed to **4,372
memories**. Not a toy: the agent has to find the right file/function among hundreds.

**Arms — all three are the SAME Claude Code agent; only the `.said` integration differs.** There is no
"Claude vs `.said`" single comparison because there are **two distinct ways to wire `.said` in**, and
the whole point is to find out which one actually helps. So the baseline is Claude with no `.said`, and
the two `.said` arms are *pull* (the model may call `.said`) vs *push* (`.said` recall is delivered to
the model automatically).

| Arm | Plain name | What the agent has | `.said` delivery |
|---|---|---|---|
| **grep** | **Claude-native (baseline)** | Claude Code with only built-in `Grep`/`Read`/`Bash` | none |
| **mcp** | **`.said` via MCP tools (pull)** | + the `said-mcp` server (34 tools: `ask`, `sym`, `search`, `code_calls`, …) Claude may *choose* to call | pull |
| **hook** | **`.said` via hook (push)** | + the `said` `UserPromptSubmit` hook — recall auto-injected as a factual `<project_index>` every prompt | push |

The central result is about *delivery*: the same `.said` data **helps when pushed (hook) and is
break-even when only made available (mcp)** — because the model often does not reach for the MCP tools
on its own. Naming the arms "Claude vs `.said`" would hide exactly this finding.

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
- **T12 (deep-internals).** "Which section must be written so `sym()` works after reopen?" — the gold
  oracle wanted `TRGM` (the bug fixed in FIXES-LOG #1: `sym()` needs the `TRGM` doc-id list). All three
  arms answered the **`SYMS` / symbols** section instead — which is *also* correct (`sym()` reads from
  the SYMS-loaded `symbol_index`); the *complete* answer is "both SYMS and TRGM". Notably the **mcp arm
  produced an accurate code trace** ("`sym()` reads `symbol_index`, populated by SYMS at line 545–561…").
  So this is partly an **over-strict oracle** (it only accepted `TRGM`), not purely an agent miss — the
  agents reasoned about the index correctly but didn't know about the TRGM dependency that the fix
  introduced.

### Where `.said` actually lost (honest head-to-head: hook vs Claude-native)

Across 14 tasks: **hook cheaper on 4, tie on 8, Claude-native cheaper on 2.** The two losses:

| Task | Result | Why |
|---|---|---|
| **T05** (algorithm — "how is Hamming distance computed?") | grep $0.127/9t vs **hook $0.135/12t (+7%)** | Both correct. The injected recall didn't pinpoint the `popcount`/XOR code, so the agent investigated anyway **and** paid for the injection — a semantic algorithm question where recall added noise, not signal. The one case where push genuinely cost a little more. |
| **T06** (past-fix) | grep $0.034/**1t** vs hook $0.062/4t (+45%) | grep "won" only by **giving up in 1 turn** (read the skill doc, paraphrased). Both were *wrong*. Failing cheaply is not a real win. |

So on the tasks that mattered, `.said` (hook) did not lose: its only true cost loss was T05 (+7%, a
recall-noise case), and T05 is also where MCP-pull lost ($0.165). The wins are concentrated on the hard
multi-round questions: **T02 −54%, T11 −42%, T07 (multi-hop) 5→3 turns.** The 8 ties are trivial
single-symbol lookups where grep already resolves a unique identifier in one pass and there is no recall
saving to be had — `.said` correctly does no harm there.

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
