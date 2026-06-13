//! DEFINITIVE WikiMQA test: Block 256 + mmap .said file.
//! 300/300 recall + 20 individual recall samples with returned text.
//!
//! Run: cargo test -p sca-core --release --features "static-embed" --test test_zstd_dict_wikimqa -- --nocapture

#[cfg(feature = "static-embed")]
#[test]
fn test_block256_wikimqa_full() {
    use sca_core::said_file::SaidFile;
    use std::collections::HashMap;
    use std::time::Instant;

    let base_paths = ["../../SAID-LAM-private/tests", "../SAID-LAM-private/tests", "."];
    let base = base_paths.iter().find(|p| std::path::Path::new(&format!("{}/wikimqa_corpus.tsv", p)).exists());
    let base = match base {
        Some(b) => *b,
        None => { println!("SKIP: wikimqa data not found."); return; }
    };

    let corpus_tsv = std::fs::read_to_string(format!("{}/wikimqa_corpus.tsv", base)).unwrap();
    let queries_tsv = std::fs::read_to_string(format!("{}/wikimqa_queries.tsv", base)).unwrap();
    let qrels_tsv = std::fs::read_to_string(format!("{}/wikimqa_qrels.tsv", base)).unwrap();

    let docs: Vec<(String, String)> = corpus_tsv.lines().filter(|l| !l.is_empty())
        .filter_map(|l| { let mut p = l.splitn(2, '\t'); Some((p.next()?.into(), p.next()?.replace("\\n", "\n"))) })
        .collect();
    let queries: Vec<(String, String)> = queries_tsv.lines().filter(|l| !l.is_empty())
        .filter_map(|l| { let mut p = l.splitn(2, '\t'); Some((p.next()?.into(), p.next()?.replace("\\n", "\n"))) })
        .collect();
    let qrels: HashMap<String, String> = qrels_tsv.lines().filter(|l| !l.is_empty())
        .filter_map(|l| { let mut p = l.splitn(2, '\t'); Some((p.next()?.into(), p.next()?.into())) })
        .collect();

    let total_text_bytes: usize = docs.iter().map(|(_, t)| t.len()).sum();

    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
        "said-lam-static",
    ];

    let said_path = "test_wikimqa_definitive.said";
    let _ = std::fs::remove_file(said_path);

    // ========================================================================
    // PHASE 1: INGEST
    // ========================================================================
    let mut brain = SaidFile::create(said_path);
    let mut loaded = false;
    for ep in &encoder_paths { if brain.load_encoder(ep).is_ok() { loaded = true; break; } }
    if !loaded { println!("SKIP: no encoder"); return; }

    let t_store = Instant::now();
    for (doc_id, text) in &docs { brain.remember_as(doc_id, text, Some(doc_id)); }
    let store_ms = t_store.elapsed().as_millis();

    let t_idx = Instant::now();
    brain.build_index().expect("index");
    let index_ms = t_idx.elapsed().as_millis();

    let t_compact = Instant::now();
    let (blocks, _) = brain.compact();
    let compact_ms = t_compact.elapsed().as_millis();

    let t_save = Instant::now();
    brain.save().expect("save");
    let save_ms = t_save.elapsed().as_millis();

    let file_size = std::fs::metadata(said_path).map(|m| m.len()).unwrap_or(0);
    drop(brain);

    // ========================================================================
    // PHASE 2: REOPEN (mmap)
    // ========================================================================
    let t_open = Instant::now();
    let mut brain2 = SaidFile::open(said_path).expect("open (mmap)");
    let open_ms = t_open.elapsed().as_millis();

    for ep in &encoder_paths { if brain2.load_encoder(ep).is_ok() { break; } }

    let t_rebuild = Instant::now();
    brain2.build_index().expect("rebuild");
    let rebuild_ms = t_rebuild.elapsed().as_millis();

    // ========================================================================
    // PHASE 3: 300/300 RECALL
    // ========================================================================
    let _ = brain2.recall("warmup", 5);

    let mut correct = 0;
    let mut times: Vec<f64> = Vec::new();

    for (qid, qtext) in &queries {
        let expected = match qrels.get(qid) { Some(d) => d, None => continue };
        let t0 = Instant::now();
        let results = brain2.recall(qtext, 10);
        times.push(t0.elapsed().as_secs_f64() * 1000.0);
        if results.iter().any(|r| r.doc_id == *expected) { correct += 1; }
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = times.len();
    let total_ms: f64 = times.iter().sum();
    let avg = total_ms / n as f64;
    let p50 = times[n / 2];
    let p99 = times[(n as f64 * 0.99) as usize];

    // ========================================================================
    // PHASE 4: 20 INDIVIDUAL RECALL SAMPLES (show returned text)
    // ========================================================================
    let sample_indices = [0, 5, 15, 25, 42, 55, 65, 77, 88, 99,
                          110, 133, 150, 175, 193, 208, 234, 250, 275, 299];
    let mut sample_results: Vec<(String, String, f64, String, usize, bool)> = Vec::new();

    for &idx in &sample_indices {
        if idx >= queries.len() { continue; }
        let (qid, qtext) = &queries[idx];
        let expected = qrels.get(qid).map(|s| s.as_str()).unwrap_or("?");

        let t0 = Instant::now();
        let results = brain2.recall(qtext, 1);
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;

        let (got_id, text_len, snippet) = if let Some(r) = results.first() {
            let snippet: String = r.content.chars().take(120).collect();
            (r.doc_id.clone(), r.content.len(), snippet)
        } else {
            ("NONE".into(), 0, String::new())
        };

        let hit = got_id == expected;
        sample_results.push((qid.clone(), got_id, elapsed, snippet, text_len, hit));
    }

    // ========================================================================
    // PHASE 5: SINGLE FRAME READ SPEED
    // ========================================================================
    let read_ids = ["doc_0", "doc_50", "doc_100", "doc_150", "doc_200", "doc_299"];
    let mut read_times: Vec<(String, u128, usize)> = Vec::new();
    for doc_id in &read_ids {
        let t0 = Instant::now();
        let text = brain2.read(doc_id);
        let us = t0.elapsed().as_micros();
        let len = text.as_ref().map(|t| t.len()).unwrap_or(0);
        read_times.push((doc_id.to_string(), us, len));
    }

    // ========================================================================
    // OUTPUT
    // ========================================================================
    println!("\n======================================================================");
    println!(".SAID FILE: REAL WIKIMQA (300 docs, {:.1}MB raw)", total_text_bytes as f64 / 1024.0 / 1024.0);
    println!("======================================================================");
    println!("  Store:               {}ms", store_ms);
    println!("  Index:               {}ms", index_ms);
    println!("  Compact:             {}ms ({} blocks)", compact_ms, blocks);
    println!("  Save:                {}ms", save_ms);
    println!();
    println!("  Raw text:            {} bytes ({:.1}MB)", total_text_bytes, total_text_bytes as f64 / 1024.0 / 1024.0);
    println!("  .said (block 256):   {} bytes ({:.1}KB)", file_size, file_size as f64 / 1024.0);
    println!("  Frames:              {}", docs.len());
    println!("  Blocks:              {}", blocks);
    println!("  Compression:         {:.2}x", total_text_bytes as f64 / file_size as f64);
    println!("  Bytes per doc:       {}", file_size / docs.len() as u64);
    println!("  Backend:             mmap (zero-copy, OS-paged)");
    println!();
    println!("  Open (mmap):         {}ms", open_ms);
    println!("  Rebuild index:       {}ms", rebuild_ms);
    println!();
    println!("  RECALL (300 queries via recall on Block 256 .said):");
    println!("    Recall@10:  {}/{} = {:.4}", correct, n, correct as f64 / n as f64);
    println!("    Speed avg:  {:.2}ms/query", avg);
    println!("    Speed p50:  {:.2}ms", p50);
    println!("    Speed p99:  {:.2}ms", p99);
    println!("    Total:      {:.0}ms for {} queries", total_ms, n);

    println!("\n  SINGLE FRAME READ (mmap + block cache):");
    for (doc_id, us, len) in &read_times {
        println!("    {}: {}us ({} bytes)", doc_id, us, len);
    }

    println!("\n  20 INDIVIDUAL RECALL SAMPLES:");
    println!("  {:>8} {:>8} {:>7} {:>6} {}", "query", "got", "ms", "chars", "text (first 120 chars)");
    println!("  {}", "-".repeat(100));
    for (qid, got_id, ms, snippet, text_len, hit) in &sample_results {
        let status = if *hit { "OK" } else { "MISS" };
        let snippet_clean: String = snippet.replace('\n', " ");
        let trunc: String = snippet_clean.chars().take(90).collect();
        println!("  [{:>4}] {:>8} {:>6.1}ms {:>5}c  {}",
            status, got_id, ms, text_len, trunc);
    }

    // ========================================================================
    // SUMMARY
    // ========================================================================
    println!("\n======================================================================");
    println!("  SUMMARY");
    println!("======================================================================");
    println!("  Raw text:              {:.1}MB ({} real Wikipedia docs)", total_text_bytes as f64 / 1024.0 / 1024.0, docs.len());
    println!("  .said block 256:       {:.1}KB", file_size as f64 / 1024.0);
    println!("  Compression:           {:.2}x", total_text_bytes as f64 / file_size as f64);
    println!();
    println!("  Recall@10:             {}/{} = {:.4}", correct, n, correct as f64 / n as f64);
    println!("  Speed (pure Rust):     {:.2}ms avg, {:.2}ms p50", avg, p50);
    println!("  Single frame read:     {}us (mmap + block cache)", read_times.first().map(|r| r.1).unwrap_or(0));
    println!("  Backend:               mmap (zero-copy)");
    println!();
    if correct == n {
        println!("  {}/{} PERFECT RECALL ON REAL WIKIMQA", correct, n);
        println!("  {:.1}MB raw -> {:.1}KB .said file (Block 256 + Zstd Dict)", total_text_bytes as f64 / 1024.0 / 1024.0, file_size as f64 / 1024.0);
        println!("  mmap backend. Pure Rust. Zero Python. Zero cosine. Zero vector database.");
    } else {
        println!("  {}/{} recall — {} misses", correct, n, n - correct);
    }
    println!("======================================================================");

    let _ = std::fs::remove_file(said_path);
}
