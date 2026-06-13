//! Prove: single-frame read from Block 256 .said — pure Rust, no Python.
//! Run: cargo test -p sca-core --release --features "static-embed" --test test_single_frame_read -- --nocapture

#[cfg(feature = "static-embed")]
#[test]
fn test_read_single_frame() {
    use sca_core::said_file::SaidFile;
    use std::time::Instant;

    let path = "test_single_frame.said";
    let _ = std::fs::remove_file(path);

    let encoder_paths = [
        "../../SAID-LAM-private/said-lam-static",
        "../SAID-LAM-private/said-lam-static",
    ];

    let mut brain = SaidFile::create(path);
    for ep in &encoder_paths {
        if brain.load_encoder(ep).is_ok() { break; }
    }

    // Store 3 docs
    brain.remember_as("meeting_notes", "Q3 revenue target is $4.7 million. Sales team needs 15 new enterprise contracts.", Some("Meeting"));
    brain.remember_as("personal", "My name is Carter, I live in Johannesburg and I love building AI systems.", Some("Personal"));
    brain.remember_as("code_ref", "The SaidFile struct in said_file.rs handles all .said file operations including remember, recall, and save.", Some("Code"));

    brain.build_index().expect("index");
    brain.compact();
    brain.save().expect("save");

    // Reopen from disk — proves persistence
    let mut brain2 = SaidFile::open(path).expect("open");

    // Read single frame — NO search, NO grep, just decompress one frame
    let t0 = Instant::now();
    let text = brain2.read("personal").expect("read personal");
    let read_us = t0.elapsed().as_micros();

    println!("Single frame read: {}us", read_us);
    println!("Content: {}", &text[..50]);
    assert!(text.contains("Carter"));
    assert!(text.contains("Johannesburg"));

    // Read another
    let t0 = Instant::now();
    let text2 = brain2.read("meeting_notes").expect("read meeting");
    let read_us2 = t0.elapsed().as_micros();
    println!("Second read (cached block): {}us", read_us2);
    assert!(text2.contains("$4.7 million"));

    // Read third
    let t0 = Instant::now();
    let text3 = brain2.read("code_ref").expect("read code");
    let read_us3 = t0.elapsed().as_micros();
    println!("Third read (cached block): {}us", read_us3);
    assert!(text3.contains("SaidFile"));

    // Now do a recall — proves search + fetch in pure Rust
    for ep in &encoder_paths {
        if brain2.load_encoder(ep).is_ok() { break; }
    }
    brain2.build_index().expect("rebuild");

    let t0 = Instant::now();
    let results = brain2.recall("what is the revenue target", 3);
    let recall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("\nRecall 'revenue target': {:.2}ms", recall_ms);
    println!("  Top result: {} (score {:.2})", results[0].doc_id, results[0].score);
    assert_eq!(results[0].doc_id, "meeting_notes");

    let t0 = Instant::now();
    let results2 = brain2.recall("where does Carter live", 3);
    let recall_ms2 = t0.elapsed().as_secs_f64() * 1000.0;
    println!("Recall 'Carter live': {:.2}ms", recall_ms2);
    println!("  Top result: {} (score {:.2})", results2[0].doc_id, results2[0].score);
    assert_eq!(results2[0].doc_id, "personal");

    println!("\nPURE RUST. Zero Python. Single frame read + recall from .said file.");

    let _ = std::fs::remove_file(path);
}
