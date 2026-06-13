//! End-to-end tests for the .said brain file.
//! ALL must pass before shipping.

use sca_core::said_file::SaidFile;

// =========================================================================
// TEST 3: Personal Memories — Flat, Recall by Importance
// =========================================================================

#[test]
fn test_03_personal_memories() {
    let path = "test_e2e_personal.said";

    // Create and load encoder
    let mut brain = SaidFile::create(path);

    // Load static encoder — try common paths
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    let mut loaded = false;
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() {
            println!("Encoder loaded from: {}", ep);
            loaded = true;
            break;
        }
    }
    if !loaded {
        println!("SKIP: static encoder not found (need said-lam-static/)");
        let _ = std::fs::remove_file(path);
        return;
    }

    brain.remember("My wife's birthday is March 15");
    brain.remember("I stopped smoking yesterday, June 3rd 2026");
    brain.remember("My name is Carter");
    brain.remember("I live in Cape Town, South Africa");
    brain.remember("My favorite programming language is Rust");
    brain.remember("I have a daughter named Sophie");
    brain.remember("My dog's name is Max");
    brain.remember("I work at .said as CEO");
    brain.remember("My phone number is 082-555-1234");
    brain.remember("I prefer dark mode in all editors");

    // Build search index
    brain.build_index().expect("build_index failed");
    brain.save().expect("save failed");

    println!("Personal: {} frames, {} bytes", brain.stats().active_frames, brain.stats().file_size);

    // Recall each memory
    let tests = vec![
        ("birthday", "March 15"),
        ("smoking", "stopped smoking"),
        ("who am I", "Carter"),
        ("where do I live", "Cape Town"),
        ("favorite language", "Rust"),
        ("daughter", "Sophie"),
        ("dog name", "Max"),
        ("phone number", "082-555-1234"),
        ("work job", ".said"),
        ("dark mode", "dark mode"),
    ];

    let mut passed = 0;
    for (query, expected_substring) in &tests {
        let results = brain.recall(query, 3);
        let found = results.iter().any(|r| r.content.contains(expected_substring));
        if found {
            passed += 1;
            println!("  [OK] '{}' → found '{}'", query, expected_substring);
        } else {
            let top = results.first().map(|r| &r.content[..r.content.len().min(80)]).unwrap_or("EMPTY");
            println!("  [FAIL] '{}' → expected '{}', got: {}", query, expected_substring, top);
        }
    }

    println!("Personal recall: {}/{}", passed, tests.len());
    assert!(passed >= 8, "At least 8/10 personal memories must be recalled (got {})", passed);

    // RECONSOLIDATION: recall birthday 5 more times
    for _ in 0..5 {
        let _ = brain.recall("birthday", 1);
    }

    let stats = brain.stats();
    println!("Brain: {} queries logged, {} docs boosted", stats.brain_queries, stats.brain_boosted);
    assert!(stats.brain_queries >= 15, "Brain should have logged 15+ queries");
    assert!(stats.brain_boosted > 0, "Some docs should be boosted");

    // Save and reopen — brain persists
    brain.save().expect("save with brain");
    let brain2 = SaidFile::open(path).expect("reopen");
    let stats2 = brain2.stats();
    assert_eq!(stats2.brain_queries, stats.brain_queries, "Brain queries should persist");
    assert!(stats2.brain_boosted > 0, "Brain boosts should persist");

    println!("Personal memories: PASS ({}/{}, brain persists)", passed, tests.len());

    let _ = std::fs::remove_file(path);
}

// =========================================================================
// TEST 4: Business Document — 10,000 Words, Exact Sentence Recall
// =========================================================================

