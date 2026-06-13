//! End-to-end rebuild / restore / compare against a real ingest.

use said_vault::SaidVault;
use std::io::Write;

fn build_minimal_docx(paragraph: &str) -> Vec<u8> {
    // Self-contained helper — duplicated from ingest_e2e.rs intentionally
    // so this test file reads in isolation.
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
fn rebuild_produces_a_docx_with_extracted_paragraph_text() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let doc_path = tmp.path().join("hello.docx");
    let original = build_minimal_docx("rebuild me");
    std::fs::write(&doc_path, &original).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &[], true, "admin@test").expect("ingest");

    let out_dir = tmp.path().join("export");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out_path = vault.rebuild(&doc_id, out_dir.to_str().unwrap()).expect("rebuild");

    assert!(std::path::Path::new(&out_path).exists());
    let rebuilt = std::fs::read(&out_path).unwrap();
    let parsed = said_vault::parser::docx::parse(&rebuilt).expect("parse rebuilt");
    let texts: Vec<&str> = parsed.paragraphs.iter().map(|p| p.text.as_str()).collect();
    assert!(texts.contains(&"rebuild me"), "rebuilt text must contain original paragraph; got {:?}", texts);
}

#[test]
fn restore_returns_byte_exact_original_when_tombstone_present() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let doc_path = tmp.path().join("hello.docx");
    let original = build_minimal_docx("restore me");
    std::fs::write(&doc_path, &original).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &[], true, "admin@test").expect("ingest");

    let out_dir = tmp.path().join("export");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out_path = vault.restore(&doc_id, out_dir.to_str().unwrap()).expect("restore");

    let restored = std::fs::read(&out_path).unwrap();
    assert_eq!(restored, original, "restored bytes MUST equal the original byte-for-byte");
}

/// Build a DOCX with many distinct paragraphs so ingest produces well over
/// the 10-frame threshold that triggers block compaction in compact().
fn build_multi_para_docx(paras: &[String]) -> Vec<u8> {
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
    let body: String = paras.iter()
        .map(|p| format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", p))
        .collect();
    let doc_xml = format!(r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}</w:body></w:document>"#, body);
    zw.start_file("word/document.xml", opts).unwrap();
    zw.write_all(doc_xml.as_bytes()).unwrap();
    zw.finish().unwrap().into_inner()
}

#[test]
fn compacted_vault_round_trips_rebuild_and_byte_exact_restore() {
    // Regression guard for compact-on-save: with >10 distinct frames the
    // vault's save path block-compacts the frame table. Rebuild (parts) and
    // restore (byte-exact tombstone) must both survive compaction.
    let tmp = tempfile::tempdir().expect("tempdir");
    let paras: Vec<String> = (0..40).map(|i| format!("paragraph number {} with distinct content", i)).collect();
    let doc_path = tmp.path().join("multi.docx");
    let original = build_multi_para_docx(&paras);
    std::fs::write(&doc_path, &original).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let ids = vault.ingest_batch(
        &[doc_path.to_string_lossy().to_string()],
        &[], true, "admin@test",
    ).expect("ingest_batch");
    let doc_id = &ids[0];

    let out_dir = tmp.path().join("export");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Rebuild must preserve every paragraph through the compacted frames.
    let rebuilt_path = vault.rebuild(doc_id, out_dir.to_str().unwrap()).expect("rebuild");
    let rebuilt = std::fs::read(&rebuilt_path).unwrap();
    let parsed = said_vault::parser::docx::parse(&rebuilt).expect("parse rebuilt");
    let texts: Vec<&str> = parsed.paragraphs.iter().map(|p| p.text.as_str()).collect();
    for p in &paras {
        assert!(texts.iter().any(|t| t == p),
            "compacted rebuild lost paragraph: {:?}", p);
    }

    // Restore must return the byte-exact original from the tombstone.
    let restore_dir = tmp.path().join("restore");
    std::fs::create_dir_all(&restore_dir).unwrap();
    let restored_path = vault.restore(doc_id, restore_dir.to_str().unwrap()).expect("restore");
    let restored = std::fs::read(&restored_path).unwrap();
    assert_eq!(restored, original,
        "compacted vault must still restore byte-exact original");
}

#[test]
fn compare_reports_text_match_true_byte_identical_false() {
    // compare restores the byte-exact original and rebuilds from parts, then
    // diffs their paragraph text. Per the spec, text must match but the two
    // byte streams must differ (rebuild re-zips → different layout).
    let tmp = tempfile::tempdir().expect("tempdir");
    let paras: Vec<String> = (0..15).map(|i| format!("compare paragraph {}", i)).collect();
    let doc_path = tmp.path().join("cmp.docx");
    std::fs::write(&doc_path, build_multi_para_docx(&paras)).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &[], true, "admin@test").expect("ingest");

    let report = vault.compare(&doc_id).expect("compare");
    assert!(report.text_match, "restore + rebuild must have identical paragraph text; diffs: {:?}", report.first_diffs);
    assert!(!report.byte_identical, "rebuild re-zips, so byte streams must differ by design");
    assert_eq!(report.restored_paragraphs, paras.len());
    assert_eq!(report.rebuilt_paragraphs, paras.len());
    assert!(report.restored_bytes > 0 && report.rebuilt_bytes > 0);
}

#[test]
fn compare_errors_on_slim_mode_no_tombstone() {
    // compare needs the tombstone (restore half). Slim ingest → clear error.
    let tmp = tempfile::tempdir().expect("tempdir");
    let doc_path = tmp.path().join("slimcmp.docx");
    std::fs::write(&doc_path, build_minimal_docx("slim compare")).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &[], false, "admin@test").expect("ingest slim");

    let err = vault.compare(&doc_id).expect_err("compare must error in slim mode");
    assert!(err.contains("slim mode"), "compare error must mention slim mode; got: {}", err);
}

#[test]
fn restore_errors_clearly_when_no_tombstone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let doc_path = tmp.path().join("slim.docx");
    std::fs::write(&doc_path, build_minimal_docx("slim mode")).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &[], false, "admin@test").expect("ingest slim");

    let out_dir = tmp.path().join("export");
    std::fs::create_dir_all(&out_dir).unwrap();
    let err = vault.restore(&doc_id, out_dir.to_str().unwrap()).expect_err("must error");
    assert!(err.contains("slim mode"),
        "error must mention 'slim mode' (the explicit contract phrase); got: {}", err);
}
