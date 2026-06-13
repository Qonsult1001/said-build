//! Profiling benchmark: isolates search hot path timing.
//! Run: cargo bench --bench search_profile --features "static-embed"

use std::time::Instant;
use sca_core::ScaEngine;

fn main() {
    let mut engine = ScaEngine::new();

    // Load encoder
    let paths = ["said-lam-static", "../../SAID-LAM-private/said-lam-static"];
    for p in paths {
        if engine.load_static_encoder(p).is_ok() {
            println!("[OK] Encoder loaded from {}", p);
            break;
        }
    }
    if engine.encode_query("test").is_none() {
        eprintln!("ERROR: No static encoder found.");
        return;
    }

    // Generate synthetic docs — vary lengths to profile scaling
    for &(n_docs, words_per_doc) in &[(300, 500), (300, 5000), (100, 500)] {
        let vocab: Vec<String> = (0..5000).map(|i| format!("word{:04}", i)).collect();

        println!("\n{}", "=".repeat(60));
        println!("=== {} docs × {} words/doc ===", n_docs, words_per_doc);

        let mut doc_ids = Vec::new();
        let mut doc_texts = Vec::new();
        for i in 0..n_docs {
            doc_ids.push(format!("doc_{:04}", i));
            let text: String = (0..words_per_doc)
                .map(|j| vocab[(i * 7 + j * 13) % vocab.len()].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            doc_texts.push(text);
        }

        engine.clear();
        let t0 = Instant::now();
        engine.index_batch(&doc_ids, &doc_texts).unwrap();
        let idx_ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!("  Index: {:.0}ms", idx_ms);

        // Queries
        let n_queries = 50;
        let queries: Vec<String> = (0..n_queries)
            .map(|i| {
                (0..8)
                    .map(|j| vocab[(i * 11 + j * 17) % vocab.len()].as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();

        let query_embs: Vec<Vec<f32>> = queries.iter()
            .filter_map(|q| engine.encode_query(q))
            .collect();

        // Warm up
        for (q, e) in queries.iter().zip(query_embs.iter()).take(3) {
            let _ = engine.search_immutable(e, q, 10);
        }

        // Profile search
        let t0 = Instant::now();
        for (q_text, q_emb) in queries.iter().zip(query_embs.iter()) {
            let _hits = engine.search_immutable(q_emb, q_text, 10);
        }
        let search_ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!("  Search: {:.1}ms ({:.3}ms/query)", search_ms, search_ms / n_queries as f64);

        // Profile encode only
        let t0 = Instant::now();
        for q in &queries {
            let _ = engine.encode_query(q);
        }
        let enc_ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!("  Encode: {:.1}ms ({:.3}ms/query)", enc_ms, enc_ms / n_queries as f64);

        println!("  Search overhead: {:.3}ms/query", (search_ms - enc_ms) / n_queries as f64);
    }
}