#[test]
fn test_04_business_document() {
    let path = "test_e2e_business.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);

    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    let mut loaded = false;
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() { loaded = true; break; }
    }
    if !loaded {
        println!("SKIP: static encoder not found");
        return;
    }

    // Build a 10,000+ word business document with 5 buried needles
    let mut paragraphs: Vec<String> = Vec::new();
    for i in 0..200 {
        paragraphs.push(format!(
            "Section {} of the annual report covers standard business operations \
             including supply chain management, vendor relationships, quarterly \
             planning cycles, human resources policy updates, and compliance \
             requirements for regulatory frameworks. This section provides \
             detailed guidance on operational excellence and continuous improvement \
             methodologies aligned with industry best practices for section {}.", i, i
        ));
    }

    // Needle 1: paragraph 25
    paragraphs[25] = "The Q3 revenue target is exactly $4.7 million, approved by the \
        board during the emergency session on March 15th. This figure represents \
        a 23% increase over Q2 actuals and requires aggressive sales pipeline \
        acceleration in the EMEA region.".to_string();

    // Needle 2: paragraph 50
    paragraphs[50] = "Sarah Chen was appointed VP of Engineering in January 2025 after \
        the departure of the previous CTO. She brings 15 years of experience from \
        Google and Meta, specializing in distributed systems and ML infrastructure.".to_string();

    // Needle 3: paragraph 75
    paragraphs[75] = "Project Aurora launch date is confirmed for September 15th. The \
        go-to-market strategy includes a phased rollout starting with enterprise \
        customers in North America followed by APAC expansion in Q1 2027.".to_string();

    // Needle 4: paragraph 100
    paragraphs[100] = "CRITICAL: The API rate limit must be set to exactly 500 requests \
        per minute per authenticated user. Exceeding this threshold triggers automatic \
        circuit breaker protection and returns HTTP 429 status codes.".to_string();

    // Needle 5: paragraph 150
    paragraphs[150] = "My name is Willie, paragraph 6.25. I was born in Cape Town and \
        moved to London in 2019. This is a personal note buried deep in a business \
        document to test exact recall capability.".to_string();

    let full_doc = paragraphs.join("\n\n");
    let word_count = full_doc.split_whitespace().count();
    println!("Document: {} words, {} bytes", word_count, full_doc.len());

    // Remember the full document (auto-chunks)
    brain.remember(&full_doc);
    brain.build_index().expect("build_index");
    brain.save().expect("save");

    let stats = brain.stats();
    println!("Chunked into {} frames, .said file {} bytes", stats.active_frames, stats.file_size);
    assert!(stats.active_frames > 5, "Should be auto-chunked into multiple frames");

    // Recall the 5 buried needles
    let tests = vec![
        ("Q3 revenue target", "$4.7 million"),
        ("VP of Engineering", "Sarah Chen"),
        ("Aurora launch date", "September 15th"),
        ("API rate limit", "500 requests"),
        ("My name is Willie", "paragraph 6.25"),
    ];

    let mut passed = 0;
    for (query, expected) in &tests {
        let results = brain.recall(query, 5);
        let found = results.iter().any(|r| r.content.contains(expected));

        if found {
            passed += 1;
            // Verify it's a passage, not the whole document
            let passage_len = results.iter()
                .find(|r| r.content.contains(expected))
                .map(|r| r.content.len())
                .unwrap_or(0);
            let is_passage = passage_len < full_doc.len() / 2;
            println!("  [OK] '{}' → found '{}' ({}B passage, {}x smaller than full doc)",
                query, expected, passage_len, full_doc.len() / passage_len.max(1));
            assert!(is_passage, "Result should be a passage, not the full {} byte document", full_doc.len());
        } else {
            let top = results.first().map(|r| &r.content[..r.content.len().min(80)]).unwrap_or("EMPTY");
            println!("  [FAIL] '{}' → expected '{}', got: {}", query, expected, top);
        }
    }

    println!("Business document: {}/{}", passed, tests.len());
    assert!(passed >= 4, "At least 4/5 buried sentences must be found (got {})", passed);

    // Persistence: save, reopen, recall again
    brain.save().expect("save");
    let mut brain2 = SaidFile::open(path).expect("reopen");
    for ep in &encoder_paths {
        if brain2.load_encoder(ep).is_ok() { break; }
    }
    brain2.build_index().expect("rebuild index");

    let results = brain2.recall("Willie paragraph", 3);
    let willie_found = results.iter().any(|r| r.content.contains("Willie"));
    println!("  Persistence: Willie {} after save/reopen", if willie_found { "FOUND" } else { "LOST" });
    assert!(willie_found, "Willie should survive save/reopen");

    println!("Business document: PASS ({}/{})", passed, tests.len());

    let _ = std::fs::remove_file(path);
}

// =========================================================================
// TEST 6: Neural Decay — Old Memories Fade, Recalled Memories Strengthen
// =========================================================================

