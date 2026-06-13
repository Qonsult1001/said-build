//! Manifest serde + invariants.

use said_vault::manifest::{Manifest, ZipEntry};

fn sample_manifest() -> Manifest {
    Manifest {
        doc_id: "blake3:abc123".into(),
        format: "docx".into(),
        filename: "Contract.docx".into(),
        size_bytes: 102400,
        ingested_at: "2026-05-26T14:30:00Z".into(),
        ingested_by: "admin@acme.com".into(),
        tags: vec!["dept:legal".into(), "classification:confidential".into()],
        zip_entries: vec![
            ZipEntry {
                name: "word/document.xml".into(),
                asset_hash: "blake3:def".into(),
                kind: "xml".into(),
            },
            ZipEntry {
                name: "word/styles.xml".into(),
                asset_hash: "blake3:cab".into(),
                kind: "xml".into(),
            },
        ],
        tombstone_hash: Some("blake3:fff".into()),
    }
}

#[test]
fn manifest_roundtrips_through_json() {
    let m = sample_manifest();
    let json = serde_json::to_string(&m).expect("serialize");
    let back: Manifest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.doc_id, m.doc_id);
    assert_eq!(back.zip_entries.len(), m.zip_entries.len());
    assert_eq!(back.tags, m.tags);
    assert_eq!(back.tombstone_hash, m.tombstone_hash);
}

#[test]
fn manifest_with_no_tombstone_hash_serializes_null() {
    let mut m = sample_manifest();
    m.tombstone_hash = None;
    let json = serde_json::to_string(&m).expect("serialize");
    assert!(json.contains("\"tombstone_hash\":null"), "None must serialize as JSON null, got {}", json);
}

#[test]
fn manifest_preserves_zip_entry_order() {
    let m = sample_manifest();
    let json = serde_json::to_string(&m).expect("serialize");
    let back: Manifest = serde_json::from_str(&json).expect("deserialize");
    // Document.xml MUST come before styles.xml in this manifest — order
    // is load-bearing for byte-faithful zip rebuild.
    assert_eq!(back.zip_entries[0].name, "word/document.xml");
    assert_eq!(back.zip_entries[1].name, "word/styles.xml");
}

#[test]
fn manifest_to_json_and_from_json_are_inverse() {
    let m = sample_manifest();
    let json = m.to_json();
    let back = Manifest::from_json(&json).expect("from_json");
    assert_eq!(back, m, "to_json + from_json must roundtrip with full equality");
}

#[test]
fn zip_entry_kind_distinguishes_paragraph_image_font_xml() {
    // Smoke test: we expect 4 string-valued kinds. The Manifest struct
    // doesn't enforce an enum here (json flexibility), but the convention
    // is "paragraph" | "image" | "font" | "xml" per spec.
    for kind in &["paragraph", "image", "font", "xml"] {
        let z = ZipEntry {
            name: "x".into(),
            asset_hash: "h".into(),
            kind: kind.to_string(),
        };
        let json = serde_json::to_string(&z).unwrap();
        assert!(json.contains(&format!("\"kind\":\"{}\"", kind)), "got {}", json);
    }
}
