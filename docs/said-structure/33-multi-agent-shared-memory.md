# 33 — Multi-agent: `.said` as the shared-memory substrate (not a competing orchestrator)

How `.said` fits the multi-agent world. The design call (owner): **`.said` must be world-class through the
FEWEST moving parts — one portable brain a FLEET of agents share, so the next model/agent you plug in
inherits the whole brain and just works.** `.said` is NOT another orchestration framework; it is the
memory substrate that makes ANY multi-agent system (Kimi, Claude, `.said`'s own orchestrator, or a future
model) stop losing context across agents and compactions.

## How Kimi and Claude do multi-agent (read from source)

Both use the SAME parent→subagent delegation model (verified in `G:\Coding\kimi-cli-main` source; Claude
Code's `Agent` tool is structurally identical):

| Piece | Kimi (`src/kimi_cli/subagents/`, `tools/agent/`) | Claude `Agent` tool |
|---|---|---|
| Delegate | `AgentTool` params: `description`, `prompt`, `subagent_type`, `resume_agent_id`, `run_in_background` | `description`, `prompt`, `subagent_type`, `run_in_background` |
| Pick which agent | by the subagent's **`description`** (manifest + LLM-select) | by `subagent_type` description |
| Subagent context | **isolated** per agent: `Context(store.context_path(agent_id))`, persisted, resumable | isolated per subagent invocation |
| Result to parent | a **SUMMARY** (`run_with_summary_continuation`: "provide a more comprehensive summary… all important info the parent should know") | the subagent's **final message text** only |
| Foreground / background | `ForegroundSubagentRunner` / `BackgroundAgentRunner` | `run_in_background` |

## The two memory weaknesses this creates — and `.said` fixes

Multi-agent **multiplies** the single-agent memory problems:

1. **N× compaction amnesia.** Each subagent's context window compacts independently. So instead of one
   agent "going stupid" after compaction, you have a fleet of them, each losing its own thread.
2. **Lossy hand-off.** The subagent → parent result is a **summary** (Kimi) or just the final text
   (Claude) — the fact-dense detail (exact values, file:line, the why) is paraphrased away exactly when
   the parent needs to act on it. The parent cannot recall what the subagent actually found, only its
   summary of it.
3. **No shared learning.** Subagents don't compound across each other or across runs — each starts cold;
   a fix one agent verified is not reused by the next.

`.said` removes all three by being the ONE out-of-band brain every agent reads + writes:

| Multi-agent weakness | `.said` substrate |
|---|---|
| each subagent's context compacts independently | the brain is OUT of every window — compaction-survival ×N ([30](30-beat-them-benchmark.md)) |
| subagent → parent is a lossy summary | the subagent WRITES its finding as a claim+evidence memory; the parent RECALLS the EXACT frame (byte-exact, not a paraphrase) + the evidence links to verify ([31](31-memory-evidence-standard.md)) |
| no shared learning across agents | verified fixes + blueprints FEDERATE across the fleet — agent B reuses agent A's 80% ([28](28-token-value-and-scoping.md) §2, [Phase 4](32-full-suite-reproduction.md)) |
| subagent selection by description | identical to `.said`'s memory MANIFEST (name+description → LLM-select) — the same pattern, so it composes |

## The thesis — fewest moving parts

`.said` does NOT spawn, fork, or orchestrate agents. It adds ZERO new control-plane machinery to a
multi-agent system. The only shared thing is **one `.said` file**. Any agent — Kimi subagent, Claude
subagent, the next model you plug in — that can call the `.said` CLI/MCP inherits:

- every memory (claim + evidence), recalled byte-exact;
- every verified fix + blueprint (the 80%), federated;
- the whole code/sym/docs/git brain;
- and it all survives compaction because it lives outside every agent's window.

That is "plug in the next model → it just works", achieved by SUBTRACTING moving parts (no orchestrator),
not adding them. The host supplies the agents + the intelligence; `.said` supplies the durable shared
memory. (Same "frictionless, makes the host better" thesis as the single-agent case.)

## Proven (e2e)

[`hard-eval/beat-them/multiagent-shared-memory.js`](../../hard-eval/beat-them/multiagent-shared-memory.js)
— three distinct agents (separate processes/identities) on ONE `.said`, **5/5**:

1. agent B sees agent A's memory in the shared manifest;
2. agent B recalls A's finding **byte-exact** ("…parsed as SECONDS but issued as MILLISECONDS", `auth.rs:142`)
   — NOT a lossy summary;
3. agent B gets A's evidence links (verify-before-acting);
4. agent B reuses A's verified FIX (cross-agent procedural federation);
5. a newly-plugged-in agent C inherits the whole brain (memory + fix) with **zero setup**.

```bash
node hard-eval/beat-them/multiagent-shared-memory.js   # ALL PASS (5/5)
```

## Where `.said`'s own orchestrator fits

`said-orchestration` ([15](15-orchestration.md)) is a SINGLE-agent build/gate/recall flow (not a subagent
spawner) that already reads the shared brain (`best_iterations_federated`). It is one *consumer* of the
substrate, not the substrate itself — and it needs no subagent machinery to benefit: it recalls the fleet's
accumulated fixes/canon like any other agent.

## See also

- [30 — beat-them benchmark](30-beat-them-benchmark.md) — compaction-survival + compounding (the axes a fleet multiplies).
- [31 — memory-evidence standard](31-memory-evidence-standard.md) — claim→evidence→source (the exact hand-off, not a summary).
- [28 — token value + scoping](28-token-value-and-scoping.md) — federation / cross-project reuse (cross-agent here).
- [32 — full suite reproduction](32-full-suite-reproduction.md) — the federation e2e this builds on.
