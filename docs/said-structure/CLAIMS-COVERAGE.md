# Recall-Claims Coverage Map

Every recall/memory behaviour `.said` CLAIMS in its documentation, mapped to the test that proves it.
The goal: nothing is claimed that isn't tested. A `FAIL`/`IGNORE` row is a real finding (the docs
promise something the engine doesn't yet do), not a broken test — those are tracked here honestly.

Status legend: ✅ tested+passing · ⚠️ tested, KNOWN GAP (`#[ignore]` + note) · 🔬 measurement-only.

## Recall quality (per category)


| Claim                                                         | Test                            | Status |
| ------------------------------------------------------------- | ------------------------------- | ------ |
| recall@10 ≥ MTEB gate via `ask`                               | `test_recall_at_volume.rs`      | ✅      |
| 10-category recall@1/@5/@10 (single-hop … negative/existence) | `test_recall_quality_volume.rs` | ✅      |
| Adversarial near-twin: exact discriminator never dropped      | `test_twin_precision_volume.rs` | ✅      |
| Ambiguous query surfaces multiple candidates (LLM resolves)   | `test_twin_precision_volume.rs` | ✅      |


## Ranking-affecting claims  → `test_recall_claims_ranking.rs`


| Claim                                                        | Doc                        | Status                                            |
| ------------------------------------------------------------ | -------------------------- | ------------------------------------------------- |
| Salience: decision scores above chit-chat + recallable       | row-33-salience            | ✅                                                 |
| Recall-weight: rises with repeated access, capped [1.0,2.0]  | 3.3-brain, public-overview | ✅ (fixed cap leak: recency ×1.2 could exceed 2.0) |
| Surprise/contradiction: latest version wins on recall        | row-41-surprise            | ✅                                                 |
| Entity-speaker (Layer 5): named speaker's statement recalled | 3.5 layer 5                | ✅                                                 |


## Brain-state claims → `test_recall_claims_brainstate.rs`


| Claim                                                  | Doc             | Status |
| ------------------------------------------------------ | --------------- | ------ |
| S_slow: 0 on fresh brain, magnitude grows with queries | 14.2-s-slow     | ✅      |
| Auto-dream / dream() completes without panic           | 14.5-auto-dream | ✅      |
| Recall consistent (gold not lost) across a dream       | 3.3-brain       | ✅      |


## Code-recall claims → `test_recall_claims_code.rs` (+ `test_code_graph.rs` for call/caller)


| Claim                                                   | Doc               | Status                 |
| ------------------------------------------------------- | ----------------- | ---------------------- |
| Exact symbol lookup, line-exact via `sym`               | 3.6, code.md      | ✅                      |
| Call-graph / caller-graph (`code_calls`/`code_callers`) | said_file         | ✅ (test_code_graph.rs) |
| Multi-language symbols (Rust/Python/JS) indexed + found | code.md (7 langs) | ✅                      |
| Semantic-intent code recall ("resend failed webhooks")  | public-overview   | ✅                      |


## Persistence / mode claims → `test_recall_claims_persistence.rs`


| Claim                                         | Doc                             | Status |
| --------------------------------------------- | ------------------------------- | ------ |
| mmap save→reopen→recall IDENTICAL result list | 02-file-format, public-overview | ✅      |
| Enterprise pointer: recall by summary         | row-36-external-pointer         | ✅      |
| Enterprise mode refuses content ingest        | row-37                          | ✅      |
| Scope-filtered recall returns ONLY scope      | 3.5                             | ✅      |


## Edge cases / robustness → `test_recall_edge_cases.rs`


| Case                                                                             | Status                                                       |
| -------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| empty / whitespace / single-char / stopwords-only / punctuation query → no panic | ✅                                                            |
| unicode / multiscript (Chinese) + emoji query                                    | ✅ (fixed: CJK queries yielded 0 keywords → now char-bigrams) |
| 1-memory brain                                                                   | ✅                                                            |
| all-near-identical corpus (exact discriminator found)                            | ✅                                                            |
| pure-number memories + number query                                              | ✅                                                            |
| memory superseded 20× → latest wins                                              | ✅                                                            |
| very-long query (≥20 words)                                                      | ✅                                                            |
| duplicate-content dedup in `ask` results (≤2 copies)                             | ✅                                                            |


## Combined end-to-end → `test_recall_e2e_combined.rs`
ONE mixed brain (notes + concept-bridge + update pair + 20 legal twins + footer-date doc + dup spam +
60 noise frames); 8 hard queries each exercising MANY mechanisms at once. 8/8 pass: paraphrase,
multi-hop bridge, update-latest-wins, legal discriminator (exact REF among 20 twins), footer-date
temporal, dedup, best-effort abstain, no cross-contamination. This is the "does it all hold together"
test — separate green unit checks don't prove the system works on a complex query; this does.

## Code bug-location (MCP) → `test_bug_location_e2e.rs`
The "locate the bug, guide Claude" claim, measured on the two axes the MCP-for-coding research
(MCP spec 2025-06-18; Anthropic tool-design; RepoCoder/SWE-bench/Lost-in-the-Middle; Serena/ast-grep
MCPs) says matter: ACCURACY and TOKEN ECONOMY.

**Setup:** a realistic project — the real auth/billing/util code (one function, `make_session`, has a
minutes-vs-seconds expiry bug) hidden among ~120 filler files / ~480 functions. The symptom query
("sessions expire too fast, seconds instead of thirty minutes") shares ZERO identifiers with the buggy
line — it must be found by meaning + structure.

