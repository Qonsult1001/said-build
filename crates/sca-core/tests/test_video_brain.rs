//! Video Brain Plugin tests — ingest, cross-search, and edge recall.

use sca_core::said_file::SaidFile;

// =========================================================================
// TEST 1: Ingest a single video and recall from its transcript
// =========================================================================

#[test]
#[cfg(all(feature = "static-embed", feature = "whisper"))]
fn test_video_ingest_and_recall() {
    let video_candidates = [
        "E:/Store Secure/Videos/How to Get a Google Maps API Key Simple Easy-s6a87os7iq.mp4",
        "E:/Store Secure/Videos/Coverage Maps.mp4",
        "E:/Store Secure/Videos/Global coverage.mp4",
    ];

    let video_path = match video_candidates.iter().find(|p| std::path::Path::new(p).exists()) {
        Some(p) => *p,
        None => {
            println!("SKIP: no test videos found");
            return;
        }
    };
    println!("Using video: {}", video_path);

    let said_path = "test_video_ingest.said";
    let mut brain = SaidFile::create(said_path);

    // Load encoder
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
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
        println!("SKIP: static encoder not found");
        let _ = std::fs::remove_file(said_path);
        return;
    }

    // Ingest video
    let t0 = std::time::Instant::now();
    let report = sca_core::whisper_ingest::ingest_video(&mut brain, video_path)
        .expect("ingest_video failed");
    let ingest_ms = t0.elapsed().as_millis();

    brain.build_index().expect("build_index failed");
    brain.compact();
    brain.save().expect("save failed");

    let file_size = std::fs::metadata(said_path).map(|m| m.len()).unwrap_or(0);
    println!("--- Ingest Report ---");
    println!("  Time:       {} ms", ingest_ms);
    println!("  Duration:   {:.1} s", report.duration_secs);
    println!("  Segments:   {}", report.segments_transcribed);
    println!("  Frames:     {}", report.frames_stored);
    println!("  .said size: {} bytes", file_size);

    // Reopen the .said file
    let mut brain2 = SaidFile::open(said_path).expect("reopen failed");
    for ep in &encoder_paths {
        if brain2.load_encoder(ep).is_ok() { break; }
    }
    brain2.build_index().expect("rebuild index failed");

    // Print first 5 frames
    println!("--- First 5 frames ---");
    let stem = std::path::Path::new(video_path).file_stem()
        .and_then(|s| s.to_str()).unwrap_or("video");
    for i in 0..5 {
        let doc_id = format!("{}_seg_{:04}", stem, i);
        if let Some(text) = brain2.read(&doc_id) {
            let preview: String = text.chars().take(80).collect();
            println!("  [{}] {}", doc_id, preview);
        }
    }

    // Recall queries
    let queries = ["coverage", "map", "service", "global", "provider"];
    println!("--- Recall ---");
    for q in &queries {
        let t1 = std::time::Instant::now();
        let results = brain2.recall(q, 3);
        let recall_ms = t1.elapsed().as_millis();
        if let Some(r) = results.first() {
            let preview = &r.content[..r.content.len().min(80)];
            println!("  '{}' -> doc_id={}, {}ms, {}", q, r.doc_id, recall_ms, preview);
        } else {
            println!("  '{}' -> no results", q);
        }
    }

    let _ = std::fs::remove_file(said_path);
    println!("TEST 1 PASS: video ingest and recall");
}

// =========================================================================
// TEST 2: Multi-video cross-search
// =========================================================================

