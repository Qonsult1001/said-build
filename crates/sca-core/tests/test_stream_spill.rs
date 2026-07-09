//! Streaming-ingest spill (#4): `said init` must ingest at CONSTANT memory by
//! spilling the FrameStore's in-RAM `pending` buffer to disk once it crosses a
//! byte budget, instead of holding the whole corpus in RAM until save().
//!
//! This is the SPIMI / streaming-index pattern. The contract under test:
//!   1. With a budget set, `pending_bytes()` stays bounded (≈ budget) across a
//!      large ingest — it does NOT grow to the full corpus size.
//!   2. After build_index() + save(), every frame still round-trips via get(),
//!      including ones that were spilled to disk mid-ingest.

use sca_core::said_file::SaidFile;

#[test]
fn stream_spill_keeps_pending_bounded_and_roundtrips() {
    let budget: usize = 4 * 1024 * 1024; // 4 MB
    let dir = std::env::temp_dir().join(format!("said_spill_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("spill.said");
    let _ = std::fs::remove_file(&path);

    let mut b = SaidFile::create(&path);
    b.set_stream_spill_budget(budget);

    // ~16 MB of frames: 8000 frames × ~2 KB each, well past the 4 MB budget so
    // the spill path is exercised many times.
    let n = 8000usize;
    let body = "x".repeat(2000);
    let mut max_pending = 0usize;
    for i in 0..n {
        let doc_id = format!("d{}", i);
        let content = format!("frame {} :: {}", i, body);
        b.remember_with_salience(
            Some(&doc_id),
            &content,
            None,
            sca_core::frames::Pillar::Semantic,
            Vec::new(),
        );
        let pending = b.pending_bytes();
        if pending > max_pending {
            max_pending = pending;
        }
        // Invariant 1: pending never balloons to the full corpus. Allow up to
        // 2× budget headroom for the single in-flight frame plus the batch that
        // tips over the threshold before the spill fires.
        assert!(
            pending <= budget * 2,
            "pending_bytes={} exceeded 2×budget={} at frame {}",
            pending,
            budget * 2,
            i
        );
    }

    // Spill must actually have happened — if max pending == total corpus the
    // budget did nothing.
    assert!(
        max_pending < (n * 2000) / 2,
        "pending peaked at {} — spill never engaged",
        max_pending
    );

    b.build_index().expect("build_index");
    b.save().expect("save");

    // Invariant 2: spilled + in-RAM frames all round-trip after save.
    for i in [0usize, 1, 100, 4000, 7999] {
        let doc_id = format!("d{}", i);
        let got = b.get(&doc_id).unwrap_or_else(|| panic!("frame {} missing after save", doc_id));
        assert!(
            got.contains(&format!("frame {} ::", i)),
            "frame {} content wrong after round-trip: {:?}",
            doc_id,
            &got[..got.len().min(40)]
        );
    }

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&dir);
}