**Measured (this is the comparison vs Claude Code / Cursor's grep+read baseline):**
| Axis | `.said` | index-less baseline (read the repo) | Result |
|------|---------|-------------------------------------|--------|
| Accuracy — locate the buggy fn | **rank 0** (top-1, conf 0.85) from the symptom alone | greps + reads files | ✅ exact, by meaning not string-match |
| Tokens to locate (chars proxy) | **345** (top-5 snippets) | **34,346** (whole project) | ✅ **99.6× leaner** (matches the research ~98%) |
| Blast radius | `code_callers(make_session)` → `handle_login` | — | ✅ impact set for the LLM→LSP handoff |

So `.said`'s value vs Claude/Cursor is exactly where the research says the baseline is weak: it hands
the agent the *precise* buggy function in ~345 chars instead of the agent burning ~34K reading the
repo, and it does so by SEMANTIC + STRUCTURAL recall (cheap BM25/grep can't from this symptom).

### Code-MCP shortfalls (honest gap table — what a WORLD-CLASS code MCP would add)
Audited `crates/said-mcp/`: 33 tools, but the responses are a "retrieval facade" (text, not structured).
Tracked here so the gaps are visible, not hidden.
| Shortfall | Today | Ideal (research-backed) |
|-----------|-------|-------------------------|
| Response schema | plain TEXT (`[conf][kind] doc_id` + 500-char snippet) | structured per-result `{doc_id, file, line_range, symbol, kind, why_relevant, confidence, signals[], resource_link, needs_analysis}` as `structuredContent` + `outputSchema` (MCP spec) |
| `locate_issue` tool | none — caller chains search→sym→ask manually | one tool fusing prior-fix memory + symbol + call-graph + churn + stack-frame into a RANKED hypothesis list (best-first, with reason + confidence) |
| Call-graph over MCP | `code_calls`/`code_callers` are Rust verbs, NOT MCP tools | expose `find_references`/callers/callees as MCP tools returning structured locations |
| Progressive disclosure | `get` returns the FULL frame (can be huge) | signature-first; body/enclosing on demand (`expand_context`); `resource_link` for large bodies (Serena `include_body` pattern) |
| Response-size control | fixed 500-char snippets, no mode | `response_format: concise\|detailed`; hard cap < 25K tokens (Claude Code truncation limit) |
| Grounding for the LLM | snippet only | each item cites its span + a short why-relevant rationale (ALCE: citable spans improve faithfulness + are acted on more correctly) |

These are the next build targets to make `.said`'s MCP world-class for coding (currently code-only;
to be extended). The retrieval CORE already wins on accuracy + tokens; the gap is the MCP *surface*.

## Agent steering (Claude Code) → DESIGNED, see `16-agent-steering.md`
How `.said` tells the agent WHEN/WHAT to use it — without ever editing CLAUDE.md (which persists in
git history and can't be cleanly removed when `.said` is uninstalled). Investigated nudge
(attunehq/nudge, Apache-2.0) at the source level: it is NOT an MCP server — it's a binary the agent
invokes as a `PreToolUse` subprocess hook (stdin JSON → allow/deny+context stdout), Claude Code +
Codex only, registered in gitignored `.claude/settings.local.json`, never CLAUDE.md. `.said` will
mirror this: a `said hook` subcommand (inject `.said` recall as `additionalContext` when the agent is
about to grep, fail-open) + an opt-in `said setup` that registers it + bundles a `said` skill.
Removal-safe by construction. **OPEN EXPERIMENT recorded**: does block-redirect ("query .said first")
beat inject-and-proceed? — measure on the bug-location corpus before fixing the default. Not built yet.

## KNOWN GAPS (honest — claimed/expected but NOT yet delivered)
| Gap | Evidence | Why it matters / fix |
|-----|----------|----------------------|
| Code call-graph walk is a VERB, not auto-fired in `ask` (by design) | `ask` returns the exact symbol; `code_calls`/`code_callers` are explicit verbs | DIVISION OF LABOR (06-ingestion-plugins/lsp.md): `.said` is RETRIEVAL — `ask` returns the exact symbol the client asked for (conf 1.00) so the LLM can hand it to a language server (rust-analyzer/tsserver/pyright) for the TYPE-PRECISE work ("find all references", "what breaks if I change this signature"). `ask` deliberately does NOT auto-walk the call-graph: that'd be a shallow, name-matched (untyped) traversal duplicating the LSP and injecting possibly-wrong neighbours into normal recall. The call-graph is the explicit `code_calls`/`code_callers` verbs the caller invokes WHEN it wants the neighbourhood (test_code_graph.rs), then passes to the LSP. (An earlier Engine-A-graph auto-walk was REMOVED for this reason — same recall accuracy, cleaner boundary.) `build_concept_links` excludes `Pillar::Code` by design — code connects via `call:` edges, not `link:` concept edges. |
| ~~Abstention is corpus-sensitive~~ **FIXED** | combined e2e now abstains (empty) on a no-answer query in a noisy mixed corpus | Added a threshold-free LEXICAL-GROUNDING veto (COIL/Clarity: arxiv 2021.naacl-main.241, ciir clarity): the score-shape gate (gap/commitment) is blind to whether the top hit shares a query TERM. A no-answer query's top hit is an embedding-proximity artifact with ZERO term overlap — so if NOTHING in the result shares a query content-term AND nothing is a strong exact lexical/symbol hit, abstain. Binary set-intersection, NO magnitude constant. Opt-in via SAID_ASK_ABSTAIN_SHAPE. |

## Engine fixes found by these tests

- **recall_weight cap leak**: recency multiplier (×1.2) pushed the [1.0,2.0] weight to 2.4; now clamped.
- **CJK/spaceless-script queries returned zero keywords** → `ask` recalled nothing for Chinese/Japanese/
Korean even though content was indexed; now emits character bigrams (trigram-matchable).
- **multi-hop r@1 gate was flaky** (bridge answer ranks below its entry point by design, ~0); gated on
the real r@5/r@10 signal instead.

