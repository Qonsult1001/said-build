//! .said PRODUCT test: store 300 WikimQA docs in frames, save, recall.
//! Shows: file size, recall speed, recall accuracy.
//! This is the REAL product path — frames in memory, no FFI, no Python.
//!
//! Run: cargo test -p sca-core --release --features "static-embed" --test test_said_wiki_product -- --nocapture

#[cfg(feature = "static-embed")]
#[test]
fn test_said_wiki_300_docs() {
    use sca_core::said_file::SaidFile;
    use std::time::Instant;

    let path = "test_wiki_product.said";
    let _ = std::fs::remove_file(path);

    // Load encoder
    let mut brain = SaidFile::create(path);
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    let mut loaded = false;
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() {
            println!("Encoder: {}", ep);
            loaded = true;
            break;
        }
    }
    if !loaded {
        println!("SKIP: no encoder found");
        let _ = std::fs::remove_file(path);
        return;
    }

    // ========================================================================
    // STEP 1: Store 300 documents in frames (simulating WikimQA corpus)
    // ========================================================================
    println!("\n======================================================================");
    println!(".SAID PRODUCT TEST: 300 DOCUMENTS");
    println!("======================================================================");

    // Generate 300 realistic Wikipedia-style documents (~500 words each)
    // Mixed content: historical figures, science, geography, technology
    let t0 = Instant::now();
    let mut total_text_bytes: usize = 0;

    for i in 0..300 {
        let doc_id = format!("doc_{}", i);
        let text = generate_wiki_doc(i);
        total_text_bytes += text.len();
        brain.remember_as(&doc_id, &text, Some(&format!("Wikipedia Article {}", i)));
    }
    let store_ms = t0.elapsed().as_millis();

    println!("\n  STORE:");
    println!("    300 documents stored in {}ms", store_ms);
    println!("    Raw text: {} bytes ({:.1}KB)", total_text_bytes, total_text_bytes as f64 / 1024.0);

    // ========================================================================
    // STEP 2: Build SCA index
    // ========================================================================
    let t0 = Instant::now();
    brain.build_index().expect("build_index failed");
    let index_ms = t0.elapsed().as_millis();
    println!("\n  INDEX:");
    println!("    Built in {}ms", index_ms);

    // ========================================================================
    // STEP 3: Save to .said file
    // ========================================================================
    let t0 = Instant::now();
    brain.save().expect("save failed");
    let save_ms = t0.elapsed().as_millis();
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    println!("\n  SAVE:");
    println!("    Saved in {}ms", save_ms);
    println!("    .said file: {} bytes ({:.1}KB)", file_size, file_size as f64 / 1024.0);
    println!("    Compression: {:.1}x ({:.1}KB raw -> {:.1}KB .said)",
        total_text_bytes as f64 / file_size as f64,
        total_text_bytes as f64 / 1024.0,
        file_size as f64 / 1024.0);
    println!("    Bytes per doc: {}", file_size / 300);

    // ========================================================================
    // STEP 4: Reopen from disk (proves persistence)
    // ========================================================================
    let t0 = Instant::now();
    let mut brain2 = SaidFile::open(path).expect("open failed");
    let open_ms = t0.elapsed().as_millis();

    // Reload encoder (needed for search after reopen)
    for ep in &encoder_paths {
        if brain2.load_encoder(ep).is_ok() { break; }
    }

    // Rebuild index from loaded frames
    let t0 = Instant::now();
    brain2.build_index().expect("rebuild index failed");
    let rebuild_ms = t0.elapsed().as_millis();

    println!("\n  REOPEN:");
    println!("    Opened in {}ms", open_ms);
    println!("    Index rebuilt in {}ms", rebuild_ms);

    // ========================================================================
    // STEP 5: Recall queries — measure speed + accuracy
    // ========================================================================
    // Each query targets one of the 6 templates (0..=5). The test corpus has
    // 50 copies of each template (doc_i where i % 6 = template_id), so correct
    // means "top-5 contains any doc from the target template family".
    //
    // Previous version used `starts_with("doc_N")` which only matched doc_N
    // and doc_N0..doc_N9 — missing most template instances and producing
    // false MISS readings. Now we check (doc_id_number % 6 == template_id)
    // which correctly identifies all 50 members of each template family.
    let queries: Vec<(&str, usize)> = vec![
        ("Prince Albert born in Dresden", 0),
        ("Battle of Thermopylae Greek Sparta", 1),
        ("machine learning deep learning transformers", 2),
        ("Amazon rainforest oxygen biodiversity", 3),
        ("quantum computing qubits superposition", 4),
        ("Rust memory safety ownership borrow checker", 5),
        ("King Johann Queen Amalie Saxony", 0),
        ("Xerxes Persian invasion 480 BC", 1),
        ("BERT GPT attention mechanisms", 2),
        ("deforestation agricultural expansion", 3),
        ("Google Sycamore quantum supremacy", 4),
        ("compiler enforces rules zero runtime cost", 5),
        ("Princess Carola of Vasa married 1853", 0),
        ("Leonidas narrow coastal pass", 1),
        ("neural networks contextual understanding", 2),
    ];

    // Warm up
    for (q, _) in &queries {
        let _ = brain2.recall(q, 5);
    }

    /// Extract the numeric index from a doc_id like "doc_42" -> 42.
    fn doc_index(doc_id: &str) -> Option<usize> {
        doc_id.strip_prefix("doc_").and_then(|s| s.parse::<usize>().ok())
    }

    println!("\n  RECALL (300 docs in frames, from disk):");
    let mut total_ms = 0.0;
    let mut times: Vec<f64> = Vec::new();
    let mut correct = 0;

    for (query, expected_template) in &queries {
        let t0 = Instant::now();
        let results = brain2.recall(query, 5);
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        times.push(elapsed);
        total_ms += elapsed;

        let top_id = results.first().map(|r| r.doc_id.as_str()).unwrap_or("NONE");
        // Template-family check: any top-5 result whose index % 6 matches.
        // This correctly credits ALL 50 template instances (doc_N, doc_N+6, ...).
        let found = results.iter().any(|r| {
            doc_index(&r.doc_id).map(|i| i % 6 == *expected_template).unwrap_or(false)
        });
        if found { correct += 1; }

        let status = if found { "OK" } else { "MISS" };
        println!("    {:.2}ms [{}] '{}' -> {}", elapsed, status, query, top_id);
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = total_ms / times.len() as f64;
    let p50 = times[times.len() / 2];
    let p99 = times[times.len() - 1];

    // ========================================================================
    // STEP 6: Stats
    // ========================================================================
    let stats = brain2.stats();

    println!("\n  SPEED:");
    println!("    avg:   {:.2}ms/query", avg);
    println!("    p50:   {:.2}ms", p50);
    println!("    p99:   {:.2}ms", p99);
    println!("    total: {:.2}ms for {} queries", total_ms, queries.len());

    println!("\n  ACCURACY:");
    println!("    {}/{} queries found correct doc in top-5", correct, queries.len());

    println!("\n  STATS:");
    println!("    Active frames: {}", stats.active_frames);
    println!("    File size:     {} bytes ({:.1}KB)", stats.file_size, stats.file_size as f64 / 1024.0);
    println!("    Index docs:    {}", stats.index_docs);
    println!("    Brain queries: {}", stats.brain_queries);

    println!("\n  ========================================");
    println!("  .said file: {:.1}KB for 300 docs", file_size as f64 / 1024.0);
    println!("  Recall:     {:.2}ms avg", avg);
    println!("  Accuracy:   {}/{}", correct, queries.len());
    println!("  ========================================");

    let _ = std::fs::remove_file(path);
}

