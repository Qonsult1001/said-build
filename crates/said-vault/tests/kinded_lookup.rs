//! Kinded lookup test: identical hash across two kinds returns the right asset.

use said_vault::store::SaidStore;

#[test]
fn get_object_kinded_returns_kind_specific_asset() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = tmp.path().join("v.said");
    let mut store = SaidStore::create(p.to_str().unwrap());

    // Same hash, two different kinds, two different payloads.
    let same_hash = "ffeeddccbbaa9988776655443322110000112233445566778899aabbccddeeff";
    store.put_object(same_hash, "paragraph", b"the paragraph").unwrap();
    store.put_object(same_hash, "xml",       b"the xml asset").unwrap();

    let para = store.get_object_kinded("paragraph", same_hash).unwrap().expect("para");
    let xml  = store.get_object_kinded("xml",       same_hash).unwrap().expect("xml");

    assert_eq!(para, b"the paragraph");
    assert_eq!(xml,  b"the xml asset");
}

#[test]
fn get_object_kinded_returns_none_for_missing_kind() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let p = tmp.path().join("v.said");
    let mut store = SaidStore::create(p.to_str().unwrap());
    let hash = "aaaa1111bbbb2222cccc3333dddd4444eeee5555ffff66667777888899990000";
    store.put_object(hash, "paragraph", b"only para").unwrap();

    // Asking for "xml" with the same hash must return None — not silently
    // route to the paragraph asset.
    assert!(store.get_object_kinded("xml", hash).unwrap().is_none());
}
