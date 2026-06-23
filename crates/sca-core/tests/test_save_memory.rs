//! Phase-1 loop for the save() OOM (#4): save() accumulates the WHOLE file in one
//! Vec<u8> then keeps a second copy (self.data = Owned(buf)). On large brains this
//! single contiguous allocation OOMs. This test builds a brain big enough that the save
//! buffer is large, and asserts peak transient memory stays bounded relative to file size
//! (i.e. we do NOT hold ~2-3× the file in RAM at save time).
//!
//! It's a proxy: we can't easily measure RSS portably, so we (a) verify save works at a
//! size that approximates the failure, and (b) after the streaming fix, assert self.data
//! is NOT a full redundant copy when the file is large (the fix drops the Owned(buf)
//! double-hold for big files, reloading lazily instead).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_save_memory -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn save_does_not_double_hold_large_file() {
    let path = "test_save_mem.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());

    // Build a brain with many frames, each carrying a few KB of body, so the on-disk
    // file is multiple MB — enough that holding 2-3 copies would be visible. (Kept modest
    // so the test is fast; the OOM mechanism is the same at any scale — one big Vec<u8>.)
    let body = "x".repeat(2000); // ~2KB per frame
    let n = 3000;                 // ~6 MB of payload
    for i in 0..n {
        b.remember_with_salience(Some(&format!("d{i}")), &format!("{body} frame {i}"),
            None, Pillar::Episodic, vec![]);
    }
    b.build_index().expect("idx");
    b.save().expect("save");

    let file_size = std::fs::metadata(path).unwrap().len() as usize;
    eprintln!("file size after save: {} bytes ({} frames)", file_size, n);

    // The fix's invariant: after saving a LARGE file, the in-memory data handle must not
    // be a full redundant Owned copy of the just-written bytes. We expose a cheap probe:
    // the brain reopens correctly and round-trips a frame WITHOUT having kept the whole
    // file buffered. (Pre-fix: self.data = Owned(buf) holds the entire file.)
    let mem = b.in_memory_data_len();
    eprintln!("in-memory data len held after save: {} bytes", mem);

    // round-trip correctness (must always hold)
    let got = b.get("d1500").expect("frame d1500 present after save");
    assert!(got.contains("frame 1500"));

    // After the streaming fix, large saves should NOT retain the whole file in RAM.
    // Threshold: in-memory held bytes should be well under the file size for a large file.
    assert!(mem * 2 <= file_size || file_size < 1_000_000,
        "save() should not double-hold a large file in RAM (held {} of {} file bytes)", mem, file_size);

    let _ = std::fs::remove_file(path);
}
