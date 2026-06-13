//! SaidStore contract — ported from vault-rust/rust/src/store.rs tests,
//! adapted to said-vault's SaidStore (backed by sca_core::SaidFile).

use said_vault::store::SaidStore;

fn fresh_store() -> SaidStore {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("vault.said");
    let store = SaidStore::create(path.to_str().unwrap());
    // Leak the tempdir for the test's lifetime — the OS reclaims on
    // process exit. The point is to exercise the store API; persistence
    // tests live separately.
    std::mem::forget(tmp);
    store
}

#[test]
fn put_object_then_get_returns_bytes_text() {
    let mut s = fresh_store();
    s.put_object("blake3:h1", "paragraph", b"hello vault").expect("put");
    let got = s.get_object("blake3:h1").expect("get").expect("must be Some");
    assert_eq!(got, b"hello vault");
}

#[test]
fn put_object_then_get_returns_bytes_binary() {
    // Binary content (image-like raw bytes that may contain null/non-UTF-8)
    let mut s = fresh_store();
    let png_header = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0xFF, 0xFE];
    s.put_object("blake3:h2", "image", &png_header).expect("put");
    let got = s.get_object("blake3:h2").expect("get").expect("must be Some");
    assert_eq!(got, png_header, "binary bytes must roundtrip exactly (no base64 inflation)");
}

#[test]
fn put_object_is_idempotent() {
    let mut s = fresh_store();
    s.put_object("blake3:h1", "image", b"img").expect("put 1");
    s.put_object("blake3:h1", "image", b"img").expect("put 2 must not error");
    assert_eq!(s.object_count().expect("count"), 1);
}

#[test]
fn object_exists_reports_correctly() {
    let mut s = fresh_store();
    assert!(!s.object_exists("blake3:missing").expect("exists"));
    s.put_object("blake3:h1", "paragraph", b"data").expect("put");
    assert!(s.object_exists("blake3:h1").expect("exists"));
}

#[test]
fn get_unknown_returns_none() {
    let mut s = fresh_store();
    assert!(s.get_object("blake3:nope").expect("get").is_none());
}

#[test]
fn stats_count_objects_by_kind() {
    let mut s = fresh_store();
    s.put_object("blake3:p1", "paragraph", b"p").expect("put");
    s.put_object("blake3:p2", "paragraph", b"q").expect("put");
    s.put_object("blake3:i1", "image", b"i").expect("put");
    let st = s.stats().expect("stats");
    assert_eq!(st.total_objects, 3);
    assert_eq!(st.objects_by_kind.get("paragraph").copied().unwrap_or(0), 2);
    assert_eq!(st.objects_by_kind.get("image").copied().unwrap_or(0), 1);
}
