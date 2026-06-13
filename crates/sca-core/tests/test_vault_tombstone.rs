//! VaultTombstoneStore roundtrip: put -> get returns the exact bytes,
//! BLAKE3 verified. Sabotage check confirms the test would catch
//! a corruption regression.

use sca_core::said_file::SaidFile;
use sca_core::vault_tombstone::VaultTombstoneStore;

#[test]
fn put_and_get_roundtrip_returns_exact_bytes() {
    let mut store = VaultTombstoneStore::new();
    let original = b"hello vault tombstone";
    let hash = store.put("doc-1", original);
    assert_eq!(hash.len(), 32, "hash must be 32-byte BLAKE3");

    let restored = store.get("doc-1").expect("get must return Some after put");
    assert_eq!(restored, original, "tombstone bytes must roundtrip exactly");
}

#[test]
fn get_unknown_doc_id_returns_none() {
    let mut store = VaultTombstoneStore::new();
    assert!(store.get("nonexistent").is_none());
}

#[test]
fn count_and_total_bytes_track_inserts() {
    let mut store = VaultTombstoneStore::new();
    assert_eq!(store.count(), 0);
    assert_eq!(store.total_bytes(), 0);
    store.put("a", b"first content");
    store.put("b", b"second content but longer");
    assert_eq!(store.count(), 2);
    assert_eq!(store.total_bytes(), (b"first content".len() + b"second content but longer".len()) as u64);
}

#[test]
fn remove_returns_ok_for_existing_drops_doc() {
    let mut store = VaultTombstoneStore::new();
    store.put("doc-1", b"content");
    assert!(store.get("doc-1").is_some());
    store.remove("doc-1").expect("remove must succeed for existing doc");
    assert!(store.get("doc-1").is_none(), "doc must be gone after remove");
}

#[test]
fn remove_returns_err_for_unknown() {
    let mut store = VaultTombstoneStore::new();
    assert!(store.remove("nonexistent").is_err(), "remove of unknown id must error");
}

#[test]
fn iter_doc_ids_yields_all_present() {
    let mut store = VaultTombstoneStore::new();
    store.put("alpha", b"a");
    store.put("beta",  b"b");
    let mut ids: Vec<String> = store.iter_doc_ids().map(String::from).collect();
    ids.sort();
    assert_eq!(ids, vec!["alpha", "beta"]);
}

#[test]
fn put_overwrites_existing_doc_with_new_bytes_and_hash() {
    let mut store = VaultTombstoneStore::new();
    let h1 = store.put("doc-1", b"original content");
    assert_eq!(store.count(), 1);
    let original_total = store.total_bytes();

    // Overwrite with different bytes
    let h2 = store.put("doc-1", b"longer replacement content here");
    assert_eq!(store.count(), 1, "overwrite must not increase count");
    assert_ne!(h1, h2, "different bytes must produce different BLAKE3");
    assert!(store.total_bytes() > original_total,
        "total_bytes must reflect the longer replacement; old={}, new={}",
        original_total, store.total_bytes());

    let restored = store.get("doc-1").expect("get must succeed after overwrite");
    assert_eq!(restored, b"longer replacement content here",
        "stored bytes must be the LATEST put, not the original");
}

#[test]
fn said_file_lazy_allocates_vault_tombstones_only_when_accessed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("brain.said");
    let mut brain = SaidFile::create(path.to_str().unwrap());

    assert!(!brain.has_vault_tombstones(),
        "fresh SaidFile must NOT have a vault tombstone section (lazy alloc)");

    let _store = brain.vault_tombstones_mut();
    assert!(brain.has_vault_tombstones(),
        "vault_tombstones_mut() must lazily allocate the section");
}

#[test]
fn said_file_put_and_get_vault_tombstone_via_accessor() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("brain.said");
    let mut brain = SaidFile::create(path.to_str().unwrap());

    let bytes = b"document blob";
    let hash = brain.vault_tombstones_mut().put("doc-1", bytes);
    assert_eq!(hash, *blake3::hash(bytes).as_bytes());

    // Note: get is &self per Task 2 polish, so we can use the immutable accessor
    let restored = brain.vault_tombstones().expect("section allocated").get("doc-1")
        .expect("get must return Some after put");
    assert_eq!(restored, bytes);
}

