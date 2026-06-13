//! Stream-1 (search) ingest writes one frame per paragraph as
//! `{base}::para_{i}` with the original filename as title. The UI needs to
//! show one row per *source document* (not per paragraph) so a user can
//! confirm a search-ingest landed. `list_search_documents()` groups the
//! per-paragraph frames back into source documents with a segment count.

use sca_core::said_file::SaidFile;

#[test]
fn groups_paragraphs_into_source_documents() {
    let mut brain = SaidFile::create("/in-memory/");

    // Simulate what ingest_document_search does: para frames per file.
    for i in 0..3 {
        brain.remember_as(&format!("report_docx::para_{}", i), "body", Some("report.docx"));
    }
    for i in 0..2 {
        brain.remember_as(&format!("notice_docx::para_{}", i), "body", Some("notice.docx"));
    }

    let mut docs = brain.list_search_documents();
    docs.sort_by(|a, b| a.filename.cmp(&b.filename));

    assert_eq!(docs.len(), 2, "two distinct source documents");
    assert_eq!(docs[0].filename, "notice.docx");
    assert_eq!(docs[0].segments, 2);
    assert_eq!(docs[1].filename, "report.docx");
    assert_eq!(docs[1].segments, 3);
}

#[test]
fn ignores_non_paragraph_frames() {
    let mut brain = SaidFile::create("/in-memory/");
    brain.remember_as("just-a-memory", "hello", None);
    brain.remember_as("report_docx::para_0", "body", Some("report.docx"));

    let docs = brain.list_search_documents();
    assert_eq!(docs.len(), 1, "only ::para_ frames count as search documents");
    assert_eq!(docs[0].filename, "report.docx");
}

#[test]
fn forget_search_document_purges_only_that_doc() {
    let mut brain = SaidFile::create("/in-memory/");
    for i in 0..3 {
        brain.remember_as(&format!("report_docx::para_{}", i), "body", Some("report.docx"));
    }
    for i in 0..2 {
        brain.remember_as(&format!("notice_docx::para_{}", i), "body", Some("notice.docx"));
    }
    brain.remember_as("just-a-memory", "keep me", None);

    let removed = brain.forget_search_document("report.docx");
    assert_eq!(removed, 3, "all 3 report paragraphs removed");

    let docs = brain.list_search_documents();
    assert_eq!(docs.len(), 1, "only notice.docx remains as a search document");
    assert_eq!(docs[0].filename, "notice.docx");
    // The ordinary memory is untouched.
    assert!(brain.read("just-a-memory").is_some());
}
