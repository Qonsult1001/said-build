# .said File End-to-End Test Plan

**Status:** MUST ALL PASS before shipping
**Date:** 2026-04-06

---

## TEST 1: Code Search with AST Chunking

**What:** Index real Rust source files, recall specific functions by name and meaning.

```
1. Create brain.said
2. Index crates/sca-core/src/engine.rs (800+ lines, many functions)
3. Index crates/sca-core/src/brain.rs (400+ lines)
4. Index crates/sca-core/src/crystalline.rs (5000+ lines)
5. build_index() — SCA indexes all chunks
6. Verify: tree-sitter split engine.rs into named functions (search_immutable, index_batch, etc.)
7. recall("search immutable query") → returns the search_immutable function, NOT the whole file
8. recall("entity boost score") → returns entity_match_score function
9. recall("S_slow tensor write") → returns s_slow_write from brain.rs
10. recall("hybrid candidate retrieval") → returns the specific function from crystalline.rs
11. Verify: each result is a FUNCTION, not a 512-word chunk
12. Save brain.said, reopen, recall again → same results
```

**Pass criteria:** All 4 function recalls return the correct function. AST chunks have function names.

---

## TEST 2: Grep + SCA Fused Search

**What:** Prove grep exact match + SCA semantic work together.

```
1. Using the same indexed codebase from Test 1
2. fused_search("fn search_immutable") → grep finds EXACT text + SCA finds semantically
3. fused_search("how does the brain learn") → grep finds nothing, SCA finds brain.rs functions
4. fused_search("BLAKE3 checksum") → grep finds exact string, SCA finds integrity-related code
5. Verify: fused results ranked higher than either alone
```

**Pass criteria:** Fused search returns results from both sources. Pure semantic queries work. Pure text queries work.

---

## TEST 3: Personal Memories — Flat, Recall by Importance

**What:** Store personal facts, recall them, verify reconsolidation boosts.

```
1. Create personal.said
2. remember("My wife's birthday is March 15")
3. remember("I stopped smoking yesterday, June 3rd 2026")  
4. remember("My name is Carter")
5. remember("I live in Cape Town, South Africa")
6. remember("My favorite language is Rust")
7. remember("I have a daughter named Sophie")
8. remember("My dog's name is Max")
9. remember("I work at .said as CEO")
10. remember("My phone number is 082-555-1234")
11. remember("I prefer dark mode in all editors")
12. build_index()
13. save()

RECALL TESTS:
14. recall("birthday") → returns "My wife's birthday is March 15"
15. recall("smoking") → returns "I stopped smoking yesterday..."
16. recall("who am I") → returns "My name is Carter"
17. recall("where do I live") → returns Cape Town
18. recall("what language") → returns Rust
19. recall("daughter") → returns Sophie
20. recall("dog") → returns Max
21. recall("phone number") → returns 082-555-1234
22. recall("work") → returns .said CEO

RECONSOLIDATION:
23. Recall "birthday" 5 more times
24. Check brain_stats() → birthday memory has higher recall_weight
25. Recall all 10 memories → verify brain logged 10+ queries
26. save(), reopen → brain state persists, birthday still boosted
```

**Pass criteria:** All 10 recalls return correct memory. Birthday has highest recall_weight after repeated access.

---

## TEST 4: Business Document — 10,000 Words, Exact Sentence Recall

**What:** Upload a long business document, recall specific buried sentences.

```
1. Create business.said
2. Generate 10,000 word business document with 5 unique buried sentences:
   - Paragraph 25: "The Q3 revenue target is exactly $4.7 million"
   - Paragraph 50: "Sarah Chen is the VP of Engineering since January 2025"
   - Paragraph 75: "Project Aurora launch date is September 15th"
   - Paragraph 100: "The API rate limit must be set to 500 requests per minute"
   - Paragraph 150: "My name is Willie, paragraph 6.25"
3. remember(full_document)
4. Verify: auto-chunked into ~20+ passages
5. build_index()
6. save()

RECALL TESTS:
7. recall("Q3 revenue target") → returns passage with "$4.7 million"
8. recall("VP of Engineering") → returns passage with "Sarah Chen"
9. recall("Aurora launch date") → returns passage with "September 15th"
10. recall("API rate limit") → returns passage with "500 requests per minute"
11. recall("My name is Willie") → returns passage with "paragraph 6.25"

VERIFY PASSAGE NOT WHOLE DOC:
12. Each result content length < 5000 chars (passage, not 10K word doc)
13. Each result contains the exact buried sentence

PERSISTENCE:
14. save(), reopen, recall("Willie") → still finds it
```

**Pass criteria:** All 5 buried sentences found. Results are passages, not whole document. Survives save/load.

