//! Smoke-test for SaidFile::put_binary — the binary asset path used by said-vault.

use sca_core::said_file::SaidFile;

#[test]
fn put_binary_preserves_non_utf8_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("brain.said");
    let mut brain = SaidFile::create(path.to_str().unwrap());
    let png_header: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0xFF, 0xFE, 0xFD];
    let _id = brain.put_binary("doc-1", &png_header, sca_core::frames::Pillar::Document, vec![]);
    // Verify the frame is registered after put_binary
    assert!(brain.frames.get_meta("doc-1").is_some(), "doc-1 must be registered after put_binary");
    // Verify bytes roundtrip via read_binary (not the lossy UTF-8 text path)
    let got = brain.read_binary("doc-1").expect("read_binary must return Some");
    assert_eq!(got, png_header, "non-UTF-8 bytes must roundtrip exactly via read_binary");
}
