//! Regression: a deleted memory shown by admin list-tombstones MUST be restorable by
//! admin_restore(doc_id). Bug: list-tombstones shows the doc, but admin_restore errors
//! "no tombstone found for doc_id" — the recover-a-deleted-memory flow is broken.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_admin_restore -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

#[test]
fn deleted_memory_is_restorable() {
    let path = "test_admin_restore.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    brain.remember_with_salience(Some("wifi"), "Wifi is sunflower-42.", None, Pillar::Episodic, vec![]);
    brain.build_index().expect("build_index");
    brain.delete("wifi");
    // Persist + REOPEN before restoring — this matches the real CLI: `delete` runs in
    // one process (saves the deleted frame to the block-compressed file), then
    // `admin restore` runs in a fresh process that opens from disk. Restoring an
    // already-persisted Deleted frame is where the on-disk BLAKE3 desync showed up.
    brain.save().expect("save after delete");
    drop(brain);
    let mut brain = SaidFile::open(path).expect("reopen before restore");
    assert!(brain.auto_load_encoder());

    // list-tombstones must show it (the Recycle Bin view).
    let tombs: Vec<String> = brain.admin_tombstones().iter().map(|m| m.doc_id.clone()).collect();
    eprintln!("tombstones after delete: {:?}", tombs);
    assert!(tombs.iter().any(|d| d == "wifi"), "list-tombstones must show the deleted memory");

    // restore must succeed and bring it back to Active.
    let res = brain.admin_restore("wifi");
    eprintln!("admin_restore result: {:?}", res);
    assert!(res.is_ok(), "admin_restore must succeed for a doc list-tombstones shows; got {:?}", res);

    // In-memory read works.
    assert_eq!(brain.get("wifi").as_deref(), Some("Wifi is sunflower-42."),
        "restored memory must be readable in-memory");

    // CRITICAL: persist + reopen, like the real CLI does. The on-disk BLAKE3 checksum
    // must still verify — a restore that corrupts the file (BLAKE3 mismatch on reopen)
    // is the real-world failure the CLI hit.
    brain.save().expect("save");
    drop(brain);
    let mut reopened = SaidFile::open(path).expect("reopen");
    assert!(reopened.auto_load_encoder());
    let got = reopened.get("wifi");
    let _ = std::fs::remove_file(path);
    assert_eq!(got.as_deref(), Some("Wifi is sunflower-42."),
        "restored memory must survive save+reopen (no BLAKE3 mismatch); got {:?}", got);
}
