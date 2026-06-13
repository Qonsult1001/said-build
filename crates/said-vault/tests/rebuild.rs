//! Rebuild contract — verify the DOCX rebuilder uses preserved
//! word/document.xml verbatim (the table-fix contract from prior work).

use said_vault::rebuild::{self, RebuildRef};
use said_vault::parser::docx as parse_docx;
use std::io::{Read, Write};

fn build_test_docx_with_table() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(cursor);
    let opts = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let doc_xml = r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Intro</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell text</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
    let files = [
        ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
        ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
        ("word/document.xml", doc_xml),
    ];
    for (name, content) in &files {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(content.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

#[test]
fn rebuilt_docx_preserves_table_structure() {
    let original_bytes = build_test_docx_with_table();
    let parsed = parse_docx::parse(&original_bytes).expect("parse");

    // Convert parsed components into the rebuild input shape.
    // The DOCX rebuilder needs: every structural_xml entry as a "xml"
    // RebuildRef with its original name preserved, plus paragraph refs
    // (kind="paragraph") for any paragraphs the rebuilder might emit as
    // fallback when word/document.xml isn't in the refs.
    //
    // For this test we want the rebuilder to use word/document.xml VERBATIM
    // (the table-fix contract), so we only need to pass the structural_xml
    // refs — paragraph refs are ignored when word/document.xml is present.
    let mut refs: Vec<RebuildRef> = Vec::new();
    for x in &parsed.structural_xml {
        refs.push(RebuildRef {
            kind: "xml".into(),
            name: Some(x.name.clone()),
            bytes: x.data.clone(),
        });
    }
    for p in &parsed.paragraphs {
        refs.push(RebuildRef {
            kind: "paragraph".into(),
            name: None,
            bytes: p.text.as_bytes().to_vec(),
        });
    }

    let rebuilt = rebuild::docx::rebuild(&refs).expect("rebuild");

    // Verify the rebuilt zip contains word/document.xml with <w:tbl>
    let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(&rebuilt)).expect("zip");
    let mut doc_xml = String::new();
    zip_reader.by_name("word/document.xml").expect("word/document.xml")
        .read_to_string(&mut doc_xml).expect("read");

    assert!(doc_xml.contains("<w:tbl>"),
        "rebuilt word/document.xml MUST contain the original <w:tbl> wrapper; got first 500 chars:\n{}",
        &doc_xml[..doc_xml.len().min(500)]);
    assert!(doc_xml.contains("Cell text"));
}

#[test]
fn rebuild_with_no_document_xml_falls_back_to_paragraph_assembly() {
    // When word/document.xml is NOT in the refs (legacy vault format), the
    // rebuilder should fall back to regenerating document.xml from paragraph
    // refs. The fallback won't preserve tables, but it preserves text.
    let refs = vec![
        RebuildRef {
            kind: "paragraph".into(),
            name: None,
            bytes: b"first paragraph".to_vec(),
        },
        RebuildRef {
            kind: "paragraph".into(),
            name: None,
            bytes: b"second paragraph".to_vec(),
        },
    ];

    let rebuilt = rebuild::docx::rebuild(&refs).expect("rebuild");

    // Rebuild should produce a valid zip with word/document.xml
    let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(&rebuilt)).expect("zip");
    let mut doc_xml = String::new();
    zip_reader.by_name("word/document.xml").expect("word/document.xml")
        .read_to_string(&mut doc_xml).expect("read");

    assert!(doc_xml.contains("first paragraph"));
    assert!(doc_xml.contains("second paragraph"));
}

#[test]
fn rebuild_module_exposes_rebuild_ref_type() {
    // Smoke check that the public type compiles + can be constructed.
    let _ = RebuildRef {
        kind: "paragraph".into(),
        name: None,
        bytes: vec![],
    };
}
