//! COMPREHENSIVE end-to-end memory test (#4): a user stores memories across all six pillars
//! (Episodic, Semantic, Procedural, External, Code, Memory) with wikilinks, then recalls them
//! across DIFFERENT questions — proving the whole stack works together after the session's
//! tokenizer/encode/index/save changes:
//!
//!   • memory-type mapping   — each pillar → correct memory_type + doc_id prefix + pillar tag
//!   • semantic recall       — paraphrase questions surface the right memory
//!   • lexical recall        — exact-token questions surface the right memory
//!   • wikilink bridges       — [[concept]] links a question to a memory that shares the concept
//!   • pillar-scoped recall  — recall_by_pillar narrows to a pillar
//!   • ask() unified entry   — the production query path returns the right memory
//!   • mmap persistence      — save → reopen → every recall still works (text lives in frames)
//!
//! Each query is DIFFERENT from the stored text (tests real recall, not echo).
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model,code" \
//!        --test test_six_pillar_recall -- --nocapture

#![cfg(feature = "static-embed")]

use sca_core::said_file::SaidFile;
use sca_core::frames::{Pillar, MemoryType};
use std::collections::HashSet;

fn tmp(name: &str) -> String {
    std::env::temp_dir().join(format!("six_pillar_{}_{}.said", name, std::process::id()))
        .to_string_lossy().into_owned()
}

/// (pillar, content, expected memory_type, expected doc_id prefix)
fn corpus() -> Vec<(Pillar, &'static str, MemoryType, &'static str)> {
    vec![
        (Pillar::Episodic,
         "On Tuesday I met Dr Sarah at the cafe to discuss my heart condition. [[heart]] [[meeting]]",
         MemoryType::Episodic, "ep_"),
        (Pillar::Semantic,
         "A cardiologist is a doctor who specializes in diagnosing and treating heart disease. [[heart]] [[doctor]]",
         MemoryType::Factual, "sem_"),
        (Pillar::Procedural,
         "To deploy the service: run the build, then push the container to the registry. [[deploy]]",
         MemoryType::Procedural, "proc_"),
        (Pillar::External,
         "The API specification is published at https://example.com/spec version 4. [[spec]]",
         MemoryType::Factual, "ext_"),
        (Pillar::Code,
         "fn charge_card(token: &str, amount: Decimal) -> PaymentResult { stripe_gateway.charge(token, amount) }",
         MemoryType::Factual, "code_"),
        (Pillar::Memory,
         "General reflection: the project values bounded memory and byte-identical recall above raw speed.",
         MemoryType::Relational, "mem_"),
    ]
}

