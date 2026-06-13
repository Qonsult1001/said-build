//! End-to-end: extract a real .docx the same way the WASM search-ingest does,
//! write per-paragraph frames, then confirm list_search_documents() surfaces
//! the source document. Guards the "recently ingested: 1 but no documents yet"
//! symptom — proves a real docx yields paragraphs that the list groups.

#![cfg(feature = "docx")]

use sca_core::said_file::SaidFile;
use std::path::Path;

#[test]
fn real_docx_ingest_shows_in_search_document_list() {
    // A small real .docx from the test corpus.
    let docx = Path::new(
        "../../docs/superpowers/test/JHB207. NOMAGEBA TRADING CC TA NOMAGEBA MEATS vs  MAXIMA 5/Acknowledgement of receipt.docx",
    );
    if !docx.exists() {
        eprintln!("skipping: sample docx not present at {:?}", docx);
        return;
    }
    let bytes = std::fs::read(docx).expect("read docx");
    let filename = "Acknowledgement of receipt.docx";

    // Mirror ingest_document_search exactly.
    let mut texts: Vec<String> = Vec::new();
    sca_core::document_ingest::extract_docx_bytes(&bytes, |seg| texts.push(seg.text))
        .expect("extract docx");
    assert!(
        !texts.is_empty(),
        "a real docx must yield at least one paragraph (else the UI shows \
         'recently ingested' with an empty list)"
    );

    let mut brain = SaidFile::create("/in-memory/");
    let base = filename.replace('.', "_");
    for (i, text) in texts.iter().enumerate() {
        brain.remember_as(&format!("{}::para_{}", base, i), text, Some(filename));
    }

    let docs = brain.list_search_documents();
    assert_eq!(docs.len(), 1, "exactly one source document");
    assert_eq!(docs[0].filename, filename);
    assert_eq!(docs[0].segments, texts.len());
}