fn generate_wiki_doc(i: usize) -> String {
    match i % 6 {
        0 => format!(
            "Prince Albert of Saxony was born in Dresden in 1828. He was the son of King Johann \
             and Queen Amalie Auguste. Albert served as King of Saxony from 1873 to 1902. He married \
             Princess Carola of Vasa in 1853. Their marriage produced no children. Albert was \
             known for his military reforms and modernization of the Saxon army. He fought in \
             the Austro-Prussian War of 1866 and the Franco-Prussian War of 1870-71. As king, \
             he oversaw significant industrial development in Saxony, including the expansion of \
             railways and the growth of Leipzig as a major commercial center. He was considered a \
             progressive monarch who supported education and the arts. Albert died on June 19, 1902 \
             at Castle Sibyllenort in Silesia. He was succeeded by his brother George. Document {}.", i),
        1 => format!(
            "The Battle of Thermopylae was fought between an alliance of Greek city-states led \
             by King Leonidas I of Sparta and the Persian Empire of Xerxes I over three days during \
             the second Persian invasion of Greece in 480 BC. It took place at the narrow coastal \
             pass of Thermopylae in central Greece. The Greek force of approximately 7000 men \
             held the pass against the vastly larger Persian army estimated at 100000 to 300000. \
             The Greeks held for two full days before being outflanked via a mountain path. Leonidas \
             and his 300 Spartans along with 700 Thespians chose to remain and fight to the death, \
             allowing the rest of the Greek forces to retreat. The sacrifice became legendary and \
             inspired the Greek city-states to unite against Persia. Document {}.", i),
        2 => format!(
            "Machine learning is a subset of artificial intelligence that focuses on building \
             systems that learn from and make decisions based on data. Deep learning uses neural \
             networks with many layers to learn hierarchical representations. The transformer \
             architecture introduced in 2017 revolutionized natural language processing. BERT \
             from Google and GPT from OpenAI use attention mechanisms for contextual understanding \
             of text. These models are pre-trained on massive corpora and then fine-tuned for \
             specific tasks. Recent advances include instruction tuning, reinforcement learning \
             from human feedback, and chain-of-thought reasoning. The field continues to evolve \
             rapidly with new architectures and training techniques. Document {}.", i),
        3 => format!(
            "The Amazon rainforest produces roughly 20 percent of the world's oxygen and covers \
             approximately 5.5 million square kilometers across nine South American countries. \
             It is home to an estimated 10 percent of all species on Earth, making it the most \
             biodiverse place on the planet. The forest contains over 40000 plant species, 1300 \
             bird species, 3000 types of fish, and 427 mammal species. Deforestation rates have \
             increased significantly since 2019 due to agricultural expansion, cattle ranching, \
             and illegal logging. The Amazon River, which flows through the forest, is the largest \
             river by volume of water in the world. Indigenous communities have lived in the \
             Amazon for thousands of years. Document {}.", i),
        4 => format!(
            "Quantum computing uses quantum-mechanical phenomena such as superposition and \
             entanglement to perform computation. Unlike classical computers that use bits which \
             can be either 0 or 1, quantum computers use qubits that can exist in multiple states \
             simultaneously. Google achieved quantum supremacy in 2019 with their Sycamore \
             processor, completing a calculation in 200 seconds that would take a classical \
             supercomputer approximately 10000 years. IBM has deployed systems with over 127 \
             qubits and plans to reach 100000 qubits by 2033. Applications include cryptography, \
             drug discovery, materials science, and optimization problems. Document {}.", i),
        _ => format!(
            "Rust is a systems programming language focused on safety, speed, and concurrency. \
             It achieves memory safety without garbage collection through its ownership model \
             and borrow checker. The compiler enforces these rules at compile time with zero \
             runtime cost. Rust has been voted the most loved programming language in Stack \
             Overflow surveys for seven consecutive years. Major companies using Rust include \
             Mozilla, Google, Microsoft, Amazon, and Meta. The language is particularly suited \
             for systems programming, web assembly, embedded systems, and command-line tools. \
             Cargo, Rust's package manager, makes dependency management straightforward. Document {}.", i),
    }
}