#[test]
#[cfg(all(feature = "static-embed", feature = "whisper"))]
fn test_multi_video_cross_search() {
    let video_dir = std::path::Path::new("E:/Store Secure/Videos");
    if !video_dir.exists() {
        println!("SKIP: video directory not found");
        return;
    }

    let said_path = "test_multi_video.said";
    let mut brain = SaidFile::create(said_path);

    // Load encoder
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
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
        println!("SKIP: static encoder not found");
        let _ = std::fs::remove_file(said_path);
        return;
    }

    // Ingest all .mp4 files
    let mut total_videos = 0;
    if let Ok(entries) = std::fs::read_dir(video_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "mp4").unwrap_or(false) {
                let path_str = path.to_string_lossy().to_string();
                match sca_core::whisper_ingest::ingest_video(&mut brain, &path_str) {
                    Ok(report) => {
                        println!("  Ingested: {} — {} segments, {:.1}s",
                            path.file_name().unwrap().to_string_lossy(),
                            report.segments_transcribed,
                            report.duration_secs);
                        total_videos += 1;
                    }
                    Err(e) => {
                        println!("  WARN: {} — {}", path.file_name().unwrap().to_string_lossy(), e);
                    }
                }
            }
        }
    }

    if total_videos == 0 {
        println!("SKIP: no .mp4 files ingested");
        let _ = std::fs::remove_file(said_path);
        return;
    }

    brain.build_index().expect("build_index failed");
    brain.compact();
    brain.save().expect("save failed");

    println!("Ingested {} videos, {} frames", total_videos, brain.stats().active_frames);

    // Reopen
    let mut brain2 = SaidFile::open(said_path).expect("reopen failed");
    for ep in &encoder_paths {
        if brain2.load_encoder(ep).is_ok() { break; }
    }
    brain2.build_index().expect("rebuild index failed");

    // Cross-search
    let results = brain2.recall("coverage map", 5);
    println!("--- Cross-search: 'coverage map' ---");
    for r in &results {
        let preview = &r.content[..r.content.len().min(80)];
        println!("  doc_id={}, score={:.4}, {}", r.doc_id, r.score, preview);
    }
    assert!(!results.is_empty(), "cross-search returned no results");

    let _ = std::fs::remove_file(said_path);
    println!("TEST 2 PASS: multi-video cross-search");
}

// =========================================================================
// TEST 3: Edge recall without Whisper — proves edge devices work
// =========================================================================

#[test]
#[cfg(feature = "static-embed")]
fn test_edge_recall_without_whisper() {
    use sca_core::frames::{PutOptions, MemoryType, MemoryKind, MemorySubject, MemoryScope};

    let said_path = "test_edge_video_recall.said";
    let mut brain = SaidFile::create(said_path);

    // Load encoder
    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
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
        println!("SKIP: static encoder not found");
        let _ = std::fs::remove_file(said_path);
        return;
    }

    // Manually store 2 frames simulating video transcript segments
    let opts1 = PutOptions::new(
        "meeting_seg_0001",
        "The quarterly revenue target is four point seven million dollars",
    )
    .with_title("meeting.mp4 [00:30-00:40]")
    .with_type(MemoryType::Episodic)
    .with_kind(MemoryKind::Event)
    .with_subject(MemorySubject::World)
    .with_scope(MemoryScope::Personal)
    .with_tags(vec![
        "source:file:///meetings/q3.mp4".into(),
        "ts_start:30.0".into(),
        "ts_end:40.0".into(),
        "media:video".into(),
    ]);
    brain.put_with(&opts1);

    let opts2 = PutOptions::new(
        "meeting_seg_0002",
        "We need fifteen new enterprise contracts to hit the target",
    )
    .with_title("meeting.mp4 [00:40-00:50]")
    .with_type(MemoryType::Episodic)
    .with_kind(MemoryKind::Event)
    .with_subject(MemorySubject::World)
    .with_scope(MemoryScope::Personal)
    .with_tags(vec![
        "source:file:///meetings/q3.mp4".into(),
        "ts_start:40.0".into(),
        "ts_end:50.0".into(),
        "media:video".into(),
    ]);
    brain.put_with(&opts2);

    brain.build_index().expect("build_index failed");
    brain.compact();
    brain.save().expect("save failed");

    println!("Stored 2 video transcript frames, .said size: {} bytes",
        std::fs::metadata(said_path).map(|m| m.len()).unwrap_or(0));

    // Reopen — this is the "edge device" path (no Whisper needed)
    let mut brain2 = SaidFile::open(said_path).expect("reopen failed");
    for ep in &encoder_paths {
        if brain2.load_encoder(ep).is_ok() { break; }
    }
    brain2.build_index().expect("rebuild index failed");

    // Recall "revenue target"
    let results = brain2.recall("revenue target", 3);
    assert!(!results.is_empty(), "recall returned no results for 'revenue target'");
    let top = &results[0];
    assert!(top.doc_id.contains("meeting_seg"),
        "expected doc_id containing 'meeting_seg', got: {}", top.doc_id);
    println!("  Recall 'revenue target' -> doc_id={}, score={:.4}, content={}",
        top.doc_id, top.score, &top.content[..top.content.len().min(80)]);

    // Check frame metadata
    let meta = brain2.frames.get_meta("meeting_seg_0001")
        .expect("get_meta returned None for meeting_seg_0001");
    let has_ts_tag = meta.tags.iter().any(|t| t.starts_with("ts_start:"));
    assert!(has_ts_tag, "expected tags to contain 'ts_start:', got: {:?}", meta.tags);
    println!("  Metadata tags: {:?}", meta.tags);

    let _ = std::fs::remove_file(said_path);
    println!("TEST 3 PASS: edge recall without Whisper — video .said files work on edge devices");
}
