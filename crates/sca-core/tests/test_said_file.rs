//! End-to-end test for the unified .said brain file.

use sca_core::said_file::SaidFile;
use std::path::Path;

#[test]
fn test_create_put_save_open_read() {
    let path = "test_output.said";

    // Create + put docs
    {
        let mut sf = SaidFile::create(path);

        sf.put("doc_1", "Albert Einstein was a theoretical physicist born in Germany.", Some("Einstein"));
        sf.put("doc_2", "Marie Curie conducted pioneering research on radioactivity.", Some("Curie"));
        sf.put("doc_3", "Isaac Newton formulated the laws of motion and universal gravitation.", Some("Newton"));

        assert_eq!(sf.frames.active_count(), 3);
        assert!(sf.is_dirty());

        sf.save().expect("save failed");
        assert!(!sf.is_dirty());

        let stats = sf.stats();
        assert_eq!(stats.active_frames, 3);
        assert!(stats.compressed_bytes > 0);
        // Short texts may not compress well — just verify it stored something
        println!("Created: {} bytes, {} frames, {:.1}x compression",
            stats.file_size, stats.active_frames, stats.compression_ratio);
    }

    // Open + read
    {
        let sf = SaidFile::open(path).expect("open failed");

        assert_eq!(sf.frames.active_count(), 3);

        // Random access read — single frame
        let text = sf.read("doc_1").expect("read doc_1 failed");
        assert!(text.contains("Einstein"));
        assert!(text.contains("physicist"));

        let text2 = sf.read("doc_2").expect("read doc_2 failed");
        assert!(text2.contains("Curie"));
        assert!(text2.contains("radioactivity"));

        let text3 = sf.read("doc_3").expect("read doc_3 failed");
        assert!(text3.contains("Newton"));

        // Non-existent doc
        assert!(sf.read("doc_999").is_none());

        println!("Read back all 3 docs successfully");

        let stats = sf.stats();
        println!("Opened: {} bytes, {} frames", stats.file_size, stats.active_frames);
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_delete_and_reopen() {
    let path = "test_delete.said";

    // Create with 3 docs, delete 1
    {
        let mut sf = SaidFile::create(path);
        sf.put("doc_a", "First document content", None);
        sf.put("doc_b", "Second document to be deleted", None);
        sf.put("doc_c", "Third document content", None);
        sf.save().expect("save failed");

        assert_eq!(sf.frames.active_count(), 3);

        // Delete doc_b
        assert!(sf.delete("doc_b"));
        assert_eq!(sf.frames.active_count(), 2);

        // Read deleted doc returns None
        assert!(sf.read("doc_b").is_none());

        // Other docs still readable
        assert!(sf.read("doc_a").is_some());
        assert!(sf.read("doc_c").is_some());

        sf.save().expect("save after delete failed");
    }

    // Reopen — deleted doc should still be gone
    {
        let sf = SaidFile::open(path).expect("reopen failed");
        assert_eq!(sf.frames.active_count(), 2);
        assert!(sf.read("doc_b").is_none());
        assert!(sf.read("doc_a").is_some());
        println!("Delete persisted across save/load");
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_blake3_integrity() {
    let path = "test_integrity.said";

    {
        let mut sf = SaidFile::create(path);
        sf.put("doc_1", "This content has BLAKE3 integrity protection.", None);
        sf.save().expect("save failed");
    }

    // Read raw file bytes, corrupt one byte in the frame data section
    {
        let mut data = std::fs::read(path).expect("read file");
        // Corrupt a byte in the middle of the file (frame data region)
        let mid = data.len() / 2;
        data[mid] ^= 0xFF;
        std::fs::write(path, &data).expect("write corrupted");
    }

    // Open should fail CRC32 check
    let result = SaidFile::open(path);
    assert!(result.is_err(), "Should fail on corrupted file");
    println!("CRC32 corruption detected correctly");

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_wal_crash_recovery() {
    let path = "test_wal.said";
    let tmp_path = "test_wal.said.tmp";

    // Create valid file
    {
        let mut sf = SaidFile::create(path);
        sf.put("doc_1", "Valid content", None);
        sf.save().expect("save failed");
    }

    // Simulate crash: create a .tmp file (incomplete write)
    std::fs::write(tmp_path, b"garbage incomplete data").expect("write tmp");

    // Open should detect .tmp, warn, use last good .said
    {
        let sf = SaidFile::open(path).expect("should recover from WAL");
        assert_eq!(sf.frames.active_count(), 1);
        assert!(sf.read("doc_1").is_some());
        println!("WAL recovery: used last good file, cleaned up .tmp");
    }

    // .tmp should be cleaned up
    assert!(!Path::new(tmp_path).exists(), ".tmp should be removed");

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_brain_persists() {
    let path = "test_brain.said";

    // Create, search (builds brain state), save
    {
        let mut sf = SaidFile::create(path);
        sf.put("doc_1", "Quantum computing uses qubits for parallel computation.", None);
        sf.put("doc_2", "Classical computers use binary bits for sequential processing.", None);
        sf.put("doc_3", "Machine learning models learn patterns from training data.", None);
        sf.save().expect("save failed");

        // Brain should be empty
        let stats = sf.stats();
        assert_eq!(stats.brain_queries, 0);
        assert_eq!(stats.brain_boosted, 0);

        // Simulate some queries hitting the brain
        sf.engine.brain.log_query("quantum computing", "doc_1", 0.95);
        sf.engine.brain.log_query("binary processing", "doc_2", 0.90);
        sf.engine.brain.log_query("quantum qubits", "doc_1", 0.92);

        let stats = sf.stats();
        assert_eq!(stats.brain_queries, 3);
        assert!(stats.brain_boosted > 0);

        sf.save().expect("save with brain");
    }

    // Reopen — brain should persist
    {
        let sf = SaidFile::open(path).expect("reopen");
        let stats = sf.stats();
        assert_eq!(stats.brain_queries, 3, "Brain query log should persist");
        assert!(stats.brain_boosted > 0, "Brain boosted docs should persist");

        // doc_1 should have higher recall weight (recalled twice)
        let w1 = sf.engine.brain.get_recall_weight("doc_1");
        let w3 = sf.engine.brain.get_recall_weight("doc_3");
        assert!(w1 > w3, "doc_1 (recalled 2x) should have higher weight than doc_3 (never recalled)");

        println!("Brain persisted: {} queries, {} boosted, doc_1 weight={:.4}",
            stats.brain_queries, stats.brain_boosted, w1);
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_lazy_compression() {
    let path = "test_lazy.said";
    let long_text = "The quick brown fox jumps over the lazy dog near the river bank. ".repeat(500);

    {
        let mut sf = SaidFile::create(path);

        // Put stores PLAIN initially
        sf.put("doc_1", &long_text, None);
        sf.put("doc_2", &long_text, None);
        sf.put("doc_3", "short text", None); // too short to compress

        assert_eq!(sf.uncompressed_count(), 2); // 2 large, 1 too small
        sf.save().expect("save plain");

        let size_before = sf.stats().file_size;
        println!("Before compact: {} bytes, {} uncompressed frames", size_before, sf.uncompressed_count());

        // Compact — compress in background
        let (compressed, saved) = sf.compact();
        println!("Compacted: {} frames, saved {} bytes", compressed, saved);
        assert_eq!(compressed, 2);
        assert!(saved > 0);
        assert_eq!(sf.uncompressed_count(), 0);

        sf.save().expect("save compressed");
        let size_after = sf.stats().file_size;
        println!("After compact: {} bytes (saved {})", size_after, size_before - size_after);
        assert!(size_after < size_before, "File should be smaller after compaction");
    }

    // Verify content survives compact
    {
        let sf = SaidFile::open(path).expect("reopen");
        let text = sf.read("doc_1").expect("read after compact");
        assert!(text.contains("quick brown fox"));
        let short = sf.read("doc_3").expect("read short");
        assert_eq!(short, "short text");
        println!("Content verified after lazy compression");
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_compression_stats() {
    let path = "test_compression.said";

    let long_text = "The quick brown fox jumps over the lazy dog. ".repeat(1000);

    {
        let mut sf = SaidFile::create(path);
        sf.put("doc_long", &long_text, Some("Long document"));
        sf.save().expect("save failed");

        let stats = sf.stats();
        println!("Compression: {}B compressed / {}B original = {:.1}x ratio",
            stats.compressed_bytes, stats.uncompressed_bytes, stats.compression_ratio);

        // Before compact: frames are plain (lazy compression)
        // Compact then check
        sf.compact();
        sf.save().expect("save after compact");
        let stats = sf.stats();
        println!("After compact: {}B compressed / {}B original = {:.1}x ratio",
            stats.compressed_bytes, stats.uncompressed_bytes, stats.compression_ratio);
        assert!(stats.compression_ratio > 2.0, "Should compress repetitive text well after compact");
    }

    let _ = std::fs::remove_file(path);
}
