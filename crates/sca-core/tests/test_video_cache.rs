//! Test: model caching (second video is faster) + skip already-indexed.
//! Run: cargo test -p sca-core --release --features "static-embed,whisper" --test test_video_cache -- --nocapture

#[cfg(all(feature = "static-embed", feature = "whisper"))]
#[test]
fn test_model_cache_and_skip() {
    use sca_core::said_file::SaidFile;
    use sca_core::whisper_ingest;
    use std::time::Instant;

    let videos = [
        "E:/Store Secure/Videos/Access a wide range of service providers.mp4",
        "E:/Store Secure/Videos/Global coverage.mp4",
    ];

    // Find two available videos
    let available: Vec<&str> = videos.iter().filter(|p| std::path::Path::new(p).exists()).copied().collect();
    if available.len() < 2 {
        println!("SKIP: need at least 2 test videos");
        return;
    }

    let said_path = "test_video_cache.said";
    let _ = std::fs::remove_file(said_path);

    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
    ];

    let mut brain = SaidFile::create(said_path);
    for ep in &encoder_paths { if brain.load_encoder(ep).is_ok() { break; } }

    println!("\n======================================================================");
    println!("MODEL CACHE + SKIP TEST");
    println!("======================================================================");

    // Video 1: cold start (model loads from disk)
    let t0 = Instant::now();
    let r1 = whisper_ingest::ingest_video(&mut brain, available[0]).expect("ingest 1");
    let time1 = t0.elapsed().as_millis();
    println!("\n  Video 1 (COLD — model loads from disk):");
    println!("    File:     {}", available[0]);
    println!("    Time:     {}ms", time1);
    println!("    Duration: {:.1}s", r1.duration_secs);
    println!("    Segments: {}", r1.segments_transcribed);
    println!("    Skipped:  {}", r1.skipped);

    // Video 2: warm start (model already in memory)
    let t0 = Instant::now();
    let r2 = whisper_ingest::ingest_video(&mut brain, available[1]).expect("ingest 2");
    let time2 = t0.elapsed().as_millis();
    println!("\n  Video 2 (WARM — model cached in memory):");
    println!("    File:     {}", available[1]);
    println!("    Time:     {}ms", time2);
    println!("    Duration: {:.1}s", r2.duration_secs);
    println!("    Segments: {}", r2.segments_transcribed);
    println!("    Skipped:  {}", r2.skipped);

    // Video 1 again: should be SKIPPED (already indexed)
    let t0 = Instant::now();
    let r3 = whisper_ingest::ingest_video(&mut brain, available[0]).expect("ingest 3");
    let time3 = t0.elapsed().as_millis();
    println!("\n  Video 1 AGAIN (should SKIP — already indexed):");
    println!("    File:     {}", available[0]);
    println!("    Time:     {}ms", time3);
    println!("    Skipped:  {}", r3.skipped);
    assert!(r3.skipped, "Should have skipped already-indexed file!");
    assert!(time3 < 100, "Skip should be instant, was {}ms", time3);

    // Summary
    let speedup = if time2 > 0 { time1 as f64 / time2 as f64 } else { 0.0 };
    println!("\n======================================================================");
    println!("  RESULTS");
    println!("======================================================================");
    println!("  Cold start (model load + transcribe): {}ms", time1);
    println!("  Warm start (model cached):            {}ms", time2);
    println!("  Skip (already indexed):               {}ms", time3);
    println!("  Warm vs Cold speedup:                 {:.1}x", speedup);
    println!("  Total frames in .said:                {}", r1.frames_stored + r2.frames_stored);
    println!("======================================================================");

    let _ = std::fs::remove_file(said_path);
}
