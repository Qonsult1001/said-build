//! Speed benchmark: recall() on frames with 300 documents.
//! This is the PRODUCTION path — frames in memory, no FFI overhead.

#[cfg(feature = "static-embed")]
#[test]
fn test_recall_speed_300_docs() {
    use sca_core::said_file::SaidFile;
    use std::time::Instant;

    let path = "test_speed_300.said";
    let _ = std::fs::remove_file(path);

    let mut brain = SaidFile::create(path);

    // Load static encoder
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];
    let mut loaded = false;
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() {
            loaded = true;
            break;
        }
    }
    if !loaded {
        println!("SKIP: no encoder found");
        let _ = std::fs::remove_file(path);
        return;
    }

    // Generate 300 realistic documents (Wikipedia-style, ~500 words each)
    println!("Storing 300 documents in frames...");
    let t0 = Instant::now();
    for i in 0..300 {
        let doc_id = format!("doc_{}", i);
        // Mix of different content types to simulate WikimQA
        let text = match i % 6 {
            0 => format!(
                "Prince Albert of Saxony was born in Dresden in 1828. He was the son of King Johann \
                 and Queen Amalie. Albert served as King of Saxony from 1873 to 1902. He married \
                 Princess Carola of Vasa in 1853. Their marriage produced no children. Albert was \
                 known for his military reforms and modernization of the Saxon army. He fought in \
                 the Austro-Prussian War of 1866 and the Franco-Prussian War of 1870. Document {}.", i),
            1 => format!(
                "The Battle of Thermopylae was fought between an alliance of Greek city-states led \
                 by King Leonidas of Sparta and the Persian Empire of Xerxes I over three days during \
                 the second Persian invasion of Greece. It took place simultaneously with the naval \
                 battle at Artemisium in 480 BC at the narrow coastal pass of Thermopylae. Document {}.", i),
            2 => format!(
                "Machine learning is a subset of artificial intelligence that focuses on building \
                 systems that learn from data. Deep learning uses neural networks with many layers. \
                 Transformers have revolutionized natural language processing since 2017. BERT and \
                 GPT models use attention mechanisms for contextual understanding. Document {}.", i),
            3 => format!(
                "The Amazon rainforest produces roughly 20 percent of the world's oxygen. It covers \
                 5.5 million square kilometers across nine countries. The biodiversity includes over \
                 40000 plant species and 1300 bird species. Deforestation rates have increased since \
                 2019 due to agricultural expansion and illegal logging. Document {}.", i),
            4 => format!(
                "Quantum computing uses quantum-mechanical phenomena such as superposition and \
                 entanglement to perform computation. A quantum computer uses qubits instead of \
                 classical bits. Google achieved quantum supremacy in 2019 with their Sycamore \
                 processor completing a task in 200 seconds. IBM has deployed 127-qubit systems. Document {}.", i),
            _ => format!(
                "Rust is a systems programming language focused on safety speed and concurrency. \
                 It achieves memory safety without garbage collection through its ownership model \
                 and borrow checker. The compiler enforces these rules at compile time with zero \
                 runtime cost. Rust has been voted most loved language for seven years. Document {}.", i),
        };
        brain.remember_as(&doc_id, &text, Some(&format!("Document {}", i)));
    }
    let store_ms = t0.elapsed().as_millis();
    println!("  Stored 300 docs in {}ms", store_ms);

    // Build index
    let t0 = Instant::now();
    brain.build_index().expect("build_index failed");
    let index_ms = t0.elapsed().as_millis();
    println!("  Index built in {}ms", index_ms);

    // Queries that test all layers of recall():
    let queries = vec![
        "Prince Albert of Saxony",           // exact entity match
        "quantum computing qubits",          // semantic
        "machine learning neural networks",  // semantic
        "Amazon rainforest biodiversity",     // semantic
        "Rust memory safety ownership",       // semantic
        "King Leonidas Sparta battle",        // entity + semantic
        "where was Prince Albert born",       // multi-hop style
        "who married Princess Carola",        // entity chain
        "deforestation Amazon rates",         // semantic
        "Google quantum supremacy Sycamore",  // specific entities
    ];

    // Warm up
    for q in &queries {
        let _ = brain.recall(q, 5);
    }

    // Timed run
    println!("\n  Recall speed (300 docs in frames, production path):");
    let mut total_ms = 0.0;
    let mut times: Vec<f64> = Vec::new();

    for q in &queries {
        let t0 = Instant::now();
        let results = brain.recall(q, 5);
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        times.push(elapsed);
        total_ms += elapsed;
        let top = results.first().map(|r| r.doc_id.as_str()).unwrap_or("NONE");
        println!("    {:.2}ms | '{}' -> {}", elapsed, q, top);
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = total_ms / times.len() as f64;
    let p50 = times[times.len() / 2];
    let p99 = times[times.len() - 1];

    println!("\n  avg:  {:.2}ms", avg);
    println!("  p50:  {:.2}ms", p50);
    println!("  p99:  {:.2}ms", p99);
    println!("  total: {:.2}ms for {} queries", total_ms, queries.len());

    // Assert speed target: <50ms avg on 300 docs
    assert!(avg < 50.0,
        "Recall too slow: {:.2}ms avg (target: <50ms)", avg);

    let _ = std::fs::remove_file(path);
    println!("\n  PASS: recall on 300 frames at {:.2}ms avg", avg);
}
