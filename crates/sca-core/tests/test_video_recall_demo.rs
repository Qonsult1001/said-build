//! Demo: recall exact quotes and semantic questions from a video .said file.
//! Run: cargo test -p sca-core --release --features "static-embed,whisper" --test test_video_recall_demo -- --nocapture

#[cfg(all(feature = "static-embed", feature = "whisper"))]
#[test]
fn test_video_recall_exact_and_semantic() {
    use sca_core::said_file::SaidFile;
    use sca_core::whisper_ingest;
    use std::time::Instant;

    let video_path = "E:/Store Secure/Videos/How to Get a Google Maps API Key Simple Easy-s6a87os7iq.mp4";
    if !std::path::Path::new(video_path).exists() {
        println!("SKIP: video not found");
        return;
    }

    let said_path = "test_video_recall_demo.said";
    let _ = std::fs::remove_file(said_path);

    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
    ];

    // Ingest video
    let mut brain = SaidFile::create(said_path);
    for ep in &encoder_paths { if brain.load_encoder(ep).is_ok() { break; } }

    println!("\n[Ingesting video...]");
    let t0 = Instant::now();
    let report = whisper_ingest::ingest_video(&mut brain, video_path).expect("ingest");
    println!("[Ingested in {}s — {} segments]", t0.elapsed().as_secs(), report.segments_transcribed);

    brain.build_index().expect("index");
    brain.compact();
    brain.save().expect("save");

    // Reopen from disk (mmap) — simulates production
    let mut brain2 = SaidFile::open(said_path).expect("open");
    for ep in &encoder_paths { if brain2.load_encoder(ep).is_ok() { break; } }
    brain2.build_index().expect("rebuild");

    // Warm up
    let _ = brain2.recall("warmup", 1);

    println!("\n======================================================================");
    println!("VIDEO RECALL DEMO: Exact Quotes + Semantic Questions");
    println!("======================================================================");

    let queries = [
        // Exact quotes from the transcription
        ("The name is created. Awesome, there we go", "EXACT QUOTE"),
        ("keys and credentials", "EXACT PHRASE"),
        ("API key is just like a string of characters", "EXACT QUOTE"),
        // Semantic questions (not in the text verbatim)
        ("how do I add a key", "SEMANTIC"),
        ("what is an API key", "SEMANTIC"),
        ("how to restrict access", "SEMANTIC"),
        ("where to find my key after creating it", "SEMANTIC"),
        ("Google Maps setup tutorial", "SEMANTIC"),
        ("password for Google Maps", "SEMANTIC"),
        ("copy the API key", "SEMANTIC"),
    ];

    for (query, qtype) in &queries {
        let t0 = Instant::now();
        let results = brain2.recall(query, 1);
        let us = t0.elapsed().as_micros();
        let ms = us as f64 / 1000.0;

        if let Some(r) = results.first() {
            // Get the title (contains timestamp)
            let meta = brain2.frames.get_meta(&r.doc_id);
            let title = meta.and_then(|m| m.title.as_deref()).unwrap_or("?");
            let tags = meta.map(|m| &m.tags).cloned().unwrap_or_default();
            let ts_start = tags.iter().find(|t| t.starts_with("ts_start:"))
                .map(|t| &t[9..]).unwrap_or("?");

            let preview: String = r.content.chars().take(100).collect();
            println!("\n  [{:>8}] \"{}\"", qtype, query);
            println!("  {:>10} → {} | {} | {:.2}ms", "", r.doc_id, title, ms);
            println!("  {:>10}   \"{}...\"", "", preview);
            println!("  {:>10}   timestamp: {}s | score: {:.2}", "", ts_start, r.score);
        } else {
            println!("\n  [{:>8}] \"{}\" → NO RESULTS ({:.2}ms)", qtype, query, ms);
        }
    }

    let file_size = std::fs::metadata(said_path).map(|m| m.len()).unwrap_or(0);
    println!("\n======================================================================");
    println!("  .said file: {:.1}KB | Segments: {} | Video: {:.0}s",
        file_size as f64 / 1024.0, report.segments_transcribed, report.duration_secs);
    println!("  Pure Rust. mmap. Zero ffmpeg. Recall from video in microseconds.");
    println!("======================================================================");

    let _ = std::fs::remove_file(said_path);
}
