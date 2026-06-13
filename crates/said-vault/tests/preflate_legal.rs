//! Legal-tier preflate recipe: ingest→restore must be byte-exact, via the
//! recipe path for DOCX and the verbatim path for non-DOCX.

use said_vault::SaidVault;
use std::io::Write;

fn build_docx(paragraph: &str) -> Vec<u8> {
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
    let body: String = (0..40)
        .map(|i| format!("<w:p><w:r><w:t>{} {}</w:t></w:r></w:p>", paragraph, i))
        .collect();
    let doc = format!(r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}</w:body></w:document>"#, body);
    zw.start_file("word/document.xml", opts).unwrap();
    zw.write_all(doc.as_bytes()).unwrap();
    zw.finish().unwrap().into_inner()
}

#[test]
fn legal_docx_restores_byte_exact_via_recipe() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let doc_path = tmp.path().join("legal.docx");
    let original = build_docx("compliance copy");
    std::fs::write(&doc_path, &original).unwrap();

    let vault_path = tmp.path().join("vault.said");
    let mut vault = SaidVault::init(vault_path.to_str().unwrap(), "admin@test").expect("init");
    let doc_id = vault.ingest(doc_path.to_str().unwrap(), &[], true, "admin@test").expect("ingest");

    let out_dir = tmp.path().join("export");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out_path = vault.restore(&doc_id, out_dir.to_str().unwrap()).expect("restore");

    let restored = std::fs::read(&out_path).unwrap();
    assert_eq!(restored, original, "legal restore MUST be byte-identical to the original docx");
}