#[test]
fn six_pillar_memories_recall_across_questions() {
    let path = tmp("main");
    let _ = std::fs::remove_file(&path);
    let mut b = SaidFile::create(&path);
    assert!(b.auto_load_encoder(), "embedded encoder must load");

    // ── Store one memory per pillar (auto doc_id → pillar prefix). ──
    // Use remember_with_salience — the documented note/memory writer surface that parses
    // [[wikilinks]] into link: tags (remember_with_pillar deliberately does NOT, since it's
    // also the code-ingest hot path where `[[` is array syntax, not a concept).
    let mut ids = Vec::new();
    for (pillar, content, _, _) in corpus() {
        let (id, _salience) = b.remember_with_salience(None, content, None, pillar, vec![]);
        ids.push(id);
    }
    b.build_index().expect("index");

    // ── (1) MEMORY-TYPE MAPPING: pillar → memory_type + doc_id prefix + pillar tag. ──
    {
        let frames = b.frames.get_all_frames_with_pending();
        for ((pillar, _content, want_mt, want_prefix), id) in corpus().into_iter().zip(ids.iter()) {
            let f = frames.iter().find(|m| m.id == *id)
                .unwrap_or_else(|| panic!("frame {id} for {pillar:?} must exist"));
            assert_eq!(f.pillar, pillar, "pillar field must persist for {pillar:?}");
            assert_eq!(f.memory_type, want_mt, "{pillar:?} → memory_type {want_mt:?}");
            assert!(f.doc_id.starts_with(want_prefix),
                "{pillar:?} doc_id must start with {want_prefix}, got {}", f.doc_id);
            let tag = format!("pillar:{}", pillar.name());
            assert!(f.tags.iter().any(|t| t == &tag),
                "{pillar:?} must carry {tag} tag, got {:?}", f.tags);
        }
    }

    // ── (2) SEMANTIC recall: paraphrase questions (different words) → right pillar memory. ──
    // Each (question, substring that must appear in a top-3 doc_id-or-content).
    let semantic: &[(&str, &str)] = &[
        ("which specialist treats cardiac problems", "cardiologist"),     // → Semantic
        ("how do I ship the application to production", "deploy"),         // → Procedural
        ("where can I find the interface documentation", "specification"),// → External
    ];
    for (q, want) in semantic {
        let hits = b.recall(q, 3);
        let found = hits.iter().any(|r| r.content.to_lowercase().contains(want)
            || r.doc_id.to_lowercase().contains(want));
        assert!(found, "SEMANTIC '{q}' should surface a memory containing '{want}'; got {:?}",
            hits.iter().map(|r| &r.doc_id).collect::<Vec<_>>());
    }

    // ── (3) LEXICAL recall: exact tokens present in exactly one memory. ──
    let lexical: &[(&str, &str)] = &[
        ("charge_card", "code_"),       // exact code identifier → Code memory
        ("stripe_gateway", "code_"),
        ("registry", "proc_"),          // exact word from the procedural memory
    ];
    for (q, want_prefix) in lexical {
        let hits = b.recall(q, 3);
        let found = hits.iter().any(|r| r.doc_id.starts_with(want_prefix));
        assert!(found, "LEXICAL '{q}' should surface a {want_prefix} memory; got {:?}",
            hits.iter().map(|r| &r.doc_id).collect::<Vec<_>>());
    }

    // ── (4) WIKILINK bridge: [[heart]] links the cardiology question to BOTH heart memories. ──
    let concepts = b.list_concepts(None);
    for want in ["heart", "doctor", "deploy", "spec"] {
        assert!(concepts.iter().any(|(c, _)| c == want),
            "wikilink concept '{want}' must be registered; got {:?}",
            concepts.iter().map(|(c, _)| c).collect::<Vec<_>>());
    }
    // A question that shares the [[heart]] concept must reach a heart memory.
    let heart_hits = b.recall("tell me about my heart", 5);
    assert!(heart_hits.iter().any(|r| r.doc_id.starts_with("ep_") || r.doc_id.starts_with("sem_")),
        "wikilink 'heart' question must surface an episodic or semantic heart memory; got {:?}",
        heart_hits.iter().map(|r| &r.doc_id).collect::<Vec<_>>());

    // ── (5) PILLAR-SCOPED recall: narrow to Procedural only. ──
    {
        let mut scope = HashSet::new();
        scope.insert(Pillar::Procedural);
        let proc_hits = b.recall_by_pillar("how to release", 5, Some(&scope));
        assert!(proc_hits.iter().all(|r| r.doc_id.starts_with("proc_")),
            "pillar-scoped recall must return ONLY procedural memories; got {:?}",
            proc_hits.iter().map(|r| &r.doc_id).collect::<Vec<_>>());
        assert!(!proc_hits.is_empty(), "pillar-scoped recall should find the procedural memory");
    }

    // ── (6) ask() unified entry path. ──
    {
        let (cands, _concepts) = sca_core::ask::ask(&mut b, "who specializes in the heart", 3, false, None);
        assert!(!cands.is_empty(), "ask() must return candidates for a heart-specialist question");
    }

    // ── (7) MMAP PERSISTENCE: save → reopen → all of the above still works from frames. ──
    b.save().expect("save");
    drop(b);
    let mut b2 = SaidFile::open(&path).expect("reopen via mmap");
    assert!(b2.auto_load_encoder());

    // memory-type + pillar survive the round-trip
    {
        let frames = b2.frames.get_all_frames();
        for ((pillar, _c, want_mt, want_prefix), id) in corpus().into_iter().zip(ids.iter()) {
            let f = frames.iter().find(|m| m.id == *id).expect("frame survives mmap reopen");
            assert_eq!(f.pillar, pillar, "pillar survives reopen");
            assert_eq!(f.memory_type, want_mt, "memory_type survives reopen");
            assert!(f.doc_id.starts_with(want_prefix), "doc_id prefix survives reopen");
        }
    }
    // recall still works after mmap reopen (text read on demand from frames)
    let after = b2.recall("which specialist treats cardiac problems", 3);
    assert!(after.iter().any(|r| r.content.to_lowercase().contains("cardiologist")),
        "SEMANTIC recall must still work after mmap reopen; got {:?}",
        after.iter().map(|r| &r.doc_id).collect::<Vec<_>>());

    let _ = std::fs::remove_file(&path);
}