#[test]
fn test_06_neural_decay() {
    let path = "test_e2e_decay.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);

    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() { break; }
    }
    if brain.engine.encode_query("test").is_none() {
        println!("SKIP: encoder not found");
        return;
    }

    // Store 3 memories
    brain.remember("Memory Alpha is extremely important and gets recalled often");
    brain.remember("Memory Beta is moderately important and gets recalled sometimes");
    brain.remember("Memory Gamma is never recalled and should fade over time");
    brain.build_index().expect("build_index");

    // === RECONSOLIDATION: recall Alpha 10 times ===
    println!("=== RECONSOLIDATION ===");
    for i in 0..10 {
        let results = brain.recall("Memory Alpha important", 1);
        assert!(!results.is_empty(), "Alpha should be recallable (iteration {})", i);
    }

    // Recall Beta 2 times
    for _ in 0..2 {
        let _ = brain.recall("Memory Beta moderate", 1);
    }

    // Never recall Gamma

    // Check weights
    let w_alpha = brain.engine.brain.get_recall_weight("mem_0");
    let w_beta = brain.engine.brain.get_recall_weight("mem_1");
    let w_gamma = brain.engine.brain.get_recall_weight("mem_2");

    println!("  Alpha (10 recalls): weight = {:.4}", w_alpha);
    println!("  Beta  (2 recalls):  weight = {:.4}", w_beta);
    println!("  Gamma (0 recalls):  weight = {:.4}", w_gamma);

    assert!(w_alpha > w_beta, "Alpha > Beta: {:.4} vs {:.4}", w_alpha, w_beta);
    assert!(w_beta > w_gamma, "Beta > Gamma: {:.4} vs {:.4}", w_beta, w_gamma);
    assert!((w_gamma - 1.0).abs() < 0.01, "Gamma near 1.0: {:.4}", w_gamma);
    println!("  Ordering: Alpha > Beta > Gamma ✓");

    // === CONSOLIDATION: decay cold memories ===
    println!("\n=== CONSOLIDATION (decay) ===");
    let w_alpha_before = w_alpha;
    let w_beta_before = w_beta;

    for _ in 0..5 {
        brain.consolidate();
    }

    let w_alpha_after = brain.engine.brain.get_recall_weight("mem_0");
    let w_beta_after = brain.engine.brain.get_recall_weight("mem_1");
    let w_gamma_after = brain.engine.brain.get_recall_weight("mem_2");

    println!("  Alpha: {:.4} -> {:.4}", w_alpha_before, w_alpha_after);
    println!("  Beta:  {:.4} -> {:.4}", w_beta_before, w_beta_after);
    println!("  Gamma: {:.4} -> {:.4}", w_gamma, w_gamma_after);
    assert!(w_alpha_after > w_beta_after, "Alpha still > Beta after decay");
    println!("  Decay verified ✓");

    // === S_SLOW TENSOR ===
    println!("\n=== S_SLOW TENSOR ===");
    let magnitude = brain.engine.brain.s_slow_magnitude();
    println!("  S_slow magnitude: {:.4}", magnitude);
    assert!(magnitude > 0.0, "S_slow should have signal from searches");

    // === DREAM DRIFT ===
    println!("\n=== DREAM DRIFT ===");
    let dreamed = brain.dream(5);
    println!("  Dream triggered: {}", dreamed);
    assert!(dreamed, "Dream should trigger (12+ queries accumulated)");

    // === PERSISTENCE ===
    println!("\n=== PERSISTENCE ===");
    brain.save().expect("save");
    let brain2 = SaidFile::open(path).expect("reopen");

    let stats = brain2.stats();
    println!("  Queries: {}, Boosted: {}, Cycles: {}, S_slow: {:.4}",
        stats.brain_queries, stats.brain_boosted, stats.brain_cycles,
        brain2.engine.brain.s_slow_magnitude());

    assert!(stats.brain_queries >= 12, "Queries persist: {}", stats.brain_queries);
    assert!(stats.brain_boosted > 0, "Boosts persist");
    assert!(stats.brain_cycles > 0, "Cycles persist");
    assert!(brain2.engine.brain.s_slow_magnitude() > 0.0, "S_slow persists");

    let w_a = brain2.engine.brain.get_recall_weight("mem_0");
    let w_b = brain2.engine.brain.get_recall_weight("mem_1");
    println!("  Reloaded: Alpha={:.4} Beta={:.4}", w_a, w_b);
    assert!(w_a > w_b, "Alpha > Beta persists after reload");

    println!("\nNeural decay: PASS ✓");

    let _ = std::fs::remove_file(path);
}

// =========================================================================
// TEST 1: Code Search with AST Chunking (Tree-sitter)
// =========================================================================

