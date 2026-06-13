//! Read-path authorization enforcement — authorize_doc against a real
//! ingested doc's manifest tags.

use said_vault::SaidVault;
use said_vault::access::Role;
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
fn authorize_doc_allows_matching_tag_and_operation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let doc_path = tmp.path().join("legal.docx");
    std::fs::write(&doc_path, build_minimal_docx("legal doc")).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(
        doc_path.to_str().unwrap(),
        &["dept:legal".into()],
        true,
        "admin@test",
    ).expect("ingest");

    let legal_reader = Role {
        role: "legal-reader".into(),
        allows: vec!["dept:legal".into()],
        denies: vec![],
        operations: vec!["read".into(), "rebuild".into()],
    };

    // Allowed: tag matches + operation listed.
    assert!(vault.authorize_doc(&[legal_reader.clone()], &doc_id, "read").is_ok());
    assert!(vault.authorize_doc(&[legal_reader.clone()], &doc_id, "rebuild").is_ok());

    // Denied: operation not in the role's list.
    let err = vault.authorize_doc(&[legal_reader], &doc_id, "restore").unwrap_err();
    assert!(err.contains("permission denied"), "got: {}", err);
}

#[test]
fn authorize_doc_denies_tag_outside_allows() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let doc_path = tmp.path().join("hr.docx");
    std::fs::write(&doc_path, build_minimal_docx("hr doc")).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(
        doc_path.to_str().unwrap(),
        &["dept:hr".into()],
        true,
        "admin@test",
    ).expect("ingest");

    // legal-reader can't touch an hr-tagged doc.
    let legal_reader = Role {
        role: "legal-reader".into(),
        allows: vec!["dept:legal".into()],
        denies: vec![],
        operations: vec!["read".into(), "rebuild".into()],
    };
    let err = vault.authorize_doc(&[legal_reader], &doc_id, "read").unwrap_err();
    assert!(err.contains("permission denied"), "got: {}", err);
}

#[test]
fn authorize_operation_gates_corpus_wide_reads() {
    // A role permitting `read` with a non-empty allows can do corpus-wide reads;
    // a role missing `read`, or with empty allows, cannot.
    let reader = Role {
        role: "reader".into(),
        allows: vec!["*".into()],
        denies: vec![],
        operations: vec!["read".into()],
    };
    assert!(SaidVault::authorize_operation(&[reader.clone()], "read"));
    assert!(!SaidVault::authorize_operation(&[reader], "ingest"));

    let no_allows = Role {
        role: "empty".into(),
        allows: vec![],
        denies: vec![],
        operations: vec!["read".into()],
    };
    assert!(!SaidVault::authorize_operation(&[no_allows], "read"));
}
