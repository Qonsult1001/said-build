//! Block 256 compression test: H.265 GOP-inspired standard for .said files.
//!
//! Verifies: store → compact (block 256 + dict) → save → reopen → read integrity.
//!
//! Run: cargo test -p sca-core --release --features "static-embed" --test test_zstd_dict -- --nocapture

#[cfg(feature = "static-embed")]
#[test]
fn test_block256_standard() {
    use sca_core::said_file::SaidFile;
    use std::time::Instant;

    println!("\n======================================================================");
    println!("BLOCK 256 STANDARD COMPRESSION TEST");
    println!("======================================================================");

    let path = "test_block256.said";
    let _ = std::fs::remove_file(path);

    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];

    let mut brain = SaidFile::create(path);
    let mut loaded = false;
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() {
            println!("  Encoder: {}", ep);
            loaded = true;
            break;
        }
    }
    if !loaded {
        println!("SKIP: no encoder found");
        let _ = std::fs::remove_file(path);
        return;
    }

    // Store 300 docs
    let mut total_text_bytes: usize = 0;
    let docs: Vec<(String, String)> = (0..300).map(|i| {
        let doc_id = format!("doc_{}", i);
        let text = generate_wiki_doc(i);
        (doc_id, text)
    }).collect();
    total_text_bytes = docs.iter().map(|(_, t)| t.len()).sum();

    for (doc_id, text) in &docs {
        brain.remember_as(doc_id, text, Some(&format!("Article {}", doc_id)));
    }
    brain.build_index().expect("build_index failed");

    println!("  Raw text: {} bytes ({:.1}KB)", total_text_bytes, total_text_bytes as f64 / 1024.0);
    println!("  Documents: {}", docs.len());

    // Compact with new standard (Block 256 + Dict)
    let t0 = Instant::now();
    let (blocks, saved) = brain.compact();
    let compact_ms = t0.elapsed().as_millis();

    brain.save().expect("save failed");
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    println!("\n  Compact:     {}ms ({} blocks, {} bytes saved)", compact_ms, blocks, saved);
    println!("  File size:   {} bytes ({:.1}KB)", file_size, file_size as f64 / 1024.0);
    println!("  Ratio:       {:.2}x", total_text_bytes as f64 / file_size as f64);
    println!("  Per doc:     {} bytes", file_size / 300);

    // Reopen and verify ALL docs
    let mut reopen = SaidFile::open(path).expect("open failed");
    let mut ok = 0;
    for (doc_id, original) in &docs {
        if let Some(content) = reopen.read(doc_id) {
            if content == *original { ok += 1; }
        }
    }
    println!("  Integrity:   {}/{} docs verified after reopen", ok, docs.len());
    assert_eq!(ok, docs.len(), "Content mismatch after reopen!");

    println!("\n  PASS: Block 256 standard — compact + save + reopen + read");
    println!("======================================================================");

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