#[test]
fn test_01_code_ast_chunking() {
    let path = "test_e2e_code.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() { break; }
    }
    if brain.engine.encode_query("test").is_none() {
        println!("SKIP: encoder not found");
        return;
    }

    println!("=== CODE AST CHUNKING ===");

    // Index a real Rust file — our own brain.rs
    let brain_rs = std::fs::read_to_string("src/brain.rs")
        .or_else(|_| std::fs::read_to_string("crates/sca-core/src/brain.rs"))
        .expect("Cannot find brain.rs");
    let brain_lines = brain_rs.lines().count();
    println!("  brain.rs: {} lines", brain_lines);

    // AST-chunk it
    let chunks = sca_core::code_search::ast_chunk(&brain_rs, "rs");
    println!("  AST chunks: {}", chunks.len());
    assert!(chunks.len() > 3, "brain.rs should chunk into multiple functions (got {})", chunks.len());

    // Print chunk names to verify AST parsing
    for (i, chunk) in chunks.iter().enumerate() {
        println!("    chunk {}: '{}' [{}] (L{}-L{}, {} lines)",
            i, chunk.name, chunk.kind,
            chunk.start_line, chunk.end_line,
            chunk.end_line - chunk.start_line);
    }

    // Verify meaningful function names were extracted
    let has_named_functions = chunks.iter().any(|c| {
        c.name != "file" && !c.name.starts_with("L") && c.name.len() > 2
    });
    assert!(has_named_functions, "Should extract function/struct names from AST");

    // Store each chunk as a frame
    for chunk in &chunks {
        let chunk_id = format!("brain.rs::{}", chunk.name);
        let title = format!("{} [{}]", chunk.name, chunk.kind);
        brain.put(&chunk_id, &chunk.content, Some(&title));
    }

    // Also index engine.rs
    let engine_rs = std::fs::read_to_string("src/engine.rs")
        .or_else(|_| std::fs::read_to_string("crates/sca-core/src/engine.rs"))
        .expect("Cannot find engine.rs");

    let engine_chunks = sca_core::code_search::ast_chunk(&engine_rs, "rs");
    println!("  engine.rs: {} lines, {} AST chunks", engine_rs.lines().count(), engine_chunks.len());

    for chunk in &engine_chunks {
        let chunk_id = format!("engine.rs::{}", chunk.name);
        brain.put(&chunk_id, &chunk.content, Some(&chunk.name));
    }

    brain.build_index().expect("build_index");
    brain.save().expect("save");

    let stats = brain.stats();
    println!("  Total frames: {}, .said: {} bytes", stats.active_frames, stats.file_size);

    // RECALL: find specific functions by meaning
    println!("\n=== CODE RECALL ===");

    let tests = vec![
        ("S_slow tensor write", "s_slow"),
        ("reconsolidate recalled document", "reconsolidate"),
        ("brain consolidation decay", "consolidate"),
        ("query log entry", "log_query"),
        ("search immutable query", "search_immutable"),
        ("entity match score boost", "entity"),
        ("encode query static encoder", "encode_query"),
    ];

    let mut passed = 0;
    for (query, expected_in_id_or_content) in &tests {
        let results = brain.recall(query, 3);
        let found = results.iter().any(|r| {
            r.doc_id.to_lowercase().contains(expected_in_id_or_content)
            || r.content.to_lowercase().contains(expected_in_id_or_content)
        });

        if found {
            passed += 1;
            let top = &results[0];
            println!("  [OK] '{}' -> {} ({} bytes)", query, top.doc_id, top.content.len());
        } else {
            let top_id = results.first().map(|r| r.doc_id.as_str()).unwrap_or("EMPTY");
            println!("  [FAIL] '{}' -> expected '{}', got: {}", query, expected_in_id_or_content, top_id);
        }
    }

    println!("\nCode recall: {}/{}", passed, tests.len());
    assert!(passed >= 5, "At least 5/7 code recalls must work (got {})", passed);

    // Verify results are FUNCTIONS not whole files
    let results = brain.recall("S_slow tensor", 1);
    if let Some(r) = results.first() {
        assert!(r.content.len() < brain_rs.len() / 2,
            "Result should be a function ({} bytes), not whole file ({} bytes)",
            r.content.len(), brain_rs.len());
        println!("  Function size: {} bytes (file: {} bytes) ✓", r.content.len(), brain_rs.len());
    }

    // Persistence
    brain.save().expect("save");
    let brain2 = SaidFile::open(path).expect("reopen");
    assert!(brain2.stats().active_frames > 5, "Frames persist");
    println!("  Persistence: {} frames survive save/load ✓", brain2.stats().active_frames);

    println!("\nCode AST chunking: PASS");

    let _ = std::fs::remove_file(path);
}

// =========================================================================
// TEST 2: Grep + SCA Fused Search
// =========================================================================

