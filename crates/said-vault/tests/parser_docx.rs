//! DOCX parser contract — ports vault-rust's parser tests with the
//! fidelity fixes already applied (preserve-by-default, word/document.xml
//! preserved verbatim).

use said_vault::parser::docx;
use std::io::Write;

fn build_test_docx() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(cursor);
    let opts = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let files = [
        ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
        ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
        ("word/document.xml", r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello vault</w:t></w:r></w:p></w:body></w:document>"#),
        // Audit-gap entry from prior fidelity work: docProps/core.xml MUST be preserved (100% of corpus lost it pre-fix)
        ("docProps/core.xml", r#"<?xml version="1.0"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"><dc:creator xmlns:dc="http://purl.org/dc/elements/1.1/">Test Author</dc:creator></cp:coreProperties>"#),
        // Audit-gap entry: word/numbering.xml MUST be preserved (96% of corpus lost it pre-fix)
        ("word/numbering.xml", r#"<?xml version="1.0"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#),
        // Architectural guard: an entry the audit didn't enumerate. The
        // preserve-by-default contract says this must round-trip even though
        // the parser has never heard of it.
        ("unexpected/extra.xml", r#"<?xml version="1.0"?><custom/>"#),
    ];
    for (name, content) in &files {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(content.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

#[test]
fn parser_extracts_paragraph_text() {
    let data = build_test_docx();
    let result = docx::parse(&data).expect("parse");
    let texts: Vec<&str> = result.paragraphs.iter().map(|p| p.text.as_str()).collect();
    assert!(texts.contains(&"Hello vault"), "got: {:?}", texts);
}

#[test]
fn parser_preserves_docprops_core_xml() {
    // Fidelity audit showed 216/216 corpus docs lost docProps pre-fix.
    // This is the regression guard.
    let data = build_test_docx();
    let result = docx::parse(&data).expect("parse");
    let names: Vec<&str> = result.structural_xml.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"docProps/core.xml"),
        "docProps/core.xml MUST be in structural_xml; got: {:?}", names);
}

#[test]
fn parser_preserves_word_numbering_xml() {
    let data = build_test_docx();
    let result = docx::parse(&data).expect("parse");
    let names: Vec<&str> = result.structural_xml.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"word/numbering.xml"),
        "word/numbering.xml MUST be preserved; got: {:?}", names);
}

#[test]
fn parser_preserves_unknown_entries_by_default() {
    let data = build_test_docx();
    let result = docx::parse(&data).expect("parse");
    let names: Vec<&str> = result.structural_xml.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"unexpected/extra.xml"),
        "unknown entries MUST round-trip (preserve-by-default contract); got: {:?}", names);
}

#[test]
fn parser_preserves_word_document_xml_verbatim() {
    let data = build_test_docx();
    let result = docx::parse(&data).expect("parse");
    let doc_xml = result.structural_xml.iter().find(|s| s.name == "word/document.xml")
        .expect("word/document.xml MUST be preserved alongside paragraph extraction");
    assert!(std::str::from_utf8(&doc_xml.data).unwrap().contains("Hello vault"));
}
