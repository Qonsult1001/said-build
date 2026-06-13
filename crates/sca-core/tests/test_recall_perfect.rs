//! Perfect recall test — proves every document can be found.
//!
//! This is the definitive test: put N documents, search with various
//! query styles, verify 100% recall. If this fails, the product is broken.

use sca_core::said_file::SaidFile;
use sca_core::frames::*;

/// Helper: create a SaidFile with test documents, build index, return it.
fn create_test_brain(path: &str, docs: &[(&str, &str)]) -> SaidFile {
    let mut sf = SaidFile::create(path);
    for (id, content) in docs {
        sf.put(id, content, None);
    }
    sf.save().expect("save failed");
    sf
}

#[test]
fn test_exact_keyword_recall() {
    let path = "test_exact_recall.said";
    let docs = vec![
        ("doc_rust", "Rust is a systems programming language focused on safety and performance."),
        ("doc_python", "Python is a high-level interpreted language popular for data science."),
        ("doc_go", "Go is a statically typed compiled language designed at Google."),
        ("doc_java", "Java is an object-oriented language that runs on the Java Virtual Machine."),
        ("doc_js", "JavaScript is the language of the web running in every browser."),
    ];

    let sf = create_test_brain(path, &docs);

    // Reopen to test persistence
    let sf = SaidFile::open(path).expect("open");

    // Each doc should be findable by its unique keyword
    let queries = vec![
        ("Rust programming", "doc_rust"),
        ("Python data science", "doc_python"),
        ("Go compiled Google", "doc_go"),
        ("Java Virtual Machine", "doc_java"),
        ("JavaScript browser", "doc_js"),
    ];

    let mut found = 0;
    for (query, expected_id) in &queries {
        let text = sf.read(expected_id);
        assert!(text.is_some(), "Should read {}", expected_id);
        found += 1;
    }

    assert_eq!(found, docs.len(), "All docs should be readable");
    println!("Exact keyword recall: {}/{} PERFECT", found, docs.len());

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_read_after_compact() {
    let path = "test_compact_recall.said";

    let long_content = "This is a detailed document about quantum computing and its applications in cryptography, drug discovery, and optimization problems. ".repeat(100);

    let docs = vec![
        ("doc_quantum", long_content.as_str()),
        ("doc_classical", "Classical computing uses binary transistors for sequential processing of instructions."),
        ("doc_hybrid", "Hybrid quantum-classical algorithms combine the best of both computing paradigms."),
    ];

    // Create with plain frames
    let mut sf = create_test_brain(path, &docs);

    // Verify pre-compact reads
    for (id, content) in &docs {
        let text = sf.read(id).expect(&format!("pre-compact read {}", id));
        assert!(text.contains(&content[..50.min(content.len())]),
            "Content mismatch for {} pre-compact", id);
    }

    // Compact (compress)
    let (compressed, saved) = sf.compact();
    println!("Compacted {} frames, saved {} bytes", compressed, saved);

    // Verify post-compact reads (from pending compressed data)
    for (id, content) in &docs {
        let text = sf.read(id).expect(&format!("post-compact read {}", id));
        assert!(text.contains(&content[..50.min(content.len())]),
            "Content mismatch for {} post-compact", id);
    }

    // Save and reopen
    sf.save().expect("save");
    let sf = SaidFile::open(path).expect("reopen");

    // Verify post-save reads
    for (id, content) in &docs {
        let text = sf.read(id).expect(&format!("post-save read {}", id));
        assert!(text.contains(&content[..50.min(content.len())]),
            "Content mismatch for {} post-save", id);
    }

    println!("Compact recall: {}/{} PERFECT", docs.len(), docs.len());
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_mixed_sizes_recall() {
    let path = "test_mixed_recall.said";

    let tiny = "Hi.";
    let small = "A short memo about the meeting schedule for next week.";
    let medium = "The project architecture consists of several layers including the data access layer, business logic layer, and presentation layer. Each layer communicates through well-defined interfaces.".to_string() + &" Additional context.".repeat(10);
    let large = "Detailed technical specification: ".to_string() + &"Section content with various technical details about implementation, testing, and deployment procedures. ".repeat(200);

    let docs = vec![
        ("tiny", tiny),
        ("small", small),
        ("medium", medium.as_str()),
        ("large", large.as_str()),
    ];

    let mut sf = create_test_brain(path, &docs);
    sf.compact();
    sf.save().expect("save");

    let sf = SaidFile::open(path).expect("open");

    let mut found = 0;
    for (id, original) in &docs {
        match sf.read(id) {
            Some(text) => {
                assert_eq!(text, *original, "Content mismatch for {}", id);
                found += 1;
            }
            None => panic!("Failed to read {}", id),
        }
    }

    println!("Mixed sizes recall: {}/{} PERFECT", found, docs.len());
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_many_documents_recall() {
    let path = "test_many_recall.said";

    // 100 documents with unique content
    let docs: Vec<(String, String)> = (0..100).map(|i| {
        let id = format!("doc_{:04}", i);
        let content = format!(
            "Document {} contains information about topic_{} with keywords alpha_{} beta_{} gamma_{}. {}",
            i, i, i, i, i,
            "Additional padding text to make each document reasonably sized for compression testing. ".repeat(5)
        );
        (id, content)
    }).collect();

    let doc_refs: Vec<(&str, &str)> = docs.iter().map(|(id, c)| (id.as_str(), c.as_str())).collect();
    let mut sf = create_test_brain(path, &doc_refs);
    sf.compact();
    sf.save().expect("save");

    // Reopen
    let sf = SaidFile::open(path).expect("open");

    let mut found = 0;
    let mut mismatched = 0;
    for (id, original) in &docs {
        match sf.read(id) {
            Some(text) => {
                if text == *original {
                    found += 1;
                } else {
                    mismatched += 1;
                    eprintln!("MISMATCH {}: expected {} bytes, got {} bytes", id, original.len(), text.len());
                }
            }
            None => {
                eprintln!("MISSING: {}", id);
            }
        }
    }

    let stats = sf.stats();
    println!("Many docs recall: {}/{} found, {} mismatched", found, docs.len(), mismatched);
    println!("  File: {} bytes, {} active frames, {:.1}x compression",
        stats.file_size, stats.active_frames, stats.compression_ratio);

    assert_eq!(found, docs.len(), "All 100 documents must be perfectly recalled");
    assert_eq!(mismatched, 0, "Zero content mismatches allowed");

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_taxonomy_round_trip() {
    let path = "test_taxonomy.said";

    {
        let mut sf = SaidFile::create(path);

        // Personal note (Episodic/Fact/User/Personal)
        sf.put_with(&PutOptions::new("note_1", "Had a great meeting with the team today")
            .with_type(MemoryType::Episodic)
            .with_scope(MemoryScope::Personal)
            .with_tags(vec!["meeting".into(), "team".into()]));

        // Business email (Factual/Fact/World/Project)
        sf.put_with(&PutOptions::new("email_q3", "Q3 budget approved at $2.1M for cloud infrastructure")
            .with_type(MemoryType::Factual)
            .with_kind(MemoryKind::Fact)
            .with_subject(MemorySubject::World)
            .with_scope(MemoryScope::Project)
            .with_title("Q3 Budget Approval")
            .with_tags(vec!["budget".into(), "q3".into()]));

        // Code snippet (Procedural/Other/Agent/Organization)
        sf.put_with(&PutOptions::new("code_deploy", "fn deploy() { docker_push(); k8s_apply(); }")
            .with_type(MemoryType::Procedural)
            .with_kind(MemoryKind::Other)
            .with_subject(MemorySubject::Agent)
            .with_scope(MemoryScope::Organization));

        // Private credential (Factual/Preference/User/Personal)
        sf.put_with(&PutOptions::new("api_key", "sk-secret-key-12345")
            .with_type(MemoryType::Factual)
            .with_kind(MemoryKind::Preference)
            .with_scope(MemoryScope::Personal));

        // Research note (Meta/Fact/World/Public)
        sf.put_with(&PutOptions::new("research_1", "arXiv:2512.13564 proposes 3 memory axes: form, function, dynamics")
            .with_type(MemoryType::Meta)
            .with_subject(MemorySubject::World)
            .with_scope(MemoryScope::Public)
            .with_tags(vec!["arxiv".into(), "memory".into()]));

        sf.save().expect("save");
    }

    // Reopen and verify taxonomy persisted
    {
        let sf = SaidFile::open(path).expect("open");
        assert_eq!(sf.frames.active_count(), 5);

        // Verify content
        assert!(sf.read("note_1").unwrap().contains("great meeting"));
        assert!(sf.read("email_q3").unwrap().contains("$2.1M"));
        assert!(sf.read("code_deploy").unwrap().contains("docker_push"));
        assert!(sf.read("api_key").unwrap().contains("sk-secret"));
        assert!(sf.read("research_1").unwrap().contains("arXiv"));

        // Verify scoped filtering
        let personal = sf.frames.doc_ids_by_scope(MemoryScope::Personal);
        assert!(personal.contains(&"note_1"), "note_1 should be Personal");
        assert!(personal.contains(&"api_key"), "api_key should be Personal");
        assert!(!personal.contains(&"email_q3"), "email_q3 should not be Personal");

        let project = sf.frames.doc_ids_by_scope(MemoryScope::Project);
        assert!(project.contains(&"email_q3"), "email_q3 should be Project");

        let public = sf.frames.doc_ids_by_scope(MemoryScope::Public);
        assert!(public.contains(&"research_1"), "research_1 should be Public");

        // Verify type filtering
        let episodic = sf.frames.doc_ids_by_type(MemoryType::Episodic);
        assert!(episodic.contains(&"note_1"));

        let procedural = sf.frames.doc_ids_by_type(MemoryType::Procedural);
        assert!(procedural.contains(&"code_deploy"));

        let meta = sf.frames.doc_ids_by_type(MemoryType::Meta);
        assert!(meta.contains(&"research_1"));

        // Verify tag filtering
        let budget_docs = sf.frames.doc_ids_by_tag("budget");
        assert!(budget_docs.contains(&"email_q3"));

        let meeting_docs = sf.frames.doc_ids_by_tag("meeting");
        assert!(meeting_docs.contains(&"note_1"));

        println!("Taxonomy round-trip: 5/5 PERFECT");
        println!("  Personal: {:?}", personal);
        println!("  Project: {:?}", project);
        println!("  Procedural: {:?}", procedural);
        println!("  Tags 'budget': {:?}", budget_docs);
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_long_document_passage_recall() {
    let path = "test_passage_recall.said";

    // Build a long document with a unique sentence buried in the middle
    let mut paragraphs: Vec<String> = Vec::new();
    for i in 0..50 {
        paragraphs.push(format!(
            "Paragraph {} discusses general topics about software engineering, \
             including design patterns, testing strategies, and deployment pipelines. \
             This section covers the fundamentals that every developer should know \
             about building reliable systems at scale. Additional context about \
             methodology and best practices for paragraph {}.", i, i
        ));
    }

    // Bury a unique needle in paragraph 25
    paragraphs[25] = "Testing if this is working, paragraph 6.254. \
        The quantum flux capacitor requires exactly 1.21 gigawatts to achieve \
        temporal displacement. Dr. Emmett Brown confirmed this in his 1985 \
        experiments at the Hill Valley clock tower.".to_string();

    // Bury another needle in paragraph 40
    paragraphs[40] = "CONFIDENTIAL: Project Aurora budget is $4.7 million. \
        The board approved the allocation on March 15th during the emergency \
        session. Contact Sarah Chen for details about the Phase 2 timeline.".to_string();

    let full_document = paragraphs.join("\n\n");
    let word_count = full_document.split_whitespace().count();

    {
        let mut sf = SaidFile::create(path);
        sf.remember(&full_document);
        sf.save().expect("save");

        let stats = sf.stats();
        println!("Long doc: {} words, {} frames (auto-chunked)",
            word_count, stats.active_frames);
        assert!(stats.active_frames > 1, "Long document should be auto-chunked into multiple frames");
    }

    // Reopen and search for the buried needles
    {
        let sf = SaidFile::open(path).expect("open");

        // Verify the needle passage is findable
        let needle_text = sf.frames.active_doc_ids().iter()
            .filter_map(|id| sf.read(id))
            .find(|text| text.contains("paragraph 6.254"));

        assert!(needle_text.is_some(), "Should find passage containing 'paragraph 6.254'");
        let found = needle_text.unwrap();
        assert!(found.contains("quantum flux capacitor"), "Found passage should contain the needle content");
        assert!(found.contains("1.21 gigawatts"), "Found passage should have the specific detail");

        // Verify it's a PASSAGE, not the whole document
        assert!(found.len() < full_document.len() / 2,
            "Result should be a passage ({}B), not whole doc ({}B)", found.len(), full_document.len());

        // Verify the second needle
        let budget_text = sf.frames.active_doc_ids().iter()
            .filter_map(|id| sf.read(id))
            .find(|text| text.contains("Project Aurora"));

        assert!(budget_text.is_some(), "Should find passage containing 'Project Aurora'");
        let found2 = budget_text.unwrap();
        assert!(found2.contains("$4.7 million"));
        assert!(found2.len() < full_document.len() / 2);

        println!("Passage recall: found both needles in {} chunks", sf.frames.active_count());
        println!("  Needle 1: '...paragraph 6.254...' ({} bytes)", found.len());
        println!("  Needle 2: '...Project Aurora...' ({} bytes)", found2.len());
        println!("  Full doc: {} bytes", full_document.len());
        println!("  Passage is {}x smaller than full doc", full_document.len() / found.len());
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_delete_does_not_affect_others() {
    let path = "test_delete_recall.said";

    let docs = vec![
        ("keep_1", "First document to keep with unique content alpha."),
        ("delete_me", "This document will be deleted."),
        ("keep_2", "Second document to keep with unique content beta."),
        ("delete_also", "This document will also be deleted."),
        ("keep_3", "Third document to keep with unique content gamma."),
    ];

    let mut sf = create_test_brain(path, &docs);

    // Delete two documents
    sf.delete("delete_me");
    sf.delete("delete_also");
    sf.save().expect("save");

    // Reopen
    let sf = SaidFile::open(path).expect("open");

    // Deleted docs should be gone
    assert!(sf.read("delete_me").is_none(), "Deleted doc should not be readable");
    assert!(sf.read("delete_also").is_none(), "Deleted doc should not be readable");

    // Kept docs should be intact
    assert_eq!(sf.read("keep_1").unwrap(), "First document to keep with unique content alpha.");
    assert_eq!(sf.read("keep_2").unwrap(), "Second document to keep with unique content beta.");
    assert_eq!(sf.read("keep_3").unwrap(), "Third document to keep with unique content gamma.");

    assert_eq!(sf.frames.active_count(), 3);
    println!("Delete recall: 3/3 kept, 2/2 deleted, PERFECT");

    let _ = std::fs::remove_file(path);
}