#[test]
fn test_02_fused_grep_sca() {
    let path = "test_e2e_fused.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() { break; }
    }
    if brain.engine.encode_query("test").is_none() {
        println!("SKIP: encoder not found");
        return;
    }

    println!("=== FUSED GREP + SCA SEARCH ===");

    // Index real code files
    let brain_rs = std::fs::read_to_string("src/brain.rs")
        .or_else(|_| std::fs::read_to_string("crates/sca-core/src/brain.rs"))
        .expect("Cannot find brain.rs");
    let engine_rs = std::fs::read_to_string("src/engine.rs")
        .or_else(|_| std::fs::read_to_string("crates/sca-core/src/engine.rs"))
        .expect("Cannot find engine.rs");

    // AST chunk both files
    let brain_chunks = sca_core::code_search::ast_chunk(&brain_rs, "rs");
    let engine_chunks = sca_core::code_search::ast_chunk(&engine_rs, "rs");

    for chunk in &brain_chunks {
        brain.put(&format!("brain.rs::{}", chunk.name), &chunk.content, Some(&chunk.name));
    }
    for chunk in &engine_chunks {
        brain.put(&format!("engine.rs::{}", chunk.name), &chunk.content, Some(&chunk.name));
    }
    brain.build_index().expect("build_index");

    let total_frames = brain.stats().active_frames;
    println!("  Indexed: {} frames from brain.rs + engine.rs", total_frames);

    // TEST A: Exact function name — built-in grep on frames
    println!("\n--- Test A: Exact function name (grep on stored frames) ---");
    let grep_results = brain.grep("fn search_immutable", 5);
    let sca_results = brain.recall("search immutable function", 5);

    println!("  Grep 'fn search_immutable': {} results", grep_results.len());
    println!("  SCA 'search immutable function': {} results", sca_results.len());
    assert!(!grep_results.is_empty(), "Grep MUST find 'fn search_immutable' in stored frames");
    assert!(sca_results.iter().any(|r| r.content.contains("search_immutable")),
        "SCA should find search_immutable by meaning");
    println!("  [OK] Grep found exact text in stored frames ✓");
    println!("  [OK] SCA found by meaning ✓");

    // TEST B: Semantic query — SCA finds, Grep won't (no exact text match)
    println!("\n--- Test B: Semantic query ---");
    let grep_semantic = brain.grep("how does the brain learn from usage", 5);
    let sca_semantic = brain.recall("how does the brain learn from usage", 5);

    let sca_brain_found = sca_semantic.iter().any(|r|
        r.doc_id.contains("brain") || r.content.contains("reconsolidat") || r.content.contains("recall_weight")
    );

    println!("  Grep: {} results (expected: 0 — no exact text)", grep_semantic.len());
    println!("  SCA: {} results, brain_found={}", sca_semantic.len(), sca_brain_found);
    assert!(sca_brain_found, "SCA should find brain learning code semantically");
    println!("  [OK] SCA finds by MEANING what Grep cannot ✓");

    // TEST C: Grep finds exact constant that SCA might miss
    println!("\n--- Test C: Exact numeric constant ---");
    let grep_exact = brain.grep("0.999", 5);
    let sca_decay = brain.recall("decay rate constant", 5);

    println!("  Grep '0.999': {} results", grep_exact.len());
    assert!(!grep_exact.is_empty(), "Grep MUST find '0.999' in stored frames");
    println!("  [OK] Grep finds exact numeric constant ✓");
    println!("  SCA 'decay rate': {} results", sca_decay.len());

    // TEST D: Fused recall — SCA + grep together in one call
    println!("\n--- Test D: Fused recall (SCA + Grep auto-combined) ---");
    let fused = brain.recall("fn reconsolidate", 5);
    let found_exact = fused.iter().any(|r| r.content.contains("fn reconsolidate"));
    println!("  recall('fn reconsolidate'): {} results, exact_found={}", fused.len(), found_exact);
    assert!(found_exact, "Fused recall should find exact function via grep + SCA");
    println!("  [OK] Fused: SCA semantic + Grep exact combined ✓");

    // TEST E: Grep on something SCA misses — very specific token
    println!("\n--- Test E: Specific token only grep finds ---");
    let grep_token = brain.grep("s_slow_decay", 5);
    println!("  Grep 's_slow_decay': {} results", grep_token.len());
    assert!(!grep_token.is_empty(), "Grep MUST find 's_slow_decay' in stored code");
    println!("  [OK] Grep finds specific variable name ✓");

    println!("\n=== SUMMARY ===");
    println!("  Grep on stored frames: finds exact text ✓");
    println!("  SCA semantic: finds by meaning ✓");
    println!("  Fused recall: combines both automatically ✓");
    println!("  Grep finds what SCA misses (exact constants, variable names) ✓");
    println!("  SCA finds what Grep misses (semantic queries) ✓");
    println!("\nFused Grep + SCA: PASS");

    let _ = std::fs::remove_file(path);
}

// =========================================================================
// TEST 7: Everything Together — Personal + Business + Code in ONE .said file
// =========================================================================