---

## TEST 5: S_slow Tensor — Cross-Document Synthesis

**What:** Prove the S_slow tensor connects documents that share NO keywords.

```
1. Create synthesis.said
2. remember("Eleanor Vance wants to start Solaris Flux, a solar panel company using photonic inverter technology")
3. remember("Aura Systems licenses photonic inverter technology for $15 million upfront plus 8% royalties")  
4. build_index()

5. S_slow tensor should now contain signal from both embeddings
6. recall("What financial hurdle does Eleanor face?") 
   → Should return BOTH documents (connected through "photonic inverter")
7. Check s_slow_magnitude() > 0 (tensor has accumulated signal)

COMPARE WITHOUT S_SLOW:
8. Create a fresh engine WITHOUT brain
9. Same documents, same query
10. Verify S_slow brain gives better ranking for the cross-document connection
```

**Pass criteria:** Both documents retrieved for the synthesis query. S_slow magnitude > 0.

---

## TEST 6: Neural Decay — Old Memories Fade, Recalled Memories Strengthen

**What:** Prove the brain decays cold memories and strengthens recalled ones.

```
1. Create decay.said
2. remember("Memory A — very important")
3. remember("Memory B — somewhat important")
4. remember("Memory C — not important")
5. build_index()

6. Recall "Memory A" 10 times → recall_weight should be ~1.15+
7. Recall "Memory B" 2 times → recall_weight should be ~1.07
8. Never recall "Memory C" → recall_weight stays at 1.0

9. Run consolidate() → Memory C's weight should decay toward 1.0 (stay neutral since never boosted)
10. Run consolidate() 5 more times → Cold memories stay neutral, boosted memories decay slightly

11. Verify ordering: get_recall_weight("A") > get_recall_weight("B") > get_recall_weight("C")

DREAM DRIFT:
12. Recall "Memory A" 50 more times (accumulate query embeddings)
13. Run dream(10) → corpus_mean should drift
14. Verify s_slow_magnitude() > previous value
15. save(), reopen → all decay/boost state persists
```

**Pass criteria:** A > B > C in recall_weight. Consolidation decays. Dream drifts. All persists.

---

## TEST 7: Everything Together — One .said File

**What:** Personal + Business + Code in one file, all searchable, all learning.

```
1. Create unified.said

PERSONAL:
2. remember("My name is Carter, I live in Cape Town")
3. remember("My wife's birthday is March 15")

BUSINESS:
4. remember_as("q3_report", long_business_document, "Q3 Report")

CODE:
5. code_search.index_codebase(&mut brain, &["rs"])  // index real Rust source

6. build_index()
7. save()

CROSS-DOMAIN RECALL:
8. recall("birthday") → personal memory
9. recall("Q3 revenue") → business document passage
10. recall("search_immutable function") → code function
11. recall("brain reconsolidation") → code function from brain.rs

VERIFY BRAIN LEARNS:
12. Recall "birthday" 5 times
13. Recall "search_immutable" 3 times
14. brain_stats() → shows both boosted
15. s_slow_magnitude() > 0

PERSISTENCE:
16. save()
17. Reopen unified.said
18. All recalls still work
19. Brain state persists
20. S_slow tensor persists

FILE STATS:
21. Print file size, frame count, compression ratio
22. Verify .said file < raw text size (compression working)
```

**Pass criteria:** All 4 domains searchable in one file. Brain learns from cross-domain usage. File persists.

---

## TEST 8: LSP Operations (Structural)

**What:** NOT implementing LSP client — but verify the interface is designed and ready.

```
1. Verify CodeSearch::lsp_operations() returns all 9 operations
2. Verify operations match Claude Code's LSPTool exactly:
   goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol,
   goToImplementation, prepareCallHierarchy, incomingCalls, outgoingCalls
3. Document: LSP client implementation deferred (needs lsp-types + tower-lsp crates)
```

**Pass criteria:** Interface defined. Implementation deferred with clear plan.

---

## IMPLEMENTATION ORDER

1. Test 3 (Personal) — simplest, proves remember/recall
2. Test 4 (Business document) — proves chunking + passage recall
3. Test 6 (Decay) — proves brain learning is real
4. Test 5 (S_slow synthesis) — proves tensor cross-doc works
5. Test 1 (Code AST) — proves tree-sitter chunking
6. Test 2 (Fused search) — proves grep+SCA together
7. Test 7 (Everything) — proves unified .said file
8. Test 8 (LSP) — interface only

Each test is a standalone Rust test in `tests/test_said_e2e.rs`.
Each must pass with `cargo test --features "static-embed,encryption,code"`.
NO test is optional. ALL must pass.
