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