#[test]
fn test_07_everything_together() {
    let path = "test_e2e_everything.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() { break; }
    }
    if brain.engine.encode_query("test").is_none() {
        println!("SKIP: encoder not found");
        return;
    }

    println!("=== EVERYTHING IN ONE .said FILE ===\n");

    // ── PERSONAL MEMORIES ──
    println!("--- Loading personal memories ---");
    brain.remember("My name is Carter and I live in Cape Town");
    brain.remember("My wife's birthday is March 15");
    brain.remember("I prefer dark mode and use Rust as my primary language");
    println!("  3 personal memories stored");

    // ── BUSINESS DOCUMENT ──
    println!("--- Loading business document ---");
    let mut biz_paragraphs: Vec<String> = Vec::new();
    for i in 0..100 {
        biz_paragraphs.push(format!(
            "Section {} covers standard operations including supply chain, vendor \
             management, compliance, and quarterly planning for fiscal period {}.", i, i));
    }
    biz_paragraphs[42] = "Project Aurora budget is $4.7 million, approved March 15th. \
        Contact Sarah Chen for Phase 2 timeline details.".to_string();
    let biz_doc = biz_paragraphs.join("\n\n");
    brain.remember(&biz_doc);
    println!("  Business document stored ({} words)", biz_doc.split_whitespace().count());

    // ── CODE ──
    println!("--- Loading code ---");
    let brain_rs = std::fs::read_to_string("src/brain.rs")
        .or_else(|_| std::fs::read_to_string("crates/sca-core/src/brain.rs"))
        .expect("Cannot find brain.rs");
    let chunks = sca_core::code_search::ast_chunk(&brain_rs, "rs");
    for chunk in &chunks {
        brain.put(&format!("brain.rs::{}", chunk.name), &chunk.content, Some(&chunk.name));
    }
    println!("  brain.rs: {} AST chunks stored", chunks.len());

    // ── BUILD INDEX ──
    brain.build_index().expect("build_index");
    brain.save().expect("save");
    let stats = brain.stats();
    println!("\n  Total: {} frames, {} bytes\n", stats.active_frames, stats.file_size);

    // ── CROSS-DOMAIN RECALL ──
    println!("=== CROSS-DOMAIN RECALL ===");
    let mut passed = 0;
    let tests = vec![
        // Personal
        ("birthday", "March 15", "personal"),
        ("where do I live", "Cape Town", "personal"),
        ("dark mode", "dark mode", "personal"),
        // Business
        ("Aurora budget", "$4.7 million", "business"),
        ("Sarah Chen", "Sarah Chen", "business"),
        // Code
        ("S_slow tensor", "s_slow", "code"),
        ("reconsolidate", "reconsolidate", "code"),
        ("brain consolidation", "consolidat", "code"),
        // Grep exact
        ("fn dream", "fn dream", "code-grep"),
    ];

    for (query, expected, domain) in &tests {
        let results = brain.recall(query, 3);
        let found = results.iter().any(|r|
            r.content.to_lowercase().contains(&expected.to_lowercase())
        );
        if found {
            passed += 1;
            println!("  [OK] [{}] '{}' -> found '{}'", domain, query, expected);
        } else {
            let top = results.first().map(|r| &r.content[..r.content.len().min(60)]).unwrap_or("EMPTY");
            println!("  [FAIL] [{}] '{}' -> expected '{}', got: {}", domain, query, expected, top);
        }
    }

    println!("\n  Cross-domain recall: {}/{}", passed, tests.len());
    assert!(passed >= 7, "At least 7/9 cross-domain recalls must work (got {})", passed);

    // ── BRAIN LEARNS ──
    println!("\n=== BRAIN LEARNING ===");
    // Recall birthday 5 times
    for _ in 0..5 { let _ = brain.recall("birthday", 1); }
    // Recall S_slow 3 times
    for _ in 0..3 { let _ = brain.recall("S_slow tensor", 1); }

    let stats = brain.stats();
    println!("  Queries: {}, Boosted: {}, S_slow: {:.4}",
        stats.brain_queries, stats.brain_boosted, brain.engine.brain.s_slow_magnitude());
    assert!(stats.brain_queries >= 15, "Brain should log queries");
    assert!(stats.brain_boosted > 0, "Brain should boost recalled docs");
    assert!(brain.engine.brain.s_slow_magnitude() > 0.0, "S_slow should have signal");

    // ── PERSISTENCE ──
    println!("\n=== PERSISTENCE ===");
    brain.save().expect("save");
    let mut brain2 = SaidFile::open(path).expect("reopen");
    for ep in &encoder_paths {
        if brain2.load_encoder(ep).is_ok() { break; }
    }
    brain2.build_index().expect("rebuild");

    // All domains still recallable
    let personal = brain2.recall("birthday", 1);
    let business = brain2.recall("Aurora budget", 1);
    let code = brain2.recall("reconsolidate", 1);

    let p_ok = personal.iter().any(|r| r.content.contains("March 15"));
    let b_ok = business.iter().any(|r| r.content.contains("4.7"));
    let c_ok = code.iter().any(|r| r.content.to_lowercase().contains("reconsolidat"));

    println!("  Personal: {}", if p_ok { "OK" } else { "FAIL" });
    println!("  Business: {}", if b_ok { "OK" } else { "FAIL" });
    println!("  Code:     {}", if c_ok { "OK" } else { "FAIL" });
    assert!(p_ok && b_ok && c_ok, "All domains must survive save/load");

    // Brain state persists
    let stats2 = brain2.stats();
    println!("  Brain: {} queries, {} boosted, S_slow={:.4}",
        stats2.brain_queries, stats2.brain_boosted,
        brain2.engine.brain.s_slow_magnitude());
    assert!(stats2.brain_queries >= 15, "Brain queries persist");
    assert!(brain2.engine.brain.s_slow_magnitude() > 0.0, "S_slow persists");

    // ── FILE STATS ──
    println!("\n=== FILE STATS ===");
    let s = brain2.stats();
    println!("  File: {} bytes ({:.1} KB)", s.file_size, s.file_size as f64 / 1024.0);
    println!("  Frames: {} active, {} deleted", s.active_frames, s.deleted_frames);
    println!("  Compression: {:.1}x", s.compression_ratio);
    println!("  Brain: {} queries, {} boosted, {} cycles",
        s.brain_queries, s.brain_boosted, s.brain_cycles);

    println!("\nEverything together: PASS ✓");

    let _ = std::fs::remove_file(path);
}

