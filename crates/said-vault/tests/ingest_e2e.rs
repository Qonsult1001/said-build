//! End-to-end ingest tests: ingest a DOCX, verify manifest + assets +
//! tombstone all land in the vault file with the right pillar / tags.

use said_vault::SaidVault;
use std::io::Write;

fn build_minimal_docx(paragraph: &str) -> Vec<u8> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(buf);
    let opts = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let files = [
        ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
        ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
    ];
    for (name, content) in &files {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(content.as_bytes()).unwrap();
    }
    let doc_xml = format!(r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#, paragraph);
    zw.start_file("word/document.xml", opts).unwrap();
    zw.write_all(doc_xml.as_bytes()).unwrap();
    zw.finish().unwrap().into_inner()
}

#[test]
fn ingest_writes_manifest_and_assets_and_tombstone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let vault_path = tmp.path().join("vault.said");
    let doc_path = tmp.path().join("hello.docx");
    std::fs::write(&doc_path, build_minimal_docx("hello vault")).unwrap();

    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test")
        .expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &["dept:legal".into()], true, "admin@test")
        .expect("ingest");

    // Manifest must be present
    let manifest = vault.load_manifest(&doc_id).expect("manifest must exist");
    assert_eq!(manifest.filename, "hello.docx");
    assert_eq!(manifest.format, "docx");
    assert!(manifest.tags.contains(&"dept:legal".into()));
    assert!(manifest.tombstone_hash.is_some(),
        "tombstone hash must be in manifest after enterprise-tier ingest");
    assert_eq!(manifest.ingested_by, "admin@test",
        "manifest must record the authenticated ingester identity");

    // At least one paragraph asset must be stored
    let stats = vault.stats().expect("stats");
    assert!(stats.objects_by_kind.get("paragraph").copied().unwrap_or(0) >= 1,
        "expected at least one paragraph asset; got {:?}", stats.objects_by_kind);
}

#[test]
fn ingest_with_no_tombstone_records_none_in_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let vault_path = tmp.path().join("vault.said");
    let doc_path = tmp.path().join("hello.docx");
    std::fs::write(&doc_path, build_minimal_docx("slim mode")).unwrap();

    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &[], false, "admin@test")
        .expect("ingest slim");

    let manifest = vault.load_manifest(&doc_id).expect("manifest");
    assert!(manifest.tombstone_hash.is_none(),
        "slim mode must leave tombstone_hash None");
}

#[test]
fn re_ingest_same_file_is_idempotent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let vault_path = tmp.path().join("vault.said");
    let doc_path = tmp.path().join("hello.docx");
    std::fs::write(&doc_path, build_minimal_docx("idempotent test")).unwrap();

    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let id_a = vault.ingest(doc_path.to_str().unwrap(), &[], true, "admin@test").expect("ingest 1");
    let id_b = vault.ingest(doc_path.to_str().unwrap(), &[], true, "admin@test").expect("ingest 2 (should no-op)");
    assert_eq!(id_a, id_b, "same file content must produce same doc_id");

    let stats = vault.stats().expect("stats");
    assert_eq!(stats.objects_by_kind.get("paragraph").copied().unwrap_or(0), 1,
        "no second copy of the same paragraph");
}