#[test]
fn vault_tombstones_persist_across_save_and_open() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("brain.said");

    // Write tombstones, save, drop
    {
        let mut brain = SaidFile::create(path.to_str().unwrap());
        brain.vault_tombstones_mut().put("doc-alpha", b"the alpha bytes");
        brain.vault_tombstones_mut().put("doc-beta",  b"the beta bytes are longer");
        brain.save().expect("save must succeed");
    }

    // Reopen and verify
    {
        let mut brain = SaidFile::open(path.to_str().unwrap())
            .expect("reopen must succeed");
        assert!(brain.has_vault_tombstones(),
            "VAULT_TOMBSTONES section must be present after save+open");
        assert_eq!(brain.vault_tombstones_mut().count(), 2);
        let alpha = brain.vault_tombstones_mut().get("doc-alpha")
            .expect("alpha must be retrievable");
        assert_eq!(alpha, b"the alpha bytes");
        let beta = brain.vault_tombstones_mut().get("doc-beta")
            .expect("beta must be retrievable");
        assert_eq!(beta, b"the beta bytes are longer");
    }
}

#[test]
fn to_bytes_is_vts2_and_compresses_redundant_content() {
    // SPEC line 35: vault stored objects are zstd-compressed. The section
    // serializer must (a) use the VTS2 magic and (b) actually shrink highly
    // redundant content well below its raw size.
    let mut store = VaultTombstoneStore::new();
    // 100 KB of repetitive bytes per doc — trivially compressible, and stands
    // in for the cross-document redundancy (shared fonts/templates) real DOCX
    // tombstones carry.
    let blob = vec![b'A'; 100_000];
    store.put("doc-1", &blob);
    store.put("doc-2", &blob);

    let raw_total = blob.len() * 2; // 200 KB of content
    let bytes = store.to_bytes();
    assert_eq!(&bytes[..4], b"VTS2", "section must use the VTS2 (compressed) magic");
    assert!(bytes.len() < raw_total / 10,
        "compressed section ({} bytes) must be far smaller than raw content ({} bytes)",
        bytes.len(), raw_total);

    // Round-trips byte-exact through the compressed format.
    let restored = VaultTombstoneStore::from_bytes(&bytes).expect("from_bytes VTS2");
    assert_eq!(restored.count(), 2);
    assert_eq!(restored.get("doc-1").expect("doc-1"), blob);
    assert_eq!(restored.get("doc-2").expect("doc-2"), blob);
}

#[test]
fn from_bytes_still_reads_legacy_vts1_raw_format() {
    // Backward-compat: a hand-built VTS1 (raw, uncompressed) section must
    // still decode, so any pre-existing vault file keeps working.
    let original = b"legacy raw tombstone bytes";
    let blake3 = *blake3::hash(original).as_bytes();
    let doc_id = b"legacy-doc";

    let mut raw = Vec::new();
    raw.extend_from_slice(b"VTS1");                       // legacy magic
    raw.extend_from_slice(&1u32.to_le_bytes());           // count = 1
    raw.extend_from_slice(&blake3);                       // blake3
    raw.extend_from_slice(&(original.len() as u64).to_le_bytes()); // original_size
    raw.extend_from_slice(&(doc_id.len() as u32).to_le_bytes());   // doc_id len
    raw.extend_from_slice(doc_id);                        // doc_id
    raw.extend_from_slice(&(original.len() as u32).to_le_bytes()); // content len
    raw.extend_from_slice(original);                      // content

    let store = VaultTombstoneStore::from_bytes(&raw).expect("must read legacy VTS1");
    assert_eq!(store.count(), 1);
    assert_eq!(store.get("legacy-doc").expect("legacy-doc"), original);
}

#[test]
fn vault_tombstones_absent_in_personal_brain_files() {
    // A SaidFile that NEVER calls vault_tombstones_mut() must write no
    // VTS section. On reopen, has_vault_tombstones() must still return false.
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("personal.said");
    {
        let mut brain = SaidFile::create(path.to_str().unwrap());
        brain.save().expect("save personal brain must succeed");
    }
    {
        let brain = SaidFile::open(path.to_str().unwrap())
            .expect("reopen must succeed");
        assert!(!brain.has_vault_tombstones(),
            "personal brain file must NOT have VAULT_TOMBSTONES section");
    }
}

#[test]
fn vault_tombstones_mut_allocates_but_empty_writes_no_section() {
    // Contract: vault_tombstones_mut() eagerly allocates Some(empty store),
    // but save() must NOT emit the VTS section when count() == 0. On reopen,
    // has_vault_tombstones() must report false (same as a personal-brain
    // file that never touched the section).
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("allocated-but-empty.said");

    {
        let mut brain = SaidFile::create(path.to_str().unwrap());
        // Trigger the lazy allocation but DO NOT put any entries
        let _ = brain.vault_tombstones_mut();
        assert!(brain.has_vault_tombstones(),
            "after vault_tombstones_mut(), the section IS allocated in memory");
        brain.save().expect("save must succeed even with empty vault section");
    }

    {
        let brain = SaidFile::open(path.to_str().unwrap())
            .expect("reopen must succeed");
        assert!(!brain.has_vault_tombstones(),
            "save() must NOT write an empty VTS section; reopen must report false");
    }
}