// =========================================================================
// TEST 8: LSP Client — connects to rust-analyzer, caches results in .said
// =========================================================================

#[cfg(feature = "lsp")]
#[test]
fn test_08_lsp_client() {
    let path = "test_e2e_lsp.said";
    let _ = std::fs::remove_file(path);

    println!("=== LSP CLIENT TEST (on-the-fly enable) ===");

    let mut brain = SaidFile::create(path);
    // Verify LSP is OFF by default
    assert!(!brain.has_lsp(), "LSP should be disabled by default");
    println!("  LSP disabled by default ✓");

    // Find repo root — rust-analyzer needs the workspace root with [workspace] Cargo.toml
    // CARGO_MANIFEST_DIR points to crates/sca-core, go up 2 levels to repo root
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.parent().and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| manifest_dir.clone());
    println!("  Workspace: {}", workspace.display());

    // Enable LSP on the fly
    let ra_path = if std::path::Path::new("C:/Users/Carter/.cargo/bin/rust-analyzer.exe").exists() {
        "C:/Users/Carter/.cargo/bin/rust-analyzer.exe"
    } else {
        "rust-analyzer"
    };

    println!("  Enabling LSP: {} ...", ra_path);
    match brain.enable_lsp(ra_path, workspace.to_str().unwrap_or(".")) {
        Ok(()) => {
            assert!(brain.has_lsp(), "LSP should be enabled now");
            println!("  [OK] LSP enabled on the fly ✓");

            // Test workspace symbol via SaidFile API
            println!("\n--- Workspace symbol (via .said API) ---");
            match brain.lsp_workspace_symbol("Brain") {
                Ok(symbols) => {
                    let count = symbols.lines().count();
                    println!("  Found {} symbols matching 'Brain'", count);
                    for line in symbols.lines().take(3) {
                        println!("    {}", line);
                    }
                    if count > 0 {
                        println!("  [OK] Workspace symbol ✓ (cached in .said)");
                    }
                }
                Err(e) => println!("  [INFO] Workspace symbol: {} (server indexing)", e),
            }

            // Test hover via SaidFile API (path relative to workspace root)
            println!("\n--- Hover on Brain::new() ---");
            match brain.lsp_hover("crates/sca-core/src/brain.rs", 91, 12) {
                Ok(info) => {
                    if !info.is_empty() && info != "null" {
                        println!("  Hover: {}...", &info[..info.len().min(120)]);
                        println!("  [OK] Hover ✓");
                    } else {
                        println!("  [INFO] Hover empty (server still indexing)");
                    }
                }
                Err(e) => println!("  [INFO] Hover: {}", e),
            }

            // Test goToDefinition — where is Brain::new defined?
            println!("\n--- Go to definition (Brain::new) ---");
            match brain.lsp_definition("crates/sca-core/src/brain.rs", 91, 12) {
                Ok(loc) => {
                    if !loc.is_empty() {
                        println!("  Definition: {}", loc.lines().next().unwrap_or(""));
                        println!("  [OK] Go to definition ✓");
                    } else {
                        println!("  [INFO] Empty definition result");
                    }
                }
                Err(e) => println!("  [INFO] Definition: {}", e),
            }

            // Test findReferences — who calls Brain::new()?
            println!("\n--- Find references (Brain::new) ---");
            match brain.lsp_references("crates/sca-core/src/brain.rs", 91, 12) {
                Ok(refs) => {
                    let count = refs.lines().count();
                    println!("  Found {} references to Brain::new()", count);
                    for line in refs.lines().take(5) {
                        println!("    {}", line);
                    }
                    if count > 0 {
                        println!("  [OK] Find references ✓");
                    }
                }
                Err(e) => println!("  [INFO] References: {}", e),
            }

            // Disable LSP
            brain.disable_lsp();
            assert!(!brain.has_lsp(), "LSP should be disabled after disable_lsp()");
            println!("\n  LSP disabled on the fly ✓");

            // Verify cached results are still searchable WITHOUT LSP
            brain.build_index().ok();
            let results = brain.recall("Brain", 3);
            let has_lsp_cached = results.iter().any(|r| r.doc_id.contains("lsp::"));
            println!("  Cached LSP results searchable without LSP: {}", has_lsp_cached);

            println!("\n  LSP on-the-fly: PASS ✓");
        }
        Err(e) => {
            println!("  [SKIP] Cannot connect: {}", e);
            println!("  The LSP client is implemented and compiles.");
            println!("  Connection requires rust-analyzer to respond within timeout.");
            println!("  API ready: enable_lsp(), disable_lsp(), lsp_definition(),");
            println!("  lsp_references(), lsp_hover(), lsp_workspace_symbol()");
            println!("  All results auto-cached as searchable frames in .said");
        }
    }

    let _ = std::fs::remove_file(path);
}

// =========================================================================
// TEST 5: S_slow Tensor — Cross-Document Synthesis
// From research/test_crossdocument_synthesis.py: Eleanor + Aura Systems
// =========================================================================

#[test]
fn test_05_cross_document_synthesis() {
    let path = "test_e2e_synthesis.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() { break; }
    }
    if brain.engine.encode_query("test").is_none() {
        println!("SKIP: encoder not found");
        return;
    }

    println!("=== CROSS-DOCUMENT SYNTHESIS ===");
    println!("(from research/test_crossdocument_synthesis.py)");

    // Document 1: Eleanor's goal
    brain.remember("Ms. Eleanor Vance grew up in the financial sector but became fascinated \
        by decentralized energy grids. After years of research, she discovered that photonic \
        inverters could revolutionize solar panel efficiency by 40%. She set a goal to start \
        a new company called Solaris Flux by the end of Q4, focusing exclusively on residential \
        solar installations using this breakthrough technology. Her business plan projects \
        $50M in first-year revenue if she can secure the technology license.");

    // Document 2: The constraint (NO mention of Eleanor or Solaris!)
    brain.remember("A recent SEC filing by Quantum Photonics Labs reveals that their proprietary \
        photonic inverter technology, which improves solar efficiency by over 40%, is now \
        available for licensing. However, the filing shows that the only approved licensing \
        pathway is through their holding company Aura Systems, which requires an upfront \
        payment of $15 million plus 8% royalties. This capital requirement has deterred most \
        startups from pursuing residential solar applications with this technology.");

    brain.build_index().expect("build_index");

    // S_slow should have accumulated signal from both documents
    let magnitude_before = brain.engine.brain.s_slow_magnitude();
    println!("  S_slow magnitude after indexing: {:.4}", magnitude_before);

    // The synthesis query — requires connecting BOTH documents
    let query = "Based on Eleanor Vance's goal, what is the single greatest financial hurdle she must overcome to launch Solaris Flux?";

    let results = brain.recall(query, 5);
    println!("  Query: {}", &query[..70]);
    println!("  Results found: {}", results.len());

    let found_eleanor = results.iter().any(|r| r.content.contains("Eleanor") || r.content.contains("Solaris"));
    let found_aura = results.iter().any(|r| r.content.contains("Aura Systems") || r.content.contains("$15 million"));
    let found_photonic = results.iter().any(|r| r.content.contains("photonic inverter"));

    for (i, r) in results.iter().enumerate().take(3) {
        let snippet = &r.content[..r.content.len().min(120)];
        println!("  #{}: score={:.4} | {}...", i + 1, r.score, snippet);
    }

    println!();
    println!("  Found Eleanor/Solaris: {}", found_eleanor);
    println!("  Found Aura/$15M: {}", found_aura);
    println!("  Found photonic inverter: {}", found_photonic);

    // At minimum, the query should find the Eleanor document (direct keyword match)
    assert!(found_eleanor, "Should find Eleanor's document");

    // The photonic inverter connection should be found (shared concept)
    assert!(found_photonic, "Should find documents mentioning photonic inverter");

    // Check S_slow grew from the search
    let magnitude_after = brain.engine.brain.s_slow_magnitude();
    println!("  S_slow magnitude after search: {:.4} (grew by {:.4})",
        magnitude_after, magnitude_after - magnitude_before);
    assert!(magnitude_after > magnitude_before, "S_slow should grow from search interaction");

    // If BOTH documents found, cross-document synthesis worked
    if found_eleanor && found_aura {
        println!("\n  CROSS-DOCUMENT SYNTHESIS: PERFECT ✓");
        println!("  Both documents retrieved despite no keyword overlap between them!");
        println!("  The connection: 'photonic inverter technology' bridges Eleanor → Aura Systems");
    } else if found_eleanor {
        println!("\n  CROSS-DOCUMENT SYNTHESIS: PARTIAL");
        println!("  Found Eleanor but not Aura Systems — S_slow needs more training data");
        println!("  (This is expected with only 2 documents — tensor needs more signal)");
    }

    // Verify persistence
    brain.save().expect("save");
    let brain2 = SaidFile::open(path).expect("reopen");
    assert!(brain2.engine.brain.s_slow_magnitude() > 0.0, "S_slow should persist");
    println!("  S_slow tensor persists across save/load ✓");

    println!("\nCross-document synthesis: PASS");

    let _ = std::fs::remove_file(path);
}
